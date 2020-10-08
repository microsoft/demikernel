use super::{
    constants::FALLBACK_MSS,
    established::state::{
        receiver::Receiver,
        sender::Sender,
        ControlBlock,
    },
    isn_generator::IsnGenerator,
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ipv4,
        tcp::{
            segment::TcpSegment,
            SeqNumber,
        },
    },
    runtime::Runtime,
    scheduler::SchedulerHandle,
};
use hashbrown::{HashMap, HashSet};
use std::{
    cell::RefCell,
    collections::{
        VecDeque,
    },
    convert::TryInto,
    future::Future,
    num::Wrapping,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::Duration,
};

struct InflightAccept {
    local_isn: SeqNumber,
    remote_isn: SeqNumber,
    window_size: u32,
    window_scale: u8,
    mss: usize,

    #[allow(unused)]
    handle: SchedulerHandle,
}

struct ReadySockets<RT: Runtime> {
    ready: VecDeque<Result<ControlBlock<RT>, Fail>>,
    endpoints: HashSet<ipv4::Endpoint>,
    waker: Option<Waker>,
}

impl<RT: Runtime> ReadySockets<RT> {
    fn push_ok(&mut self, cb: ControlBlock<RT>) {
        assert!(self.endpoints.insert(cb.remote));
        self.ready.push_back(Ok(cb));
        self.waker.take().map(|w| w.wake());
    }

    fn push_err(&mut self, err: Fail) {
        self.ready.push_back(Err(err));
        self.waker.take().map(|w| w.wake());
    }

    fn pop(&mut self) -> Option<Result<ControlBlock<RT>, Fail>> {
        let r = self.ready.pop_front()?;
        if let Ok(ref cb) = r {
            assert!(self.endpoints.remove(&cb.remote));
        }
        Some(r)
    }

    fn poll(&mut self, ctx: &mut Context) -> Poll<Result<ControlBlock<RT>, Fail>> {
        let r = match self.ready.pop_front() {
            Some(r) => r,
            None => {
                self.waker.replace(ctx.waker().clone());
                return Poll::Pending;
            },
        };
        if let Ok(ref cb) = r {
            assert!(self.endpoints.remove(&cb.remote));
        }
        Poll::Ready(r)
    }

    fn len(&self) -> usize {
        self.ready.len()
    }
}

pub struct PassiveSocket<RT: Runtime> {
    inflight: HashMap<ipv4::Endpoint, InflightAccept>,
    ready: Rc<RefCell<ReadySockets<RT>>>,

    max_backlog: usize,
    isn_generator: IsnGenerator,

    local: ipv4::Endpoint,
    rt: RT,
    arp: arp::Peer<RT>,
}

impl<RT: Runtime> PassiveSocket<RT> {
    pub fn new(local: ipv4::Endpoint, max_backlog: usize, rt: RT, arp: arp::Peer<RT>) -> Self {
        let ready = ReadySockets {
            ready: VecDeque::new(),
            endpoints: HashSet::new(),
            waker: None,
        };
        let ready = Rc::new(RefCell::new(ready));
        let nonce = rt.rng_gen();
        Self {
            inflight: HashMap::new(),
            ready,
            max_backlog,
            isn_generator: IsnGenerator::new(nonce),
            local,
            rt,
            arp,
        }
    }

    pub fn accept(&mut self) -> Result<Option<ControlBlock<RT>>, Fail> {
        match self.ready.borrow_mut().pop() {
            None => Ok(None),
            Some(Ok(cb)) => Ok(Some(cb)),
            Some(Err(e)) => Err(e),
        }
    }

    pub fn poll_accept(&mut self, ctx: &mut Context) -> Poll<Result<ControlBlock<RT>, Fail>> {
        self.ready.borrow_mut().poll(ctx)
    }

    pub fn receive_segment(&mut self, segment: TcpSegment) -> Result<(), Fail> {
        let local_port = segment.dest_port.ok_or_else(|| Fail::Malformed {
            details: "Missing destination port",
        })?;
        let local_ipv4_addr = segment.dest_ipv4_addr.ok_or_else(|| Fail::Malformed {
            details: "Missing destination IPv4 addr",
        })?;
        let local = ipv4::Endpoint::new(local_ipv4_addr, local_port);
        if local != self.local {
            return Err(Fail::Malformed {
                details: "Wrong destination address",
            });
        }
        let remote_ipv4_addr = segment.src_ipv4_addr.ok_or_else(|| Fail::Malformed {
            details: "Missing source IPv4 addr",
        })?;
        let remote_port = segment.src_port.ok_or_else(|| Fail::Malformed {
            details: "Missing source port",
        })?;
        let remote = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

        if self.ready.borrow().endpoints.contains(&remote) {
            // TODO: What should we do if a packet shows up for a connection that hasn't been
            // `accept`ed yet?
            return Ok(());
        }

        let inflight_len = self.inflight.len();

        // If the packet is for an inflight connection, route it there.
        if self.inflight.contains_key(&remote) {
            if !segment.ack {
                return Err(Fail::Malformed {
                    details: "Invalid flags",
                });
            }

            // TODO: Add entry API.
            let &InflightAccept {
                local_isn,
                remote_isn,
                window_size,
                window_scale,
                mss,
                ..
            } = self.inflight.get(&remote).unwrap();
            if segment.ack_num != local_isn + Wrapping(1) {
                return Err(Fail::Malformed {
                    details: "Invalid SYN+ACK seq num",
                });
            }
            let sender = Sender::new(local_isn + Wrapping(1), window_size, window_scale, mss);
            let receiver = Receiver::new(
                remote_isn + Wrapping(1),
                self.rt.tcp_options().receive_window_size as u32,
            );
            self.inflight.remove(&remote);
            let cb = ControlBlock {
                local: self.local.clone(),
                remote: remote.clone(),
                rt: self.rt.clone(),
                arp: self.arp.clone(),
                sender,
                receiver,
            };
            self.ready.borrow_mut().push_ok(cb);
        }
        // Otherwise, start a new connection.
        else {
            if !segment.syn || segment.ack || segment.rst {
                return Err(Fail::Malformed {
                    details: "Invalid flags",
                });
            }
            if inflight_len + self.ready.borrow().len() >= self.max_backlog {
                // TODO: Should we send a RST here?
                return Err(Fail::ConnectionRefused {});
            }
            let local_isn = self.isn_generator.generate(&local, &remote);
            let remote_isn = segment.seq_num;
            let future = Self::background(
                local_isn,
                remote_isn,
                local,
                remote.clone(),
                self.rt.clone(),
                self.arp.clone(),
                self.ready.clone(),
            );
            let handle = self.rt.spawn(future);
            let window_scale = segment.window_scale.unwrap_or(1);
            let window_size = segment
                .window_size
                .checked_shl(window_scale as u32)
                .expect("TODO: Window size overflow")
                .try_into()
                .expect("TODO: Window size overflow");
            let mss = match segment.mss {
                Some(s) => s,
                None => {
                    warn!("Falling back to MSS = {}", FALLBACK_MSS);
                    FALLBACK_MSS
                },
            };
            let accept = InflightAccept {
                local_isn,
                remote_isn,
                window_size,
                window_scale,
                mss,
                handle,
            };
            self.inflight.insert(remote, accept);
        }

        Ok(())
    }

    fn background(
        local_isn: SeqNumber,
        remote_isn: SeqNumber,
        local: ipv4::Endpoint,
        remote: ipv4::Endpoint,
        rt: RT,
        arp: arp::Peer<RT>,
        ready: Rc<RefCell<ReadySockets<RT>>>,
    ) -> impl Future<Output = ()> {
        let handshake_retries = 3usize;
        let handshake_timeout = Duration::from_secs(5);
        let max_window_size = 1024;

        async move {
            for _ in 0..handshake_retries {
                let remote_link_addr = match arp.query(remote.address()).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("ARP query failed: {:?}", e);
                        continue;
                    },
                };
                let segment_buf = TcpSegment::default()
                    .src_ipv4_addr(local.address())
                    .src_port(local.port())
                    .src_link_addr(rt.local_link_addr())
                    .dest_ipv4_addr(remote.address())
                    .dest_port(remote.port())
                    .dest_link_addr(remote_link_addr)
                    .seq_num(local_isn)
                    .window_size(max_window_size)
                    .mss(rt.tcp_options().advertised_mss)
                    .syn()
                    .ack(remote_isn + Wrapping(1))
                    .encode();

                rt.transmit(Rc::new(RefCell::new(segment_buf)));
                rt.wait(handshake_timeout).await;
            }
            ready.borrow_mut().push_err(Fail::Timeout {});
        }
    }
}
