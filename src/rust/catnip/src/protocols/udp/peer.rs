// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use tracy_client::static_span;
use super::datagram::{
    UdpHeader,
    UdpDatagram,
};
use crate::operations::{ResultFuture, OperationResult};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ipv4,
        ipv4::datagram::{Ipv4Header, Ipv4Protocol2},
        ethernet2::frame::{EtherType2, Ethernet2Header},
    },
    runtime::Runtime,
    scheduler::SchedulerHandle,
};
use hashbrown::HashMap;
use std::{
    pin::Pin,
    task::{Context, Poll},
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    rc::Rc,
};
use crate::file_table::{File, FileTable, FileDescriptor};
use bytes::Bytes;
use std::task::Waker;
use futures_intrusive::NoopLock;
use futures_intrusive::channel::shared::{generic_channel, GenericSender, GenericReceiver};
use futures_intrusive::buffer::GrowingHeapBuf;

pub struct UdpPeer<RT: Runtime> {
    inner: Rc<RefCell<Inner<RT>>>,
}

struct Listener {
    buf: VecDeque<(Option<ipv4::Endpoint>, Bytes)>,
    waker: Option<Waker>,
}

#[derive(Debug)]
struct Socket {
    // `bind(2)` fixes a local address
    local: Option<ipv4::Endpoint>,
    // `connect(2)` fixes a remote address
    remote: Option<ipv4::Endpoint>,
}

type OutgoingReq = (Option<ipv4::Endpoint>, ipv4::Endpoint, Bytes);
type OutgoingSender = GenericSender<NoopLock, OutgoingReq, GrowingHeapBuf<OutgoingReq>>;
type OutgoingReceiver = GenericReceiver<NoopLock, OutgoingReq, GrowingHeapBuf<OutgoingReq>>;

struct Inner<RT: Runtime> {
    #[allow(unused)]
    rt: RT,
    #[allow(unused)]
    arp: arp::Peer<RT>,
    file_table: FileTable,

    sockets: HashMap<FileDescriptor, Socket>,
    bound: HashMap<ipv4::Endpoint, Listener>,

    outgoing: OutgoingSender,
    #[allow(unused)]
    handle: SchedulerHandle,
}


impl<RT: Runtime> UdpPeer<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>, file_table: FileTable) -> Self {
        let (tx, rx) = generic_channel(16);
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
        Self { inner: Rc::new(RefCell::new(inner)) }
    }

    async fn background(rt: RT, arp: arp::Peer<RT>, rx: OutgoingReceiver) {
        while let Some((local, remote, buf)) = rx.receive().await {
            let r: Result<_, Fail> = try {
                let link_addr = arp.query(remote.address()).await?;
                let _s = static_span!("bg_send_udp");
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
                };
                rt.transmit(datagram);
            };
            if let Err(e) = r {
                warn!("Failed to send UDP message: {:?}", e);
            }
        }
    }

    pub fn accept(&self) -> Fail {
        Fail::Malformed { details: "Operation not supported" }
    }

    pub fn socket(&self) -> FileDescriptor {
        let mut inner = self.inner.borrow_mut();
        let fd = inner.file_table.alloc(File::UdpSocket);
        let socket = Socket { local: None, remote: None };
        assert!(inner.sockets.insert(fd, socket).is_none());
        fd
    }

    pub fn bind(&self, fd: FileDescriptor, addr: ipv4::Endpoint) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        if inner.bound.contains_key(&addr) {
            return Err(Fail::Malformed { details: "Port already listening" })
        }
        match inner.sockets.get_mut(&fd) {
            Some(Socket { ref mut local, .. }) if local.is_none() => {
                *local = Some(addr);
            },
            _ => return Err(Fail::Malformed { details: "Invalid file descriptor on bind" }),
        }
        let listener = Listener {
            buf: VecDeque::new(),
            waker: None
        };
        assert!(inner.bound.insert(addr, listener).is_none());
        Ok(())
    }

    pub fn connect(&self, fd: FileDescriptor, addr: ipv4::Endpoint) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        match inner.sockets.get_mut(&fd) {
            Some(Socket { ref mut remote, ..}) if remote.is_none() => {
                *remote = Some(addr);
                Ok(())
            },
            _ => Err(Fail::Malformed { details: "Invalid file descriptor on connect" }),
        }
    }

    pub fn receive(&self, ipv4_header: &Ipv4Header, buf: Bytes) -> Result<(), Fail> {
        let (hdr, data) = UdpHeader::parse(ipv4_header, buf)?;
        let local = ipv4::Endpoint::new(ipv4_header.dst_addr, hdr.dst_port);
        let remote = hdr.src_port.map(|p| ipv4::Endpoint::new(ipv4_header.src_addr, p));

        // TODO: Send ICMPv4 error in this condition.
        let mut inner = self.inner.borrow_mut();
        let listener = inner.bound.get_mut(&local).ok_or_else(|| Fail::Malformed {
            details: "Port not bound",
        })?;
        listener.buf.push_back((remote, data));
        listener.waker.take().map(|w| w.wake());
        Ok(())
    }

    pub fn push(&self, fd: FileDescriptor, buf: Bytes) -> Result<(), Fail> {
        let _s = static_span!();
        let inner = self.inner.borrow();
        let (local, remote) = match inner.sockets.get(&fd) {
            Some(Socket { local, remote: Some(remote) }) => (*local, *remote),
            e => {
	        eprintln!("{:?} invalid for {}", e, fd);
	        return Err(Fail::Malformed { details: "Invalid file descriptor on push" })
	    },
        };
        inner.outgoing.try_send((local, remote, buf)).unwrap();
        Ok(())
    }

    pub fn pop(&self, fd: FileDescriptor) -> PopFuture<RT>{
        PopFuture { inner: self.inner.clone(), fd }
    }

    pub fn close(&self, fd: FileDescriptor) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        let socket = match inner.sockets.remove(&fd) {
            Some(s) => s,
            None => return Err(Fail::Malformed { details: "Invalid file descriptor" }),
        };
        if let Some(local) = socket.local {
            assert!(inner.bound.remove(&local).is_some());
        }
        inner.file_table.free(fd);
        Ok(())
    }

    pub fn poll_pop(&self, fd: FileDescriptor, ctx: &mut Context) -> Poll<Result<Bytes, Fail>> {
        let _s = static_span!();
        let mut inner = self.inner.borrow_mut();
        let local = match inner.sockets.get(&fd) {
            Some(Socket { local: Some(local), .. }) => *local,
            _ => return Poll::Ready(Err(Fail::Malformed { details: "Invalid file descriptor on poll pop" })),
        };
        let listener = inner.bound.get_mut(&local).unwrap();
        match listener.buf.pop_front() {
            Some((_, buf)) => return Poll::Ready(Ok(buf)),
            None => ()
        }
        listener.waker = Some(ctx.waker().clone());
        Poll::Pending
    }
}

pub struct PopFuture<RT: Runtime> {
    pub fd: FileDescriptor,
    inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> Future for PopFuture<RT> {
    type Output = Result<Bytes, Fail>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        UdpPeer { inner: self_.inner.clone() }.poll_pop(self_.fd, ctx)
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
            UdpOperation::Accept(..)
                | UdpOperation::Connect(..)
                | UdpOperation::Push(..) => Poll::Ready(()),
            UdpOperation::Pop(ref mut f) => Future::poll(Pin::new(f), ctx),
        }
    }
}

impl<RT: Runtime> UdpOperation<RT> {
    pub fn expect_result(self) -> (FileDescriptor, OperationResult) {
        match self {
            UdpOperation::Push(fd, Err(e))
                | UdpOperation::Connect(fd, Err(e))
                | UdpOperation::Accept(fd, e) => (fd, OperationResult::Failed(e)),
            UdpOperation::Connect(fd, Ok(())) => (fd, OperationResult::Connect),
            UdpOperation::Push(fd, Ok(())) => (fd, OperationResult::Push),

            UdpOperation::Pop(ResultFuture { future, done: Some(Ok(bytes)) }) =>
                (future.fd, OperationResult::Pop(bytes)),
            UdpOperation::Pop(ResultFuture { future, done: Some(Err(e)) }) =>
                (future.fd, OperationResult::Failed(e)),

            _ => panic!("Future not ready"),
        }
    }
}
