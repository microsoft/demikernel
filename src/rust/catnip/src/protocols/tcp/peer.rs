use super::{
    active_open::ActiveOpenSocket,
    established::EstablishedSocket,
    isn_generator::IsnGenerator,
    passive_open::PassiveSocket,
};
use crate::{
    runtime::RuntimeBuf,
    fail::Fail,
    file_table::{
        File,
        FileDescriptor,
        FileTable,
    },
    protocols::{
        arp,
        ethernet2::frame::{
            EtherType2,
            Ethernet2Header,
        },
        ip,
        ip::port::EphemeralPorts,
        ipv4,
        ipv4::datagram::{
            Ipv4Header,
            Ipv4Protocol2,
        },
        tcp::{
            operations::{
                AcceptFuture,
                ConnectFuture,
                ConnectFutureState,
                PopFuture,
                PushFuture,
            },
            segment::{
                TcpHeader,
                TcpSegment,
            },
        },
    },
    runtime::Runtime,
    scheduler::SchedulerHandle,
};
use futures::channel::mpsc;
use futures::stream::StreamExt;
use std::collections::HashMap;
use std::{
    cell::RefCell,
    rc::Rc,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

pub struct Peer<RT: Runtime> {
    pub(super) inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> Peer<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>, file_table: FileTable) -> Self {
        let (tx, rx) = mpsc::unbounded();
        let inner = Rc::new(RefCell::new(Inner::new(rt.clone(), arp, file_table, tx)));
        let bg_handle = rt.spawn(Self::background(rx, inner.clone()));
        inner.borrow_mut().dead_socket_handle = Some(bg_handle);
        Self { inner }
    }

    async fn background(
        mut dead_socket_rx: mpsc::UnboundedReceiver<FileDescriptor>,
        inner: Rc<RefCell<Inner<RT>>>,
    ) {
        while let Some(fd) = dead_socket_rx.next().await {
            let mut inner = inner.borrow_mut();

            let (local, remote) = match inner.sockets.remove(&fd) {
                None => continue,
                Some(Socket::Established { local, remote }) => (local, remote),
                _ => panic!(
                    "Received dead socket message for non-established socket: {}",
                    fd
                ),
            };
            let socket = inner
                .established
                .remove(&(local, remote))
                .unwrap_or_else(|| {
                    panic!(
                        "sockets/established inconsistency: {}, {:?}, {:?}",
                        fd, local, remote
                    )
                });

            // TODO: Assert we've been properly closed here.
            // TODO: Recycle this FD.
            info!("Cleaning up dead socket for FD {}", fd);
            drop(socket);
        }
    }

    pub fn socket(&self) -> FileDescriptor {
        let mut inner = self.inner.borrow_mut();
        let fd = inner.file_table.alloc(File::TcpSocket);
        assert!(inner
            .sockets
            .insert(fd, Socket::Inactive { local: None })
            .is_none());
        fd
    }

    pub fn bind(&self, fd: FileDescriptor, addr: ipv4::Endpoint) -> Result<(), Fail> {
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

    pub fn receive(&self, ip_header: &Ipv4Header, buf: RT::Buf) -> Result<(), Fail> {
        self.inner.borrow_mut().receive(ip_header, buf)
    }

    pub fn listen(&self, fd: FileDescriptor, backlog: usize) -> Result<(), Fail> {
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

    pub fn poll_accept(
        &self,
        fd: FileDescriptor,
        ctx: &mut Context,
    ) -> Poll<Result<FileDescriptor, Fail>> {
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
        let fd = inner.file_table.alloc(File::TcpSocket);
        let established = EstablishedSocket::new(cb, fd, inner.dead_socket_tx.clone());
        let key = (established.cb.local.clone(), established.cb.remote.clone());

        let socket = Socket::Established {
            local: established.cb.local.clone(),
            remote: established.cb.remote.clone(),
        };
        assert!(inner.sockets.insert(fd, socket).is_none());
        assert!(inner.established.insert(key, established).is_none());

        Poll::Ready(Ok(fd))
    }

    pub fn accept(&self, fd: FileDescriptor) -> AcceptFuture<RT> {
        AcceptFuture {
            fd,
            inner: self.inner.clone(),
        }
    }

    pub fn connect(&self, fd: FileDescriptor, remote: ipv4::Endpoint) -> ConnectFuture<RT> {
        let mut inner = self.inner.borrow_mut();

        let r = try {
            match inner.sockets.get_mut(&fd) {
                Some(Socket::Inactive { .. }) => (),
                _ => Err(Fail::Malformed {
                    details: "Invalid file descriptor",
                })?,
            }

            // TODO: We need to free these!
            let local_port = inner.ephemeral_ports.alloc()?;
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

    pub fn peek(&self, fd: FileDescriptor) -> Result<RT::Buf, Fail> {
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

    pub fn recv(&self, fd: FileDescriptor) -> Result<Option<RT::Buf>, Fail> {
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

    pub fn poll_recv(&self, fd: FileDescriptor, ctx: &mut Context) -> Poll<Result<RT::Buf, Fail>> {
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

    pub fn push(&self, fd: FileDescriptor, buf: RT::Buf) -> PushFuture<RT> {
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

    pub fn pop(&self, fd: FileDescriptor) -> PopFuture<RT> {
        PopFuture {
            fd,
            inner: self.inner.clone(),
        }
    }

    fn send(&self, fd: FileDescriptor, buf: RT::Buf) -> Result<(), Fail> {
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

    pub fn close(&self, fd: FileDescriptor) -> Result<(), Fail> {
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
                // unimplemented!();
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        }
        Ok(())
    }

    pub fn remote_mss(&self, fd: FileDescriptor) -> Result<usize, Fail> {
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

    pub fn current_rto(&self, fd: FileDescriptor) -> Result<Duration, Fail> {
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

    pub fn endpoints(&self, fd: FileDescriptor) -> Result<(ipv4::Endpoint, ipv4::Endpoint), Fail> {
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

    file_table: FileTable,
    ephemeral_ports: EphemeralPorts,

    // FD -> local port
    sockets: HashMap<FileDescriptor, Socket>,

    passive: HashMap<ipv4::Endpoint, PassiveSocket<RT>>,
    connecting: HashMap<(ipv4::Endpoint, ipv4::Endpoint), ActiveOpenSocket<RT>>,
    established: HashMap<(ipv4::Endpoint, ipv4::Endpoint), EstablishedSocket<RT>>,

    rt: RT,
    arp: arp::Peer<RT>,

    dead_socket_tx: mpsc::UnboundedSender<FileDescriptor>,
    dead_socket_handle: Option<SchedulerHandle>,
}

impl<RT: Runtime> Inner<RT> {
    fn new(
        rt: RT,
        arp: arp::Peer<RT>,
        file_table: FileTable,
        dead_socket_tx: mpsc::UnboundedSender<FileDescriptor>,
    ) -> Self {
        Self {
            isn_generator: IsnGenerator::new(rt.rng_gen()),
            file_table,
            ephemeral_ports: EphemeralPorts::new(&rt),
            sockets: HashMap::new(),
            passive: HashMap::new(),
            connecting: HashMap::new(),
            established: HashMap::new(),
            rt,
            arp,
            dead_socket_tx,
            dead_socket_handle: None,
        }
    }

    fn receive(&mut self, ip_hdr: &Ipv4Header, buf: RT::Buf) -> Result<(), Fail> {
        let tcp_options = self.rt.tcp_options();
        let (tcp_hdr, data) = TcpHeader::parse(ip_hdr, buf, tcp_options.rx_checksum_offload)?;
        debug!("TCP received {:?}", tcp_hdr);
        let local = ipv4::Endpoint::new(ip_hdr.dst_addr, tcp_hdr.dst_port);
        let remote = ipv4::Endpoint::new(ip_hdr.src_addr, tcp_hdr.src_port);

        if remote.addr.is_broadcast() || remote.addr.is_multicast() || remote.addr.is_unspecified()
        {
            return Err(Fail::Malformed {
                details: "Invalid address type",
            });
        }
        let key = (local, remote);

        if let Some(s) = self.established.get(&key) {
            debug!("Routing to established connection: {:?}", key);
            s.receive(&tcp_hdr, data);
            return Ok(());
        }
        if let Some(s) = self.connecting.get_mut(&key) {
            debug!("Routing to connecting connection: {:?}", key);
            s.receive(&tcp_hdr);
            return Ok(());
        }
        let (local, _) = key;
        if let Some(s) = self.passive.get_mut(&local) {
            debug!("Routing to passive connection: {:?}", local);
            return s.receive(ip_hdr, &tcp_hdr);
        }

        // The packet isn't for an open port; send a RST segment.
        debug!("Sending RST for {:?}, {:?}", local, remote);
        self.send_rst(&local, &remote)?;
        Ok(())
    }

    fn send_rst(&mut self, local: &ipv4::Endpoint, remote: &ipv4::Endpoint) -> Result<(), Fail> {
        // TODO: Make this work pending on ARP resolution if needed.
        let remote_link_addr =
            self.arp
                .try_query(remote.addr)
                .ok_or_else(|| Fail::ResourceNotFound {
                    details: "RST destination not in ARP cache",
                })?;

        let mut tcp_hdr = TcpHeader::new(local.port, remote.port);
        tcp_hdr.rst = true;

        let segment = TcpSegment {
            ethernet2_hdr: Ethernet2Header {
                dst_addr: remote_link_addr,
                src_addr: self.rt.local_link_addr(),
                ether_type: EtherType2::Ipv4,
            },
            ipv4_hdr: Ipv4Header::new(local.addr, remote.addr, Ipv4Protocol2::Tcp),
            tcp_hdr,
            data: RT::Buf::empty(),
            tx_checksum_offload: self.rt.tcp_options().tx_checksum_offload,
        };
        self.rt.transmit(segment);

        Ok(())
    }

    pub(super) fn poll_connect_finished(
        &mut self,
        fd: FileDescriptor,
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
        let socket = EstablishedSocket::new(cb, fd, self.dead_socket_tx.clone());
        assert!(self.established.insert(key, socket).is_none());
        let (local, remote) = key;
        self.sockets
            .insert(fd, Socket::Established { local, remote });

        Poll::Ready(Ok(()))
    }
}
