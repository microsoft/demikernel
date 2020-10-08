use super::{
    active_open::ActiveOpenSocket,
    established::EstablishedSocket,
    isn_generator::IsnGenerator,
    passive_open::PassiveSocket,
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ip,
        ipv4,
        tcp::segment::{
            TcpSegment,
            TcpSegmentDecoder,
            TcpSegmentEncoder,
        },
        tcp::operations::{
            AcceptFuture,
            ConnectFuture,
            PopFuture,
            PushFuture,
            ConnectFutureState,
        },
    },
    runtime::Runtime,
};
use bytes::Bytes;
use hashbrown::HashMap;
use std::{
    cell::RefCell,
    collections::VecDeque,
    convert::TryFrom,
    rc::Rc,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

pub type SocketDescriptor = u16;

pub struct Peer<RT: Runtime> {
    pub(super) inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> Peer<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner::new(rt, arp))),
        }
    }

    pub fn socket(&self) -> SocketDescriptor {
        let mut inner = self.inner.borrow_mut();
        let fd = inner.alloc_fd();
        assert!(inner
            .sockets
            .insert(fd, Socket::Inactive { local: None })
            .is_none());
        fd
    }

    pub fn bind(&self, fd: SocketDescriptor, addr: ipv4::Endpoint) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        if addr.port() >= ip::Port::first_private_port() {
            return Err(Fail::Malformed {
                details: "Port number in private port range",
            });
        }
        match inner.sockets.get_mut(&fd) {
            Some(Socket::Inactive { ref mut local }) => {
                *local = Some(addr);
                Ok(())
            },
            _ => Err(Fail::Malformed {
                details: "Invalid file descriptor",
            }),
        }
    }

    pub fn receive_datagram(&self, datagram: ipv4::Datagram<'_>) {
        self.inner.borrow_mut().receive_datagram(datagram)
    }

    pub fn listen(&self, fd: SocketDescriptor, backlog: usize) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        let local = match inner.sockets.get_mut(&fd) {
            Some(Socket::Inactive { local: Some(local) }) => *local,
            _ => {
                return Err(Fail::Malformed {
                    details: "Invalid file descriptor",
                })
            },
        };
        // TODO: Should this move to bind?
        if inner.passive.contains_key(&local) {
            return Err(Fail::ResourceBusy {
                details: "Port already in use",
            });
        }

        let socket = PassiveSocket::new(local, backlog, inner.rt.clone(), inner.arp.clone());
        assert!(inner.passive.insert(local.clone(), socket).is_none());
        inner.sockets.insert(fd, Socket::Listening { local });
        Ok(())
    }

    pub fn accept(&self, fd: SocketDescriptor) -> Result<Option<SocketDescriptor>, Fail> {
        let mut inner_ = self.inner.borrow_mut();
        let inner = &mut *inner_;

        let local = match inner.sockets.get(&fd) {
            Some(Socket::Listening { local }) => local,
            Some(..) => {
                return Err(Fail::Malformed {
                    details: "Socket not listening",
                })
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        let passive = inner
            .passive
            .get_mut(local)
            .expect("sockets/local inconsistency");
        let cb = match passive.accept()? {
            Some(e) => e,
            None => return Ok(None),
        };
        let established = EstablishedSocket::new(cb);

        let fd = inner.alloc_fd();
        let key = (established.cb.local.clone(), established.cb.remote.clone());

        let socket = Socket::Established {
            local: established.cb.local.clone(),
            remote: established.cb.remote.clone(),
        };
        assert!(inner.sockets.insert(fd, socket).is_none());
        assert!(inner.established.insert(key, established).is_none());

        Ok(Some(fd))
    }

    pub fn poll_accept(&self, fd: SocketDescriptor, ctx: &mut Context) -> Poll<Result<SocketDescriptor, Fail>> {
        let mut inner_ = self.inner.borrow_mut();
        let inner = &mut *inner_;

        let local = match inner.sockets.get(&fd) {
            Some(Socket::Listening { local }) => local,
            Some(..) => {
                return Poll::Ready(Err(Fail::Malformed {
                    details: "Socket not listening",
                }))
            },
            None => return Poll::Ready(Err(Fail::Malformed { details: "Bad FD" })),
        };
        let passive = inner
            .passive
            .get_mut(local)
            .expect("sockets/local inconsistency");
        let cb = match passive.poll_accept(ctx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(e)) => e,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
        };
        let established = EstablishedSocket::new(cb);

        let fd = inner.alloc_fd();
        let key = (established.cb.local.clone(), established.cb.remote.clone());

        let socket = Socket::Established {
            local: established.cb.local.clone(),
            remote: established.cb.remote.clone(),
        };
        assert!(inner.sockets.insert(fd, socket).is_none());
        assert!(inner.established.insert(key, established).is_none());

        Poll::Ready(Ok(fd))
    }

    pub fn accept_async(&self, fd: SocketDescriptor) -> AcceptFuture<RT> {
        AcceptFuture {
            fd,
            inner: self.inner.clone(),
        }
    }

    pub fn connect(&self, fd: SocketDescriptor, remote: ipv4::Endpoint) -> ConnectFuture<RT> {
        let mut inner = self.inner.borrow_mut();

        let r = try {
            match inner.sockets.get_mut(&fd) {
                Some(Socket::Inactive { .. }) => (),
                _ => Err(Fail::Malformed {
                    details: "Invalid file descriptor",
                })?,
            }

            let local_port =
                inner
                    .unassigned_ports
                    .pop_front()
                    .ok_or_else(|| Fail::ResourceExhausted {
                        details: "Out of private ports",
                    })?;
            let local = ipv4::Endpoint::new(inner.rt.local_ipv4_addr(), local_port);

            let socket = Socket::Connecting {
                local: local.clone(),
                remote: remote.clone(),
            };
            inner.sockets.insert(fd, socket);

            let local_isn = inner.isn_generator.generate(&local, &remote);
            let key = (local.clone(), remote.clone());
            let socket = ActiveOpenSocket::new(
                local_isn,
                local,
                remote,
                inner.rt.clone(),
                inner.arp.clone(),
            );
            assert!(inner.connecting.insert(key, socket).is_none());
            fd
        };
        let state = match r {
            Ok(..) => ConnectFutureState::InProgress,
            Err(e) => ConnectFutureState::Failed(e),
        };
        ConnectFuture {
            fd,
            state,
            inner: self.inner.clone(),
        }
    }

    pub fn peek(&self, fd: SocketDescriptor) -> Result<Bytes, Fail> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.peek(),
            None => Err(Fail::Malformed {
                details: "Socket not established",
            }),
        }
    }

    pub fn recv(&self, fd: SocketDescriptor) -> Result<Option<Bytes>, Fail> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                return Err(Fail::Malformed {
                    details: "Recv: Socket not established",
                })
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.recv(),
            None => Err(Fail::Malformed {
                details: "Socket not established",
            }),
        }
    }

    pub fn poll_recv(&self, fd: SocketDescriptor, ctx: &mut Context) -> Poll<Result<Bytes, Fail>> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                return Poll::Ready(Err(Fail::Malformed {
                    details: "Recv: Socket not established",
                }))
            },
            None => return Poll::Ready(Err(Fail::Malformed { details: "Bad FD" })),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.poll_recv(ctx),
            None => Poll::Ready(Err(Fail::Malformed {
                details: "Socket not established",
            })),
        }
    }

    pub fn push_async(&self, fd: SocketDescriptor, buf: Bytes) -> PushFuture<RT> {
        let err = match self.send(fd, buf) {
            Ok(()) => None,
            Err(e) => Some(e),
        };
        PushFuture {
            fd,
            err,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn pop_async(&self, fd: SocketDescriptor) -> PopFuture<RT> {
        PopFuture {
            fd,
            inner: self.inner.clone(),
        }
    }

    pub fn send(&self, fd: SocketDescriptor, buf: Bytes) -> Result<(), Fail> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.send(buf),
            None => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
        }
    }

    pub fn close(&self, fd: SocketDescriptor) -> Result<(), Fail> {
        let inner = self.inner.borrow_mut();
        match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => {
                let key = (local.clone(), remote.clone());
                match inner.established.get(&key) {
                    Some(ref s) => s.close()?,
                    None => {
                        return Err(Fail::Malformed {
                            details: "Socket not established",
                        })
                    },
                }
            },
            Some(..) => {
                // TODO: Implement close for listening sockets.
                unimplemented!();
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        }
        Ok(())
    }

    pub fn remote_mss(&self, fd: SocketDescriptor) -> Result<usize, Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.remote_mss()),
            None => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
        }
    }

    pub fn current_rto(&self, fd: SocketDescriptor) -> Result<Duration, Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.current_rto()),
            None => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
        }
    }

    pub fn endpoints(
        &self,
        fd: SocketDescriptor,
    ) -> Result<(ipv4::Endpoint, ipv4::Endpoint), Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.endpoints()),
            None => {
                return Err(Fail::Malformed {
                    details: "Socket not established",
                })
            },
        }
    }
}

enum Socket {
    Inactive {
        local: Option<ipv4::Endpoint>,
    },
    Listening {
        local: ipv4::Endpoint,
    },
    Connecting {
        local: ipv4::Endpoint,
        remote: ipv4::Endpoint,
    },
    Established {
        local: ipv4::Endpoint,
        remote: ipv4::Endpoint,
    },
}

pub struct Inner<RT: Runtime> {
    isn_generator: IsnGenerator,

    next_fd: SocketDescriptor,
    unassigned_ports: VecDeque<ip::Port>,

    // FD -> local port
    sockets: HashMap<SocketDescriptor, Socket>,

    passive: HashMap<ipv4::Endpoint, PassiveSocket<RT>>,
    connecting: HashMap<(ipv4::Endpoint, ipv4::Endpoint), ActiveOpenSocket<RT>>,
    established: HashMap<(ipv4::Endpoint, ipv4::Endpoint), EstablishedSocket<RT>>,

    rt: RT,
    arp: arp::Peer<RT>,
}

impl<RT: Runtime> Inner<RT> {
    fn new(rt: RT, arp: arp::Peer<RT>) -> Self {
        Self {
            isn_generator: IsnGenerator::new(rt.rng_gen()),
            // TODO: Reuse old FDs.
            next_fd: 1,
            unassigned_ports: (ip::Port::first_private_port().into()..65535)
                .map(|p| ip::Port::try_from(p).unwrap())
                .collect(),
            sockets: HashMap::new(),
            passive: HashMap::new(),
            connecting: HashMap::new(),
            established: HashMap::new(),
            rt,
            arp,
        }
    }

    fn receive_datagram(&mut self, datagram: ipv4::Datagram<'_>) {
        let r: Result<_, Fail> = try {
            let decoder = TcpSegmentDecoder::try_from(datagram)?;
            let segment = TcpSegment::try_from(decoder)?;

            let local_ipv4_addr = segment.dest_ipv4_addr.ok_or_else(|| Fail::Malformed {
                details: "Missing destination IPv4 addr",
            })?;
            let local_port = segment.dest_port.ok_or_else(|| Fail::Malformed {
                details: "Missing destination port",
            })?;
            let local = ipv4::Endpoint::new(local_ipv4_addr, local_port);

            let remote_ipv4_addr = segment.src_ipv4_addr.ok_or_else(|| Fail::Malformed {
                details: "Missing source IPv4 addr",
            })?;
            let remote_port = segment.src_port.ok_or_else(|| Fail::Malformed {
                details: "Missing source port",
            })?;
            let remote = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

            if remote_ipv4_addr.is_broadcast()
                || remote_ipv4_addr.is_multicast()
                || remote_ipv4_addr.is_unspecified()
            {
                Err(Fail::Malformed {
                    details: "only unicast addresses are supported by TCP",
                })?;
            }

            let key = (local, remote);
            if let Some(s) = self.established.get(&key) {
                return s.receive_segment(segment);
            }
            if let Some(s) = self.connecting.get_mut(&key) {
                return s.receive_segment(segment);
            }
            let (local, _) = key;
            if let Some(s) = self.passive.get_mut(&local) {
                return s.receive_segment(segment)?;
            }

            // The packet isn't for an open port; send a RST segment.
            self.send_rst(segment)?;
        };
        if let Err(e) = r {
            // TODO: Actually send a RST segment here.
            warn!("Dropping invalid packet: {:?}", e);
        }
    }

    fn alloc_fd(&mut self) -> SocketDescriptor {
        let fd = self.next_fd;
        self.next_fd += 1;
        fd
    }

    fn send_rst(&mut self, segment: TcpSegment) -> Result<(), Fail> {
        let local_ipv4_addr = segment.dest_ipv4_addr.ok_or_else(|| Fail::Malformed {
            details: "Missing destination IPv4 addr",
        })?;
        let local_port = segment.dest_port.ok_or_else(|| Fail::Malformed {
            details: "Missing destination port",
        })?;
        let remote_ipv4_addr = segment.src_ipv4_addr.ok_or_else(|| Fail::Malformed {
            details: "Missing source IPv4 addr",
        })?;
        let remote_port = segment.src_port.ok_or_else(|| Fail::Malformed {
            details: "Missing source port",
        })?;

        let segment = TcpSegment::default()
            .src_ipv4_addr(local_ipv4_addr)
            .src_port(local_port)
            .dest_ipv4_addr(remote_ipv4_addr)
            .dest_port(remote_port)
            .rst();

        // TODO: Make this work pending on ARP resolution if needed.
        let remote_link_addr =
            self.arp
                .try_query(remote_ipv4_addr)
                .ok_or_else(|| Fail::ResourceNotFound {
                    details: "RST destination not in ARP cache",
                })?;

        let mut segment_buf = segment.encode();
        let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
        encoder.ipv4().header().src_addr(self.rt.local_ipv4_addr());

        let mut frame_header = encoder.ipv4().frame().header();
        frame_header.src_addr(self.rt.local_link_addr());
        frame_header.dest_addr(remote_link_addr);
        let _ = encoder.seal()?;

        self.rt.transmit(Rc::new(RefCell::new(segment_buf)));

        Ok(())
    }

    pub(super) fn poll_connect_finished(
        &mut self,
        fd: SocketDescriptor,
        context: &mut Context,
    ) -> Poll<Result<(), Fail>> {
        let key = match self.sockets.get(&fd) {
            Some(Socket::Connecting { local, remote }) => (*local, *remote),
            Some(..) => {
                return Poll::Ready(Err(Fail::Malformed {
                    details: "Socket not connecting",
                }))
            },
            None => return Poll::Ready(Err(Fail::Malformed { details: "Bad FD" })),
        };

        let result = {
            let socket = match self.connecting.get_mut(&key) {
                Some(s) => s,
                None => {
                    return Poll::Ready(Err(Fail::Malformed {
                        details: "Socket not connecting",
                    }))
                },
            };
            match socket.poll_result(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(r) => r,
            }
        };
        self.connecting.remove(&key);

        let cb = result?;
        assert!(self
            .established
            .insert(key, EstablishedSocket::new(cb))
            .is_none());
        let (local, remote) = key;
        self.sockets
            .insert(fd, Socket::Established { local, remote });

        Poll::Ready(Ok(()))
    }
}
