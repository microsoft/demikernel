use super::{
    super::{
        connection::{TcpConnection, TcpConnectionHandle, TcpConnectionId},
        segment::TcpSegment,
    },
    isn_generator::IsnGenerator,
};
use crate::{
    prelude::*,
    protocols::{arp, ip, ipv4},
    r#async::Retry,
};
use rand::seq::SliceRandom;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    num::Wrapping,
    rc::Rc,
};

pub struct TcpRuntime<'a> {
    arp: arp::Peer<'a>,
    assigned_handles: HashMap<TcpConnectionHandle, TcpConnectionId>,
    connections: HashMap<TcpConnectionId, TcpConnection>,
    isn_generator: IsnGenerator,
    open_ports: HashSet<ip::Port>,
    passive_connections: HashMap<ipv4::Endpoint, TcpConnection>,
    rt: Runtime<'a>,
    unassigned_connection_handles: VecDeque<TcpConnectionHandle>,
    unassigned_private_ports: VecDeque<ip::Port>, // todo: shared state.
}

impl<'a> TcpRuntime<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> TcpRuntime<'a> {
        // initialize the pool of available private ports.
        let unassigned_private_ports = {
            let mut ports = Vec::new();
            for i in ip::Port::first_private_port().into()..65535 {
                ports.push(ip::Port::try_from(i).unwrap());
            }
            let mut rng = rt.rng_mut();
            ports.shuffle(&mut *rng);
            VecDeque::from(ports)
        };

        // initialize the pool of available connection handles.
        let unassigned_connection_handles = {
            let mut handles = Vec::new();
            for i in 1..u16::max_value() {
                handles.push(TcpConnectionHandle::new(i));
            }
            let mut rng = rt.rng_mut();
            handles.shuffle(&mut *rng);
            VecDeque::from(handles)
        };

        let isn_generator = IsnGenerator::new(&rt);

        TcpRuntime {
            arp,
            assigned_handles: HashMap::new(),
            connections: HashMap::new(),
            isn_generator,
            open_ports: HashSet::new(),
            passive_connections: HashMap::new(),
            rt,
            unassigned_connection_handles,
            unassigned_private_ports,
        }
    }

    pub fn rt(&self) -> &Runtime<'a> {
        &self.rt
    }

    pub fn listen(&mut self, port: ip::Port) -> Result<()> {
        if self.open_ports.contains(&port) {
            return Err(Fail::ResourceBusy {
                details: "port already in use",
            });
        }

        assert!(self.open_ports.insert(port));
        Ok(())
    }

    pub fn receive(&self, cxnid: TcpConnectionId, segment: TcpSegment) {
        let cxn = self.connections.get(&cxnid).unwrap();
        cxn.incoming_segments.borrow_mut().push_front(segment);
    }

    pub fn is_port_open(&self, port: ip::Port) -> bool {
        self.open_ports.contains(&port)
    }

    pub fn get_connection(
        &self,
        cxnid: &TcpConnectionId,
    ) -> Option<&TcpConnection> {
        self.connections.get(cxnid)
    }

    pub fn new_active_connection(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        cxnid: TcpConnectionId,
    ) -> Future<'a, TcpConnectionId> {
        trace!("TcpRuntime::new_active_connection(.., {:?})", cxnid);
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            trace!(
                "TcpRuntime::new_active_connection(.., {:?})::coroutine",
                cxnid
            );
            let (cxnid, rt) = {
                let mut state = state.borrow_mut();
                let rt = state.rt.clone();
                let _ = state.new_connection(cxnid.clone())?;
                (cxnid, rt)
            };

            let options = rt.options();
            let retries = options.tcp.handshake_retries();
            let timeout = options.tcp.handshake_timeout();
            let ack_segment = r#await!(
                TcpRuntime::handshake(&state, &cxnid, false),
                rt.now(),
                Retry::exponential(timeout, 2, retries)
            )?;

            let remote_isn = ack_segment.seq_num;

            let segment = {
                let mut state = state.borrow_mut();
                let cxn = state.connections.get_mut(&cxnid).unwrap();
                cxn.set_remote_isn(remote_isn);
                cxn.negotiate_mss(ack_segment.mss)?;
                cxn.seq_num += Wrapping(1);
                TcpSegment::default()
                    .connection(&cxn)
                    .ack_num(cxn.ack_num())
                    .ack()
            };

            r#await!(TcpRuntime::cast(&state, segment), rt.now())?;
            CoroutineOk(cxnid)
        })
    }

    pub fn close_connection(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        cxnid: TcpConnectionId,
        rst: bool,
    ) -> Future<'a, ()> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            let (segment, rt) = {
                let mut state = state.borrow_mut();
                let rt = state.rt.clone();
                let cxn = if let Some(cxn) = state.connections.remove(&cxnid) {
                    cxn
                } else {
                    return Err(Fail::Ignored {});
                };

                let segment = TcpSegment::default().connection(&cxn).rst();
                (segment, rt)
            };

            if rst {
                r#await!(TcpRuntime::cast(&state, segment), rt.now())?;
            }

            CoroutineOk(())
        })
    }

    pub fn cast(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        mut segment: TcpSegment,
    ) -> Future<'a, ()> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            trace!("TcpRuntime::cast({:?})", segment);
            let (arp, rt) = {
                let state = state.borrow();
                (state.arp.clone(), state.rt.clone())
            };

            let remote_ipv4_addr = segment.dest_ipv4_addr.unwrap();
            let remote_link_addr =
                r#await!(arp.query(remote_ipv4_addr), rt.now())?;
            let options = rt.options();
            segment.src_ipv4_addr = Some(options.my_ipv4_addr);
            segment.src_link_addr = Some(options.my_link_addr);
            segment.dest_link_addr = Some(remote_link_addr);
            rt.emit_event(Event::Transmit(Rc::new(segment.encode())));
            CoroutineOk(())
        })
    }

    pub fn new_passive_connection(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        syn_segment: TcpSegment,
    ) -> Future<'a, ()> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            let (cxnid, handle, rt) = {
                let mut state = state.borrow_mut();
                let rt = state.rt.clone();
                let options = rt.options();

                assert!(syn_segment.syn && !syn_segment.ack);
                let local_port = syn_segment.dest_port.unwrap();
                assert!(state.open_ports.contains(&local_port));

                let remote_ipv4_addr = syn_segment.src_ipv4_addr.unwrap();
                let remote_port = syn_segment.src_port.unwrap();
                let cxnid = TcpConnectionId {
                    local: ipv4::Endpoint::new(
                        options.my_ipv4_addr,
                        local_port,
                    ),
                    remote: ipv4::Endpoint::new(remote_ipv4_addr, remote_port),
                };

                let cxn = state.new_connection(cxnid.clone())?;
                cxn.negotiate_mss(syn_segment.mss)?;
                cxn.set_remote_isn(syn_segment.seq_num);
                (cxnid, cxn.handle, rt)
            };

            let options = rt.options();
            let retries = options.tcp.handshake_retries();
            let timeout = options.tcp.handshake_timeout();
            let _ = r#await!(
                TcpRuntime::handshake(&state, &cxnid, true,),
                rt.now(),
                Retry::exponential(timeout, 2, retries)
            )?;

            rt.emit_event(Event::TcpConnectionEstablished(handle));
            CoroutineOk(())
        })
    }

    fn handshake(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        cxnid: &TcpConnectionId,
        ack: bool,
    ) -> Future<'a, Rc<TcpSegment>> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        let cxnid = cxnid.clone();
        rt.start_coroutine(move || {
            trace!("TcpRuntime::handshake({:?})", ack);
            let (segment, rt, incoming_segments) = {
                let state = state.borrow();
                let cxn = state.connections.get(&cxnid).unwrap();
                let mut segment =
                    TcpSegment::default().connection(cxn).mss(cxn.mss()).syn();
                if ack {
                    segment.ack = true;
                    segment.ack_num = cxn.ack_num();
                }
                (segment, state.rt.clone(), cxn.incoming_segments.clone())
            };

            let expected_ack_num = segment.seq_num + Wrapping(1);

            r#await!(TcpRuntime::cast(&state, segment), rt.now())?;

            loop {
                if yield_until!(
                    !incoming_segments.borrow().is_empty(),
                    state.rt.now()
                ) {
                    let segment =
                        incoming_segments.borrow_mut().pop_front().unwrap();
                    if segment.rst {
                        return Err(Fail::ConnectionRefused {});
                    }

                    if segment.ack
                        && ack != segment.syn
                        && segment.ack_num == expected_ack_num
                    {
                        return CoroutineOk(Rc::new(segment));
                    }
                }
            }
        })
    }

    pub fn acquire_private_port(&mut self) -> Result<ip::Port> {
        if let Some(p) = self.unassigned_private_ports.pop_front() {
            Ok(p)
        } else {
            Err(Fail::ResourceExhausted {
                details: "no more private ports",
            })
        }
    }

    pub fn release_private_port(&mut self, port: ip::Port) {
        assert!(port.is_private());
        self.unassigned_private_ports.push_back(port);
    }

    fn acquire_connection_handle(&mut self) -> Result<TcpConnectionHandle> {
        if let Some(h) = self.unassigned_connection_handles.pop_front() {
            Ok(h)
        } else {
            Err(Fail::ResourceExhausted {
                details: "no more connection handles",
            })
        }
    }

    fn release_connection_handle(&mut self, handle: TcpConnectionHandle) {
        self.unassigned_connection_handles.push_back(handle);
    }

    fn new_connection(
        &mut self,
        cxnid: TcpConnectionId,
    ) -> Result<&mut TcpConnection> {
        let options = self.rt.options();
        let handle = self.acquire_connection_handle()?;
        let local_isn = self.isn_generator.next(&cxnid);
        let cxn = TcpConnection::new(
            cxnid.clone(),
            handle,
            local_isn,
            options.tcp.receive_window_size(),
        );
        let local_port = cxnid.local.port();
        assert!(self.connections.insert(cxnid.clone(), cxn).is_none());
        assert!(self
            .assigned_handles
            .insert(handle, cxnid.clone())
            .is_none());
        self.open_ports.insert(local_port);
        Ok(self.connections.get_mut(&cxnid).unwrap())
    }
}
