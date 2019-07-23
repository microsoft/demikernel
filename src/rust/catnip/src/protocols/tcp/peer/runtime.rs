use super::{
    super::{
        connection::{TcpConnection, TcpConnectionHandle, TcpConnectionId},
        segment::{TcpSegment, DEFAULT_MSS, MIN_MSS},
    },
    isn_generator::IsnGenerator,
};
use crate::{
    prelude::*,
    protocols::{arp, ip, ipv4},
    r#async::{Future, Retry},
};
use rand::seq::SliceRandom;
use std::{
    any::Any,
    cell::RefCell,
    cmp::min,
    collections::{HashMap, HashSet, VecDeque},
    num::Wrapping,
    rc::Rc,
    time::Duration,
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

    pub fn connect(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        remote_endpoint: ipv4::Endpoint,
    ) -> Future<'a, TcpConnectionHandle> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            let (cxnid, rt) = {
                let mut state = state.borrow_mut();
                let rt = state.rt.clone();
                let options = rt.options();
                let cxnid = TcpConnectionId {
                    local: ipv4::Endpoint::new(
                        options.my_ipv4_addr,
                        state.acquire_private_port()?,
                    ),
                    remote: remote_endpoint,
                };

                let _ = state.new_connection(cxnid.clone(), None)?;
                (cxnid, rt)
            };

            let remote_isn = r#await!(
                TcpRuntime::handshake(&state, &cxnid, None),
                rt.now(),
                Retry::exponential(Duration::from_secs(3), 2, 5)
            )?;

            let ack_num = remote_isn + Wrapping(1);

            let (handle, segment) = {
                let mut state = state.borrow_mut();
                let cxn = state.connections.get_mut(&cxnid).unwrap();
                cxn.seq_num += Wrapping(1);
                let segment = TcpSegment::default()
                    .connection(&cxn)
                    .ack_num(ack_num)
                    .ack();

                (cxn.handle, segment)
            };

            r#await!(TcpRuntime::cast(&state, segment), rt.now())?;

            let x: Rc<dyn Any> = Rc::new(handle);
            Ok(x)
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
            rt.emit_effect(Effect::Transmit(Rc::new(segment.encode())));

            let x: Rc<dyn Any> = Rc::new(());
            Ok(x)
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

                assert!(syn_segment.syn && !syn_segment.ack);
                let local_port = syn_segment.dest_port.unwrap();
                assert!(state.open_ports.contains(&local_port));

                let remote_ipv4_addr = syn_segment.src_ipv4_addr.unwrap();
                let remote_port = syn_segment.src_port.unwrap();
                let cxnid = TcpConnectionId {
                    local: ipv4::Endpoint::new(
                        rt.options().my_ipv4_addr,
                        local_port,
                    ),
                    remote: ipv4::Endpoint::new(remote_ipv4_addr, remote_port),
                };
                let mss = min(DEFAULT_MSS, syn_segment.mss.unwrap_or(MIN_MSS));
                let handle = state.new_connection(cxnid.clone(), Some(mss))?;

                (cxnid, handle, rt)
            };

            let _remote_isn = r#await!(
                TcpRuntime::handshake(
                    &state,
                    &cxnid,
                    Some(syn_segment.seq_num),
                ),
                rt.now()
            )?;

            rt.emit_effect(Effect::TcpConnectionEstablished(handle));

            let x: Rc<dyn Any> = Rc::new(());
            Ok(x)
        })
    }

    fn handshake(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        cxnid: &TcpConnectionId,
        syn_seq_num: Option<Wrapping<u32>>,
    ) -> Future<'a, Wrapping<u32>> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        let cxnid = cxnid.clone();
        rt.start_coroutine(move || {
            trace!("TcpRuntime::handshake({:?})", syn_seq_num);
            let mut ack = false;
            let (segment, rt, incoming_segments) = {
                let state = state.borrow();
                let cxn = state.connections.get(&cxnid).unwrap();
                let mut segment =
                    TcpSegment::default().connection(cxn).mss(cxn.mss).syn();
                if let Some(syn_seq_num) = syn_seq_num {
                    ack = true;
                    segment.ack = true;
                    segment.ack_num = syn_seq_num + Wrapping(1);
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
                        let x: Rc<dyn Any> = Rc::new(segment.seq_num);
                        return Ok(x);
                    }
                }
            }
        })
    }

    fn acquire_private_port(&mut self) -> Result<ip::Port> {
        if let Some(p) = self.unassigned_private_ports.pop_front() {
            Ok(p)
        } else {
            Err(Fail::ResourceExhausted {
                details: "no more private ports",
            })
        }
    }

    fn release_private_port(&mut self, port: ip::Port) {
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
        mss: Option<usize>,
    ) -> Result<TcpConnectionHandle> {
        let handle = self.acquire_connection_handle()?;
        let isn = self.isn_generator.next(&cxnid);
        let cxn = TcpConnection::new(cxnid.clone(), handle, isn, mss);
        let local_port = cxnid.local.port();
        assert!(self.connections.insert(cxnid.clone(), cxn).is_none());
        assert!(self.assigned_handles.insert(handle, cxnid).is_none());
        self.open_ports.insert(local_port);
        Ok(handle)
    }
}
