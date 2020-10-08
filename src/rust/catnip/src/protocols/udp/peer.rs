// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::datagram::{
    UdpDatagram,
    UdpDatagramDecoder,
    UdpDatagramEncoder,
};
use crate::operations::{ResultFuture, OperationResult};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ipv4,
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
    convert::TryFrom,
    future::Future,
    rc::Rc,
};
use crate::file_table::{File, FileTable, FileDescriptor};
use bytes::{Bytes, BytesMut};
use std::task::Waker;
use futures_intrusive::NoopLock;
use futures_intrusive::channel::shared::{generic_channel, GenericSender, GenericReceiver};
use futures_intrusive::buffer::GrowingHeapBuf;

pub struct UdpPeer<RT: Runtime> {
    inner: Rc<RefCell<Inner<RT>>>,
}

struct Listener {
    buf: VecDeque<(ipv4::Endpoint, Bytes)>,
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
                let mut bytes = UdpDatagramEncoder::new_vec(buf.len());
                let mut encoder = UdpDatagramEncoder::attach(&mut bytes);
                encoder.text()[..buf.len()].copy_from_slice(&buf);
                let mut udp_header = encoder.header();
                udp_header.dest_port(remote.port());
                if let Some(local) = local {
                    udp_header.src_port(local.port());
                }
                let mut ipv4_header = encoder.ipv4().header();
                ipv4_header.src_addr(rt.local_ipv4_addr());
                ipv4_header.dest_addr(remote.address());
                let mut frame_header = encoder.ipv4().frame().header();
                frame_header.dest_addr(link_addr);
                frame_header.src_addr(rt.local_link_addr());
                let _ = encoder.seal()?;
                rt.transmit(Rc::new(RefCell::new(bytes)));
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

    pub fn receive(&self, ipv4_datagram: ipv4::Datagram<'_>) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();

        trace!("UdpPeer::receive(...)");
        let decoder = UdpDatagramDecoder::try_from(ipv4_datagram)?;
        let udp_datagram = UdpDatagram::try_from(decoder)?;

        let local_ipv4_addr = udp_datagram.dest_ipv4_addr.ok_or_else(|| Fail::Malformed {
            details: "Missing destination IPv4 addr",
        })?;
        let local_port = udp_datagram.dest_port.ok_or_else(|| Fail::Malformed {
            details: "Missing destination port",
        })?;
        let local = ipv4::Endpoint::new(local_ipv4_addr, local_port);

        let remote_ipv4_addr = udp_datagram.src_ipv4_addr.ok_or_else(|| Fail::Malformed {
            details: "Missing source IPv4 addr",
        })?;
        let remote_port = udp_datagram.src_port.ok_or_else(|| Fail::Malformed {
            details: "Missing source port",
        })?;
        let remote = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

        // TODO: Send ICMPv4 error in this condition.
        let listener = inner.bound.get_mut(&local).ok_or_else(|| Fail::Malformed {
            details: "Port not bound",
        })?;

        let bytes = BytesMut::from(&udp_datagram.payload[..]).freeze();
        listener.buf.push_back((remote, bytes));
        listener.waker.take().map(|w| w.wake());

        Ok(())
    }

    pub fn push(&self, fd: FileDescriptor, buf: Bytes) -> Result<(), Fail> {
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
        let inner = self.inner.borrow_mut();
        if !inner.sockets.contains_key(&fd) {
            return Err(Fail::Malformed { details: "Invalid file descriptor on close" });
        }
	// TODO: Actually clean up state here.
        Ok(())
    }

    pub fn poll_pop(&self, fd: FileDescriptor, ctx: &mut Context) -> Poll<Result<Bytes, Fail>> {
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
