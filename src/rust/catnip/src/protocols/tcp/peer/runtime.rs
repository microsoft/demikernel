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
    connections: HashMap<TcpConnectionId, Rc<RefCell<TcpConnection<'a>>>>,
    isn_generator: IsnGenerator,
    open_ports: HashSet<ip::Port>,
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

    pub fn enqueue_segment(
        &mut self,
        cxnid: TcpConnectionId,
        segment: TcpSegment,
    ) -> Result<()> {
        if let Some(cxn) = self.connections.get_mut(&cxnid) {
            let mut cxn = cxn.borrow_mut();
            cxn.input_queue_mut().push_back(segment);
            Ok(())
        } else {
            Err(Fail::ResourceNotFound {
                details: "unrecognized connection ID",
            })
        }
    }

    pub fn is_port_open(&self, port: ip::Port) -> bool {
        self.open_ports.contains(&port)
    }

    pub fn get_connection_given_id(
        &self,
        cxnid: &TcpConnectionId,
    ) -> Option<&Rc<RefCell<TcpConnection<'a>>>> {
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

            let (cxn, rt) = {
                let mut state = state.borrow_mut();
                let rt = state.rt().clone();
                (
                    state.new_connection(cxnid.clone(), rt.clone())?,
                    state.rt.clone(),
                )
            };

            let options = rt.options();
            let retries = options.tcp.handshake_retries();
            let timeout = options.tcp.handshake_timeout();
            let ack_segment = r#await!(
                TcpRuntime::handshake(&state, cxn.clone()),
                rt.now(),
                Retry::binary_exponential(timeout, retries)
            )?;

            let remote_isn = ack_segment.seq_num;

            let segment = {
                let mut cxn = cxn.borrow_mut();
                cxn.set_remote_isn(remote_isn);
                cxn.negotiate_mss(ack_segment.mss)?;
                cxn.set_remote_receive_window_size(ack_segment.window_size)?;
                cxn.incr_seq_num();
                TcpSegment::default().connection(&cxn)
            };

            r#await!(TcpRuntime::cast(&state, segment), rt.now())?;
            CoroutineOk(cxnid)
        })
    }

    pub fn close_connection(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        cxnid: TcpConnectionId,
        error: Option<Fail>,
        notify: bool,
    ) -> Future<'a, ()> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            let (rst_segment, cxn_handle, rt) = {
                let mut state = state.borrow_mut();
                let cxn = if let Some(cxn) = state.connections.remove(&cxnid) {
                    cxn
                } else {
                    return Err(Fail::ResourceNotFound {
                        details: "unrecognized connection ID",
                    });
                };

                let cxn = cxn.borrow();
                let rst_segment = TcpSegment::default().connection(&cxn).rst();
                let local_port = cxnid.local.port();
                if local_port.is_private() {
                    state.release_private_port(local_port)
                }

                (rst_segment, cxn.get_handle(), state.rt.clone())
            };

            if let Some(e) = error {
                if notify {
                    rt.emit_event(Event::TcpConnectionClosed {
                        handle: cxn_handle,
                        error: Some(e),
                    });
                }

                let _ =
                    r#await!(TcpRuntime::cast(&state, rst_segment), rt.now());
            } else if notify {
                rt.emit_event(Event::TcpConnectionClosed {
                    handle: cxn_handle,
                    error: None,
                });
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
            let (cxn, rt) = {
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

                let cxn = state.new_connection(cxnid.clone(), rt.clone())?;
                {
                    let mut cxn = cxn.borrow_mut();
                    cxn.negotiate_mss(syn_segment.mss)?;
                    cxn.set_remote_isn(syn_segment.seq_num);
                }

                (cxn, rt)
            };

            let options = rt.options();
            let retries = options.tcp.handshake_retries();
            let timeout = options.tcp.handshake_timeout();
            let ack_segment = r#await!(
                TcpRuntime::handshake(&state, cxn.clone()),
                rt.now(),
                Retry::binary_exponential(timeout, retries)
            )?;

            {
                // SYN+ACK packet has been acknowledged; increment the sequence
                // number and notify the caller.
                let mut cxn = cxn.borrow_mut();
                cxn.set_remote_receive_window_size(ack_segment.window_size)?;
                cxn.incr_seq_num();
                rt.emit_event(Event::TcpConnectionEstablished(
                    cxn.get_handle(),
                ));
            }

            r#await!(
                TcpRuntime::on_connection_established(&state, cxn),
                rt.now()
            )?;

            CoroutineOk(())
        })
    }

    pub fn on_connection_established(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        cxn: Rc<RefCell<TcpConnection<'a>>>,
    ) -> Future<'a, ()> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            trace!("TcpRuntime::on_connection_established(...)::coroutine",);

            let (cxnid, rt) = {
                let state = state.borrow();
                let rt = state.rt().clone();
                let cxn = cxn.borrow();
                (cxn.get_cxnid().clone(), rt)
            };

            let error = match r#await!(
                TcpRuntime::main_connection_loop(&state, cxn.clone()),
                rt.now()
            ) {
                Ok(()) => None,
                Err(e) => Some(e),
            };

            r#await!(
                TcpRuntime::close_connection(&state, cxnid, error, true),
                rt.now()
            )?;

            CoroutineOk(())
        })
    }

    #[allow(clippy::cognitive_complexity)]
    pub fn main_connection_loop(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        cxn: Rc<RefCell<TcpConnection<'a>>>,
    ) -> Future<'a, ()> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            trace!("TcpRuntime::main_connection_loop(...)::coroutine",);

            let rt = state.borrow().rt().clone();
            let options = rt.options();
            let mut ack_owed_since = None;
            loop {
                let mut transmittable_segments: VecDeque<TcpSegment> = {
                    let mut cxn = cxn.borrow_mut();
                    let mut transmittable_segments = VecDeque::new();
                    while let Some(segment) = cxn.input_queue_mut().pop_front()
                    {
                        // if there's a payload, we need to acknowledge it at
                        // some point. we set a timer if it hasn't already been
                        // set.
                        if ack_owed_since.is_none()
                            && segment.payload.len() > 0
                        {
                            ack_owed_since = Some(rt.now());
                            debug!(
                                "{}: ack_owed_since = {:?}",
                                options.my_ipv4_addr, ack_owed_since
                            );
                        }

                        match cxn.receive(segment) {
                            Ok(segments) => {
                                transmittable_segments.extend(segments)
                            }
                            Err(e) => warn!(
                                "{}: packet dropped ({:?})",
                                options.my_ipv4_addr, e
                            ),
                        }
                    }

                    while let Some(segment) =
                        cxn.try_get_next_transmittable_segment()
                    {
                        transmittable_segments.push_back(segment);
                    }

                    transmittable_segments
                };

                let mut retransmissions =
                    cxn.borrow_mut().get_retransmissions()?;
                debug!(
                    "{}: {} segments will be retransmitted",
                    options.my_ipv4_addr,
                    retransmissions.len()
                );
                while let Some(segment) = retransmissions.pop_front() {
                    r#await!(TcpRuntime::cast(&state, segment), rt.now())?;
                }

                debug!(
                    "{}: {} segments can be transmitted",
                    options.my_ipv4_addr,
                    transmittable_segments.len()
                );

                while let Some(segment) = transmittable_segments.pop_front() {
                    assert!(segment.ack);
                    ack_owed_since = None;
                    r#await!(TcpRuntime::cast(&state, segment), rt.now())?;
                }

                if let Some(timestamp) = ack_owed_since {
                    debug!(
                        "{}: ack_owed_since = {:?} ({:?})",
                        options.my_ipv4_addr,
                        ack_owed_since,
                        rt.now() - timestamp
                    );
                    debug!(
                        "{}: options.tcp.trailing_ack_delay() = {:?}",
                        options.my_ipv4_addr,
                        options.tcp.trailing_ack_delay()
                    );
                    if rt.now() - timestamp > options.tcp.trailing_ack_delay()
                    {
                        debug!(
                            "{}: delayed ACK timer has expired; sending pure \
                             ACK...",
                            options.my_ipv4_addr,
                        );
                        let pure_ack =
                            TcpSegment::default().connection(&cxn.borrow());

                        r#await!(
                            TcpRuntime::cast(&state, pure_ack),
                            rt.now()
                        )?;

                        ack_owed_since = None;
                    }
                } else {
                    debug!("{}: ack_owed_since = None", options.my_ipv4_addr)
                }

                yield None;
            }
        })
    }

    fn handshake(
        state: &Rc<RefCell<TcpRuntime<'a>>>,
        cxn: Rc<RefCell<TcpConnection<'a>>>,
    ) -> Future<'a, Rc<TcpSegment>> {
        let rt = state.borrow().rt.clone();
        let state = state.clone();
        rt.start_coroutine(move || {
            trace!("TcpRuntime::handshake()");
            let (segment, rt, ack_sent) = {
                let state = state.borrow();
                let cxn = cxn.borrow();
                let segment = TcpSegment::default()
                    .connection(&cxn)
                    .mss(cxn.get_mss())
                    .syn();
                let ack_sent = segment.ack;
                (segment, state.rt.clone(), ack_sent)
            };

            let expected_ack_num = segment.seq_num + Wrapping(1);

            r#await!(TcpRuntime::cast(&state, segment), rt.now())?;

            loop {
                if yield_until!(
                    !cxn.borrow().input_queue().is_empty(),
                    state.rt.now()
                ) {
                    let segment = cxn
                        .borrow_mut()
                        .input_queue_mut()
                        .pop_front()
                        .unwrap();
                    debug!("popped a segment for handshake: {:?}", segment);
                    if segment.rst {
                        return Err(Fail::ConnectionRefused {});
                    }

                    if segment.ack
                        && ack_sent != segment.syn
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

    pub fn get_connection_given_handle(
        &self,
        handle: TcpConnectionHandle,
    ) -> Result<&Rc<RefCell<TcpConnection<'a>>>> {
        if let Some(cxnid) = self.assigned_handles.get(&handle) {
            Ok(self.connections.get(&cxnid).unwrap())
        } else {
            Err(Fail::ResourceNotFound {
                details: "unrecognized connection handle",
            })
        }
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

    #[allow(dead_code)]
    fn release_connection_handle(&mut self, handle: TcpConnectionHandle) {
        self.unassigned_connection_handles.push_back(handle);
    }

    fn new_connection(
        &mut self,
        cxnid: TcpConnectionId,
        rt: Runtime<'a>,
    ) -> Result<Rc<RefCell<TcpConnection<'a>>>> {
        let options = self.rt.options();
        let handle = self.acquire_connection_handle()?;
        let local_isn = self.isn_generator.next(&cxnid);
        let cxn = TcpConnection::new(
            cxnid.clone(),
            handle,
            local_isn,
            options.tcp.receive_window_size(),
            rt.clone(),
        );
        let local_port = cxnid.local.port();
        let cxn = Rc::new(RefCell::new(cxn));
        assert!(self
            .connections
            .insert(cxnid.clone(), cxn.clone())
            .is_none());
        assert!(self.assigned_handles.insert(handle, cxnid).is_none());
        self.open_ports.insert(local_port);
        Ok(cxn)
    }
}
