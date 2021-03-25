// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::datagram::{
    UdpDatagram,
    UdpHeader,
};
use crate::{
    fail::Fail,
    file_table::{
        File,
        FileDescriptor,
        FileTable,
    },
    operations::{
        OperationResult,
        ResultFuture,
    },
    protocols::{
        arp,
        ethernet2::frame::{
            EtherType2,
            Ethernet2Header,
        },
        ipv4,
        ipv4::datagram::{
            Ipv4Header,
            Ipv4Protocol2,
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
    collections::VecDeque,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
};

pub struct UdpPeer<RT: Runtime> {
    inner: Rc<RefCell<Inner<RT>>>,
}

struct Listener<T> {
    buf: VecDeque<(Option<ipv4::Endpoint>, T)>,
    waker: Option<Waker>,
}

#[derive(Debug)]
struct Socket {
    // `bind(2)` fixes a local address
    local: Option<ipv4::Endpoint>,
    // `connect(2)` fixes a remote address
    remote: Option<ipv4::Endpoint>,
}

type OutgoingReq<T> = (Option<ipv4::Endpoint>, ipv4::Endpoint, T);
type OutgoingSender<T> = mpsc::UnboundedSender<OutgoingReq<T>>;
type OutgoingReceiver<T> = mpsc::UnboundedReceiver<OutgoingReq<T>>;

struct Inner<RT: Runtime> {
    rt: RT,
    #[allow(unused)]
    arp: arp::Peer<RT>,
    file_table: FileTable,

    sockets: HashMap<FileDescriptor, Socket>,
    bound: HashMap<ipv4::Endpoint, Rc<RefCell<Listener<RT::Buf>>>>,

    outgoing: OutgoingSender<RT::Buf>,
    #[allow(unused)]
    handle: SchedulerHandle,
}

impl<RT: Runtime> UdpPeer<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>, file_table: FileTable) -> Self {
        let (tx, rx) = mpsc::unbounded();
        let future = Self::background(rt.clone(), arp.clone(), rx);
        let handle = rt.spawn(future);
        let inner = Inner {
            rt,
            arp,
            file_table,
            sockets: HashMap::new(),
            bound: HashMap::new(),
            outgoing: tx,
            handle,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    async fn background(rt: RT, arp: arp::Peer<RT>, mut rx: OutgoingReceiver<RT::Buf>) {
        while let Some((local, remote, buf)) = rx.next().await {
            let r: Result<_, Fail> = try {
                let link_addr = arp.query(remote.addr).await?;
                let datagram = UdpDatagram {
                    ethernet2_hdr: Ethernet2Header {
                        dst_addr: link_addr,
                        src_addr: rt.local_link_addr(),
                        ether_type: EtherType2::Ipv4,
                    },
                    ipv4_hdr: Ipv4Header::new(
                        rt.local_ipv4_addr(),
                        remote.addr,
                        Ipv4Protocol2::Udp,
                    ),
                    udp_hdr: UdpHeader {
                        src_port: local.map(|l| l.port),
                        dst_port: remote.port,
                    },
                    data: buf,

                    tx_checksum_offload: rt.udp_options().tx_checksum_offload,
                };
                rt.transmit(datagram);
            };
            if let Err(e) = r {
                warn!("Failed to send UDP message: {:?}", e);
            }
        }
    }

    pub fn accept(&self) -> Fail {
        Fail::Malformed {
            details: "Operation not supported",
        }
    }

    pub fn socket(&self) -> FileDescriptor {
        let mut inner = self.inner.borrow_mut();
        let fd = inner.file_table.alloc(File::UdpSocket);
        let socket = Socket {
            local: None,
            remote: None,
        };
        assert!(inner.sockets.insert(fd, socket).is_none());
        fd
    }

    pub fn bind(&self, fd: FileDescriptor, addr: ipv4::Endpoint) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        if inner.bound.contains_key(&addr) {
            return Err(Fail::Malformed {
                details: "Port already listening",
            });
        }
        match inner.sockets.get_mut(&fd) {
            Some(Socket { ref mut local, .. }) if local.is_none() => {
                *local = Some(addr);
            },
            _ => {
                return Err(Fail::Malformed {
                    details: "Invalid file descriptor on bind",
                })
            },
        }
        let listener = Listener {
            buf: VecDeque::new(),
            waker: None,
        };
        assert!(inner
            .bound
            .insert(addr, Rc::new(RefCell::new(listener)))
            .is_none());
        Ok(())
    }

    pub fn connect(&self, fd: FileDescriptor, addr: ipv4::Endpoint) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        match inner.sockets.get_mut(&fd) {
            Some(Socket { ref mut remote, .. }) if remote.is_none() => {
                *remote = Some(addr);
                Ok(())
            },
            _ => Err(Fail::Malformed {
                details: "Invalid file descriptor on connect",
            }),
        }
    }

    pub fn receive(&self, ipv4_header: &Ipv4Header, buf: RT::Buf) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        let (hdr, data) = UdpHeader::parse(ipv4_header, buf, inner.rt.udp_options().rx_checksum_offload)?;
        let local = ipv4::Endpoint::new(ipv4_header.dst_addr, hdr.dst_port);
        let remote = hdr
            .src_port
            .map(|p| ipv4::Endpoint::new(ipv4_header.src_addr, p));

        // TODO: Send ICMPv4 error in this condition.
        let listener = inner.bound.get_mut(&local).ok_or_else(|| Fail::Malformed {
            details: "Port not bound",
        })?;
        let mut l = listener.borrow_mut();
        l.buf.push_back((remote, data));
        l.waker.take().map(|w| w.wake());
        Ok(())
    }

    pub fn push(&self, fd: FileDescriptor, buf: RT::Buf) -> Result<(), Fail> {
        let inner = self.inner.borrow();
        let (local, remote) = match inner.sockets.get(&fd) {
            Some(Socket {
                local,
                remote: Some(remote),
            }) => (*local, *remote),
            _ => {
                return Err(Fail::Malformed {
                    details: "Invalid file descriptor on push",
                })
            },
        };
        inner.send_datagram(buf, local, remote)
    }

    pub fn pushto(&self, fd: FileDescriptor, buf: RT::Buf, to: ipv4::Endpoint) -> Result<(), Fail> {
        let inner = self.inner.borrow();
        let local = match inner.sockets.get(&fd) {
            Some(Socket { local, .. }) => *local,
            _ => {
                return Err(Fail::Malformed {
                    details: "Invalid file descriptor on pushto",
                })
            },
        };
        inner.send_datagram(buf, local, to)
    }

    pub fn pop(&self, fd: FileDescriptor) -> PopFuture<RT> {
        let inner = self.inner.borrow();
        let listener = match inner.sockets.get(&fd) {
            Some(Socket {
                local: Some(local), ..
            }) => Ok(inner.bound.get(&local).unwrap().clone()),
            _ => Err(Fail::Malformed {
                details: "Invalid file descriptor",
            }),
        };
        PopFuture { listener, fd }
    }

    pub fn close(&self, fd: FileDescriptor) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        let socket = match inner.sockets.remove(&fd) {
            Some(s) => s,
            None => {
                return Err(Fail::Malformed {
                    details: "Invalid file descriptor",
                })
            },
        };
        if let Some(local) = socket.local {
            assert!(inner.bound.remove(&local).is_some());
        }
        inner.file_table.free(fd);
        Ok(())
    }
}

impl<RT: Runtime> Inner<RT> {
    fn send_datagram(
        &self,
        buf: RT::Buf,
        local: Option<ipv4::Endpoint>,
        remote: ipv4::Endpoint,
    ) -> Result<(), Fail> {
        // First, try to send the packet immediately.
        if let Some(link_addr) = self.arp.try_query(remote.addr) {
            let datagram = UdpDatagram {
                ethernet2_hdr: Ethernet2Header {
                    dst_addr: link_addr,
                    src_addr: self.rt.local_link_addr(),
                    ether_type: EtherType2::Ipv4,
                },
                ipv4_hdr: Ipv4Header::new(
                    self.rt.local_ipv4_addr(),
                    remote.addr,
                    Ipv4Protocol2::Udp,
                ),
                udp_hdr: UdpHeader {
                    src_port: local.map(|l| l.port),
                    dst_port: remote.port,
                },
                data: buf,

                tx_checksum_offload: self.rt.udp_options().tx_checksum_offload,
            };
            self.rt.transmit(datagram);
        }
        // Otherwise defer to the async path.
        else {
            self.outgoing.unbounded_send((local, remote, buf)).unwrap();
        }
        Ok(())
    }
}

pub struct PopFuture<RT: Runtime> {
    pub fd: FileDescriptor,
    listener: Result<Rc<RefCell<Listener<RT::Buf>>>, Fail>,
}

impl<RT: Runtime> Future for PopFuture<RT> {
    type Output = Result<(Option<ipv4::Endpoint>, RT::Buf), Fail>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        match self_.listener {
            Err(ref e) => Poll::Ready(Err(e.clone())),
            Ok(ref l) => {
                let mut listener = l.borrow_mut();
                match listener.buf.pop_front() {
                    Some(r) => return Poll::Ready(Ok(r)),
                    None => (),
                }
                let waker = ctx.waker();
                listener.waker = Some(waker.clone());
                Poll::Pending
            },
        }
    }
}

pub enum UdpOperation<RT: Runtime> {
    Accept(FileDescriptor, Fail),
    Connect(FileDescriptor, Result<(), Fail>),
    Push(FileDescriptor, Result<(), Fail>),
    Pop(ResultFuture<PopFuture<RT>>),
}

impl<RT: Runtime> Future for UdpOperation<RT> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        match self.get_mut() {
            UdpOperation::Accept(..) | UdpOperation::Connect(..) | UdpOperation::Push(..) => {
                Poll::Ready(())
            },
            UdpOperation::Pop(ref mut f) => Future::poll(Pin::new(f), ctx),
        }
    }
}

impl<RT: Runtime> UdpOperation<RT> {
    pub fn expect_result(self) -> (FileDescriptor, OperationResult<RT>) {
        match self {
            UdpOperation::Push(fd, Err(e))
            | UdpOperation::Connect(fd, Err(e))
            | UdpOperation::Accept(fd, e) => (fd, OperationResult::Failed(e)),
            UdpOperation::Connect(fd, Ok(())) => (fd, OperationResult::Connect),
            UdpOperation::Push(fd, Ok(())) => (fd, OperationResult::Push),

            UdpOperation::Pop(ResultFuture {
                future,
                done: Some(Ok((addr, bytes))),
            }) => (future.fd, OperationResult::Pop(addr, bytes)),
            UdpOperation::Pop(ResultFuture {
                future,
                done: Some(Err(e)),
            }) => (future.fd, OperationResult::Failed(e)),

            _ => panic!("Future not ready"),
        }
    }
}
