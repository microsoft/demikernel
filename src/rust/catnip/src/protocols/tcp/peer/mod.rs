// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod isn_generator;

// #[cfg(test)]
// mod tests;

use super::{
    connection::{TcpConnection, TcpConnectionHandle, TcpConnectionId},
    segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder},
};
use crate::{
    protocols::{arp, ip, ipv4},
};
use crate::fail::Fail;
use fxhash::{FxHashMap, FxHashSet};
use isn_generator::IsnGenerator;
use rand::seq::SliceRandom;
use std::{
    cell::RefCell,
    collections::VecDeque,
    convert::TryFrom,
    num::Wrapping,
    rc::Rc,
    time::Duration,
};
use std::{pin::Pin, future::Future, task::{Poll, Context}};
use futures::FutureExt;
use futures::{Stream, stream::FuturesUnordered};

struct TcpPeerState {
    arp: arp::Peer<RT>,
    assigned_handles: FxHashMap<TcpConnectionHandle, Rc<TcpConnectionId>>,
    connections:
        FxHashMap<Rc<TcpConnectionId>, Rc<RefCell<TcpConnection>>>,
    isn_generator: IsnGenerator,
    open_ports: FxHashSet<ip::Port>,
    rt: Runtime,
    unassigned_connection_handles: VecDeque<TcpConnectionHandle>,
    unassigned_private_ports: VecDeque<ip::Port>, // todo: shared state.

    // TODO: Yuck, do this properly.
    background_work: Rc<RefCell<FuturesUnordered<Pin<Box<dyn Future<Output = ()>>>>>>,
}

impl TcpPeerState {
    fn new(rt: Runtime, arp: arp::Peer<RT>) -> Self {
        // initialize the pool of available private ports.
        let unassigned_private_ports = {
            let mut ports = Vec::new();
            for i in ip::Port::first_private_port().into()..65535 {
                ports.push(ip::Port::try_from(i).unwrap());
            }
            rt.with_rng(|rng| ports.shuffle(rng));
            VecDeque::from(ports)
        };

        // initialize the pool of available connection handles.
        let unassigned_connection_handles = {
            let mut handles = Vec::new();
            for i in 1..u16::max_value() {
                handles.push(TcpConnectionHandle::try_from(i).unwrap());
            }
            rt.with_rng(|rng| handles.shuffle(rng));
            VecDeque::from(handles)
        };

        let isn_generator = IsnGenerator::new(&rt);

        TcpPeerState {
            arp,
            assigned_handles: FxHashMap::default(),
            connections: FxHashMap::default(),
            isn_generator,
            open_ports: FxHashSet::default(),
            rt,
            unassigned_connection_handles,
            unassigned_private_ports,
            background_work: Rc::new(RefCell::new(FuturesUnordered::new())),
        }
    }

    fn add_background_work(&self, name: &'static str, f: impl Future<Output=Result<(), Fail>> + 'static) {
        let future = async move {
            if let Err(e) = f.await {
                warn!("{} failed: {:?}", name, e);
            }
        };
        self.background_work.borrow_mut().push(future.boxed_local());
    }

    fn get_connection_given_handle(
        &self,
        handle: TcpConnectionHandle,
    ) -> Result<&Rc<RefCell<TcpConnection>>, Fail> {
        if let Some(cxnid) = self.assigned_handles.get(&handle) {
            Ok(self.connections.get(cxnid).unwrap())
        } else {
            Err(Fail::ResourceNotFound {
                details: "unrecognized connection handle",
            })
        }
    }

    fn acquire_private_port(&mut self) -> Result<ip::Port, Fail> {
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

    fn acquire_connection_handle(&mut self) -> Result<TcpConnectionHandle, Fail> {
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
        cxnid: Rc<TcpConnectionId>,
        rt: Runtime,
    ) -> Result<Rc<RefCell<TcpConnection>>, Fail> {
        let options = self.rt.options();
        let handle = self.acquire_connection_handle()?;
        let local_isn = self.isn_generator.next(&*cxnid);
        let cxn = TcpConnection::new(
            cxnid.clone(),
            handle,
            local_isn,
            options.tcp.receive_window_size,
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

    async fn cast(state: Rc<RefCell<TcpPeerState>>, bytes: Rc<RefCell<Vec<u8>>>) -> Result<(), Fail> {
        let (arp, rt, remote_ipv4_addr) = {
            let state = state.borrow();
            let rt = state.rt.clone();
            let mut bytes = bytes.borrow_mut();
            let mut encoder = TcpSegmentEncoder::attach(bytes.as_mut());
            encoder.ipv4().header().src_addr(rt.options().my_ipv4_addr);
            let decoder = encoder.unmut();
            (state.arp.clone(), rt, decoder.ipv4().header().dest_addr())
        };
        let remote_link_addr = arp.query(remote_ipv4_addr).await?;
        {
            let options = rt.options();
            let mut bytes = bytes.borrow_mut();
            let mut encoder = TcpSegmentEncoder::attach(bytes.as_mut());
            encoder.ipv4().header().src_addr(options.my_ipv4_addr);
            let mut frame_header = encoder.ipv4().frame().header();
            frame_header.src_addr(options.my_link_addr);
            frame_header.dest_addr(remote_link_addr);
            let _ = encoder.seal()?;
        }
        rt.emit_event(Event::Transmit(bytes));
        Ok(())
    }

    async fn new_active_connection(state: Rc<RefCell<TcpPeerState>>, cxnid: Rc<TcpConnectionId>) -> Result<(), Fail> {
        trace!("TcpRuntime::new_active_connection(.., {:?})", cxnid);
        let (cxn, rt) = {
            let mut state = state.borrow_mut();
            let rt = state.rt.clone();
            (state.new_connection(cxnid.clone(), rt.clone())?, rt)
        };

        let options = rt.options();

        let handshake_future = rt.exponential_retry(
            options.tcp.handshake_timeout,
            options.tcp.handshake_retries,
            || TcpPeerState::handshake(state.clone(), cxn.clone()),
        );
        let ack_segment = handshake_future.await??;

        let remote_isn = ack_segment.seq_num;
        let (bytes, handle) = {
            let mut cxn = cxn.borrow_mut();
            cxn.set_remote_isn(remote_isn);
            cxn.negotiate_mss(ack_segment.mss)?;
            cxn.set_remote_receive_window_size(ack_segment.window_size)?;
            cxn.incr_seq_num();
            let segment = TcpSegment::default().connection(&cxn);
            (Rc::new(RefCell::new(segment.encode())), cxn.get_handle())
        };
        TcpPeerState::cast(state.clone(), bytes).await?;
        info!(
            "{}: connection established (handle = {})",
            options.my_ipv4_addr, handle
        );
        Ok(())
    }

    async fn new_passive_connection(state: Rc<RefCell<TcpPeerState>>, syn_segment: TcpSegment) -> Result<(), Fail> {
        let (cxn, rt) = {
            let mut state = state.borrow_mut();
            let rt = state.rt.clone();
            let options = rt.options();

            assert!(syn_segment.syn && !syn_segment.ack);
            let local_port = syn_segment.dest_port.unwrap();
            assert!(state.open_ports.contains(&local_port));

            let remote_ipv4_addr = syn_segment.src_ipv4_addr.unwrap();
            let remote_port = syn_segment.src_port.unwrap();
            let cxnid = Rc::new(TcpConnectionId {
                local: ipv4::Endpoint::new(
                    options.my_ipv4_addr,
                    local_port,
                ),
                remote: ipv4::Endpoint::new(remote_ipv4_addr, remote_port),
            });

            let cxn = state.new_connection(cxnid.clone(), rt.clone())?;
            {
                let mut cxn = cxn.borrow_mut();
                cxn.negotiate_mss(syn_segment.mss)?;
                cxn.set_remote_isn(syn_segment.seq_num);
            }

            (cxn, rt)
        };

        let options = rt.options();
        let handshake_future = rt.exponential_retry(
            options.tcp.handshake_timeout,
            options.tcp.handshake_retries,
            || TcpPeerState::handshake(state.clone(), cxn.clone()),
        );
        let ack_segment = handshake_future.await??;
        {
            // SYN+ACK packet has been acknowledged; increment the sequence
            // number and notify the caller.
            let mut cxn = cxn.borrow_mut();
            cxn.set_remote_receive_window_size(ack_segment.window_size)?;
            cxn.incr_seq_num();
            rt.emit_event(Event::IncomingTcpConnection(cxn.get_handle()));
        }
        TcpPeerState::on_connection_established(state, cxn).await?;
        Ok(())
    }

    async fn handshake(state: Rc<RefCell<TcpPeerState>>, cxn: Rc<RefCell<TcpConnection>>) -> Result<Rc<TcpSegment>, Fail> {
        trace!("TcpRuntime::handshake()");
        let (bytes, ack_was_sent, expected_ack_num) = {
            let cxn = cxn.borrow();
            let segment = TcpSegment::default()
                .connection(&cxn)
                .mss(cxn.get_mss())
                .syn();
            let ack_was_sent = segment.ack;
            let expected_ack_num = segment.seq_num + Wrapping(1);
            let bytes = Rc::new(RefCell::new(segment.encode()));
            (bytes, ack_was_sent, expected_ack_num)
        };
        TcpPeerState::cast(state.clone(), bytes).await?;
        loop {
            let segment = loop {

                let r = { cxn.borrow_mut().receive_queue_mut().pop_front() };
                match r {
                    Some(s) => break s,
                    None => {
                        let f = { state.borrow().rt.wait(Duration::from_micros(1)) };
                        // TODO: More precisely signal when this loop should resume.
                        f.await;
                    },
                }
            };
            if segment.rst {
                return Err(Fail::ConnectionRefused {});
            }
            if segment.ack
                && ack_was_sent != segment.syn
                && segment.ack_num == expected_ack_num
            {
                return Ok(Rc::new(segment));
            }
        }
    }

    async fn close_connection(
        state: Rc<RefCell<TcpPeerState>>,
        cxnid: Rc<TcpConnectionId>,
        error: Option<Fail>,
        notify: bool,
    ) -> Result<(), Fail>
    {
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
        let had_error = error.is_some();
        if notify {
            rt.emit_event(Event::TcpConnectionClosed { handle: cxn_handle, error });
        }
        if had_error {
            let bytes = Rc::new(RefCell::new(rst_segment.encode()));
            let _ = TcpPeerState::cast(state, bytes).await;
        }
        Ok(())
    }

    pub async fn on_connection_established(state: Rc<RefCell<TcpPeerState>>, cxn: Rc<RefCell<TcpConnection>>) -> Result<(), Fail> {
        trace!("TcpRuntime::on_connection_established(...)::coroutine",);
        let cxnid = cxn.borrow().get_id().clone();
        let error = TcpPeerState::main_connection_loop(state.clone(), cxn.clone()).await.err();
        TcpPeerState::close_connection(state, cxnid, error, true).await?;
        Ok(())
    }

    pub async fn main_connection_loop(
        state: Rc<RefCell<TcpPeerState>>,
        cxn: Rc<RefCell<TcpConnection>>,
    ) -> Result<(), Fail> {
        trace!("TcpRuntime::main_connection_loop(...)::coroutine",);

        let rt = state.borrow().rt.clone();
        let options = rt.options();
        let mut ack_owed_since = None;
        loop {
            {
                let mut cxn = cxn.borrow_mut();
                while let Some(segment) = cxn.receive_queue_mut().pop_front() {
                    if segment.rst {
                        return Err(Fail::ConnectionAborted {});
                    }

                    // if there's a payload, we need to acknowledge it at
                    // some point. we set a timer if it hasn't already been
                    // set.
                    if ack_owed_since.is_none()
                        && !segment.payload.is_empty()
                    {
                        ack_owed_since = Some(rt.now());
                        /*debug!(
                        "{}: ack_owed_since = {:?}",
                        options.my_ipv4_addr, ack_owed_since
                    );*/
                    }

                    match cxn.receive(segment) {
                        Ok(()) => (),
                        Err(Fail::Ignored { details }) => {
                            warn!("TCP segment ignored: {}", details)
                        }
                        e => e?,
                    }
                }

                cxn.enqueue_retransmissions()?;
            }
            while let Some(s) = cxn.borrow_mut().try_get_next_transmittable_segment() {
                ack_owed_since = None;
                TcpPeerState::cast(state.clone(), s).await?;
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
                    options.my_ipv4_addr, options.tcp.trailing_ack_delay
                );
                if rt.now() - timestamp > options.tcp.trailing_ack_delay {
                    debug!(
                        "{}: delayed ACK timer has expired; sending pure \
                         ACK...",
                        options.my_ipv4_addr,
                    );
                    let pure_ack = TcpSegment::default().connection(&cxn.borrow());
                    let bytes = Rc::new(RefCell::new(pure_ack.encode()));
                    TcpPeerState::cast(state.clone(), bytes).await?;
                    ack_owed_since = None;
                }
            }
            // TODO: More precisely signal when this loop should resume.
            rt.wait(Duration::from_micros(1)).await;
        }
    }
}

pub struct TcpPeer {
    state: Rc<RefCell<TcpPeerState>>,
}

impl TcpPeer {
    pub fn new(rt: Runtime, arp: arp::Peer<RT>) -> Self {
        TcpPeer {
            state: Rc::new(RefCell::new(TcpPeerState::new(rt, arp))),
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<(), Fail> {
        trace!("TcpPeer::receive(...)");
        let decoder = TcpSegmentDecoder::try_from(datagram)?;
        let segment = TcpSegment::try_from(decoder)?;
        info!(
            "{} received segment: {:?}",
            self.state.borrow().rt.options().my_ipv4_addr,
            segment
        );

        let local_ipv4_addr = segment.dest_ipv4_addr.unwrap();
        // i haven't yet seen anything that explicitly disallows categories of
        // IP addresses but it seems sensible to drop datagrams where the
        // source address does not really support a connection.
        let remote_ipv4_addr =
            segment.src_ipv4_addr.ok_or(Fail::Malformed {
                details: "source IPv4 address is missing",
            })?;
        if remote_ipv4_addr.is_broadcast()
            || remote_ipv4_addr.is_multicast()
            || remote_ipv4_addr.is_unspecified()
        {
            return Err(Fail::Malformed {
                details: "only unicast addresses are supported by TCP",
            });
        }

        let local_port = segment.dest_port.ok_or(Fail::Malformed {
            details: "destination port is zero",
        })?;

        let remote_port = segment.src_port.ok_or(Fail::Malformed {
            details: "source port is zero",
        })?;


        {
            let s = self.state.borrow_mut();
            if s.open_ports.contains(&local_port) {
                if segment.syn && !segment.ack && !segment.rst {
                    let future = TcpPeerState::new_passive_connection(self.state.clone(), segment);
                    s.add_background_work("new_passive_connection", future);
                    return Ok(());
                }

                let cxnid = TcpConnectionId {
                    local: ipv4::Endpoint::new(local_ipv4_addr, local_port),
                    remote: ipv4::Endpoint::new(remote_ipv4_addr, remote_port),
                };


                if let Some(cxn) = s.connections.get(&cxnid)
                {
                    cxn.borrow_mut().receive_queue_mut().push_back(segment);
                    return Ok(());
                } else {
                    return Err(Fail::ResourceNotFound {
                        details: "unrecognized connection ID",
                    });
                }
            }
        }

        // `local_port` is not open; send the appropriate RST segment.
        debug!("local port {} is not open; sending RST.", local_port);
        let mut ack_num =
            segment.seq_num + Wrapping(u32::try_from(segment.payload.len())?);
        // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/TCP_IP+Illustrated,+Volume+1:+The+Protocols/9780132808200/ch13.html#ch13):
        // > Although there is no data in the arriving segment, the SYN
        // > bit logically occupies 1 byte of sequence number space;
        // > therefore, in this example the ACK number in the reset
        // > segment is set to the ISN, plus the data length (0), plus 1
        // > for the SYN bit.
        if segment.syn {
            ack_num += Wrapping(1);
        }

        let segment = TcpSegment::default()
            .dest_ipv4_addr(remote_ipv4_addr)
            .dest_port(remote_port)
            .src_port(local_port)
            .ack(ack_num)
            .rst();
        let bytes = Rc::new(RefCell::new(segment.encode()));

        let future = TcpPeerState::cast(self.state.clone(), bytes);
        self.state.borrow_mut().add_background_work("cast", future);

        Ok(())
    }

    pub fn connect(&self, remote_endpoint: ipv4::Endpoint) -> impl Future<Output=Result<TcpConnectionHandle, Fail>> {
        trace!("TcpPeer::connect({:?})", remote_endpoint);
        let state = self.state.clone();
        async move {
            trace!("TcpPeer::connect({:?})::coroutine", remote_endpoint);
            let cxnid = {
                let mut state = state.borrow_mut();
                let rt = state.rt.clone();
                let options = rt.options();
                let cxnid = Rc::new(TcpConnectionId {
                    local: ipv4::Endpoint::new(
                        options.my_ipv4_addr,
                        state.acquire_private_port()?,
                    ),
                    remote: remote_endpoint,
                });
                cxnid
            };
            match TcpPeerState::new_active_connection(state.clone(), cxnid.clone()).await {
                Ok(()) => {
                    let state_ = state.borrow_mut();
                    let cxn = state_.connections.get(&cxnid).unwrap().clone();
                    let handle = cxn.borrow().get_handle();
                    let future = TcpPeerState::on_connection_established(state.clone(), cxn);
                    state_.add_background_work("on_connection_established", future);
                    Ok(handle)
                },
                Err(e) => {
                    let _ = TcpPeerState::close_connection(state.clone(), cxnid, Some(e.clone()), false).await;
                    Err(e)
                },
            }
        }
    }


    pub fn listen(&mut self, port: ip::Port) -> Result<(), Fail> {
        let mut state = self.state.borrow_mut();
        if state.open_ports.contains(&port) {
            return Err(Fail::ResourceBusy {
                details: "port already in use",
            });
        }

        assert!(state.open_ports.insert(port));
        Ok(())
    }

    pub fn write(
        &self,
        handle: TcpConnectionHandle,
        bytes: Vec<u8>,
    ) -> Result<(), Fail> {
        let state = self.state.borrow();
        let mut cxn = state.get_connection_given_handle(handle)?.borrow_mut();
        cxn.write(bytes);
        Ok(())
    }

    pub fn peek(&self, handle: TcpConnectionHandle) -> Result<Rc<Vec<u8>>, Fail> {
        let state = self.state.borrow();
        let cxn = state.get_connection_given_handle(handle)?.borrow();
        if let Some(bytes) = cxn.peek() {
            Ok(bytes.clone())
        } else {
            Err(Fail::ResourceExhausted {
                details: "The unread queue is empty.",
            })
        }
    }

    pub fn read(
        &mut self,
        handle: TcpConnectionHandle,
    ) -> Result<Rc<Vec<u8>>, Fail> {
        let state = self.state.borrow();
        let mut cxn = state.get_connection_given_handle(handle)?.borrow_mut();
        if let Some(bytes) = cxn.read() {
            Ok(bytes)
        } else {
            Err(Fail::ResourceExhausted {
                details: "The unread queue is empty.",
            })
        }
    }

    pub fn get_mss(&self, handle: TcpConnectionHandle) -> Result<usize, Fail> {
        let state = self.state.borrow();
        let cxn = state.get_connection_given_handle(handle)?.borrow();
        Ok(cxn.get_mss())
    }

    pub fn get_rto(&self, handle: TcpConnectionHandle) -> Result<Duration, Fail> {
        let state = self.state.borrow();
        let cxn = state.get_connection_given_handle(handle)?.borrow();
        Ok(cxn.get_rto())
    }

    pub fn get_connection_id(
        &self,
        handle: TcpConnectionHandle,
    ) -> Result<Rc<TcpConnectionId>, Fail> {
        let state = self.state.borrow();
        let cxn = state.get_connection_given_handle(handle)?.borrow();
        Ok(cxn.get_id().clone())
    }
}

impl Future for TcpPeer {
    type Output = !;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        let background_work = { self.get_mut().state.borrow().background_work.clone() };
        let mut background_work = background_work.borrow_mut();
        loop {
            match Stream::poll_next(Pin::new(&mut *background_work), ctx) {
                Poll::Ready(Some(..)) => continue,
                Poll::Ready(None) | Poll::Pending => return Poll::Pending,
            }
        }
    }
}
