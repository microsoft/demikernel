// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Exports
//==============================================================================

pub mod futures;
pub mod runtime;

//==============================================================================
// Imports
//==============================================================================

use self::{
    futures::{
        accept::AcceptFuture,
        connect::ConnectFuture,
        pop::PopFuture,
        push::PushFuture,
        pushto::PushtoFuture,
        FutureOperation,
        OperationResult,
    },
    runtime::PosixRuntime,
};
use ::catnip::protocols::ipv4::Ipv4Endpoint;
use ::catwalk::SchedulerHandle;
use ::libc::{
    c_int,
    EBADF,
    EINVAL,
    ENOTSUP,
    SOCK_DGRAM,
    SOCK_STREAM,
};
use ::nix::{
    sys::{
        socket,
        socket::{
            AddressFamily,
            InetAddr,
            SockAddr,
            SockFlag,
            SockProtocol,
            SockType,
        },
    },
    unistd,
};
use ::runtime::{
    fail::Fail,
    logging,
    memory::{
        Buffer,
        Bytes,
        MemoryRuntime,
    },
    network::types::{
        Ipv4Addr,
        Port16,
    },
    queue::IoQueueTable,
    task::SchedulerRuntime,
    types::{
        dmtr_accept_result_t,
        dmtr_opcode_t,
        dmtr_qr_value_t,
        dmtr_qresult_t,
        dmtr_sgarray_t,
    },
    QDesc,
    QToken,
    QType,
};
use ::std::{
    any::Any,
    collections::HashMap,
    mem,
    os::unix::prelude::RawFd,
    time::Instant,
};

//==============================================================================
// Structures
//==============================================================================

/// Catnap LibOS
pub struct CatnapLibOS {
    qtable: IoQueueTable,
    sockets: HashMap<QDesc, RawFd>,
    runtime: PosixRuntime,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catnap LibOS
impl CatnapLibOS {
    /// Instantiates a Catnap LibOS.
    pub fn new() -> Self {
        // TODO: remove this
        logging::initialize();
        let qtable: IoQueueTable = IoQueueTable::new();
        let sockets: HashMap<QDesc, RawFd> = HashMap::new();
        let runtime: PosixRuntime = PosixRuntime::new(Instant::now());
        Self {
            qtable,
            sockets,
            runtime,
        }
    }

    pub fn socket(
        &mut self,
        _domain: c_int,
        socket_type: c_int,
        _protocol: c_int,
    ) -> Result<QDesc, Fail> {
        trace!("socket()");
        let domain: AddressFamily = AddressFamily::Inet;

        let (ty, protocol): (SockType, SockProtocol) = match socket_type {
            SOCK_STREAM => (SockType::Stream, SockProtocol::Tcp),
            SOCK_DGRAM => (SockType::Datagram, SockProtocol::Udp),
            _ => {
                return Err(Fail::new(ENOTSUP, "socket type not supported"));
            },
        };

        let flags: SockFlag = SockFlag::SOCK_NONBLOCK;

        match socket::socket(domain, ty, flags, protocol) {
            Ok(fd) => {
                let qtype: QType = QType::TcpSocket;
                let qd: QDesc = self.qtable.alloc(qtype.into());
                assert!(self.sockets.insert(qd, fd).is_none());
                Ok(qd)
            },
            Err(err) => panic!("{:?}", err),
        }
    }

    pub fn bind(&mut self, qd: QDesc, local: Ipv4Endpoint) -> Result<(), Fail> {
        trace!("bind()");
        let ipv4: std::net::IpAddr = std::net::IpAddr::V4(local.get_address());
        let ip: socket::IpAddr = socket::IpAddr::from_std(&ipv4);
        let portnum: Port16 = local.get_port();
        let inet: InetAddr = InetAddr::new(ip, portnum.into());
        let addr: SockAddr = SockAddr::new_inet(inet);

        match self.sockets.get(&qd) {
            Some(&fd) => {
                trace!("bind: ipv4={:?}, portnum={:?}", ipv4, portnum);
                socket::bind(fd, &addr).unwrap();
                Ok(())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen()");
        match self.sockets.get(&qd) {
            Some(&fd) => {
                socket::listen(fd, backlog).unwrap();
                Ok(())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept()");
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let future: FutureOperation = FutureOperation::from(AcceptFuture::new(qd, fd));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn connect(&mut self, qd: QDesc, remote: Ipv4Endpoint) -> Result<QToken, Fail> {
        trace!("connect()");
        let ipv4: std::net::IpAddr = std::net::IpAddr::V4(remote.get_address());
        let ip: socket::IpAddr = socket::IpAddr::from_std(&ipv4);
        let portnum: Port16 = remote.get_port();
        let inet: InetAddr = InetAddr::new(ip, portnum.into());
        let addr: SockAddr = SockAddr::new_inet(inet);

        match self.sockets.get(&qd) {
            Some(&fd) => {
                let future: FutureOperation =
                    FutureOperation::from(ConnectFuture::new(qd, fd, addr));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close()");
        match self.sockets.get(&qd) {
            Some(&fd) => match unistd::close(fd) {
                Ok(_) => Ok(()),
                _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn push(&mut self, qd: QDesc, sga: &dmtr_sgarray_t) -> Result<QToken, Fail> {
        trace!("push()");
        let buf: Bytes = self.runtime.clone_sgarray(sga);
        if buf.len() == 0 {
            return Err(Fail::new(EINVAL, "zero-length buffer"));
        }

        match self.sockets.get(&qd) {
            Some(&fd) => {
                let future: FutureOperation = FutureOperation::from(PushFuture::new(qd, fd, buf));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn pushto(
        &mut self,
        qd: QDesc,
        sga: &dmtr_sgarray_t,
        remote: Ipv4Endpoint,
    ) -> Result<QToken, Fail> {
        trace!("pushto() qd={:?}", qd,);
        let ipv4: std::net::IpAddr = std::net::IpAddr::V4(remote.get_address());
        let ip: socket::IpAddr = socket::IpAddr::from_std(&ipv4);
        let portnum: Port16 = remote.get_port();
        let inet: InetAddr = InetAddr::new(ip, portnum.into());
        let addr: SockAddr = SockAddr::new_inet(inet);
        let buf: Bytes = self.runtime.clone_sgarray(sga);
        if buf.len() == 0 {
            return Err(Fail::new(EINVAL, "zero-length buffer"));
        }

        match self.sockets.get(&qd) {
            Some(&fd) => {
                trace!("pushto {:?}", addr);
                let future: FutureOperation =
                    FutureOperation::from(PushtoFuture::new(qd, fd, addr, buf));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn pushto2(
        &mut self,
        qd: QDesc,
        data: &[u8],
        remote: Ipv4Endpoint,
    ) -> Result<QToken, Fail> {
        trace!("pushto2() ");
        let ipv4: std::net::IpAddr = std::net::IpAddr::V4(remote.get_address());
        let ip: socket::IpAddr = socket::IpAddr::from_std(&ipv4);
        let portnum: Port16 = remote.get_port();
        let inet: InetAddr = InetAddr::new(ip, portnum.into());
        let addr: SockAddr = SockAddr::new_inet(inet);

        let buf: Bytes = Bytes::from_slice(data);
        if buf.len() == 0 {
            return Err(Fail::new(EINVAL, "zero-length buffer"));
        }

        match self.sockets.get(&qd) {
            Some(&fd) => {
                let future: FutureOperation =
                    FutureOperation::from(PushtoFuture::new(qd, fd, addr, buf));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("pop()");
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let future: FutureOperation = FutureOperation::from(PopFuture::new(qd, fd));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                let qt: QToken = handle.into_raw().into();
                Ok(qt)
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    pub fn poll(&mut self, qt: QToken) -> Option<dmtr_qresult_t> {
        trace!("poll(): qt={:?}", qt);

        let handle: SchedulerHandle = self.runtime.get_handle(qt.into()).unwrap();
        self.runtime.poll();

        self.runtime.poll();
        if !handle.has_completed() {
            return None;
        };

        let (qd, r) = self.take_operation(handle);
        Some(pack_result(&self.runtime, r, qd, qt.into()))
    }

    pub fn drop_qtoken(&mut self, _qt: QToken) {
        todo!();
    }

    fn do_wait(&mut self, qt: QToken) -> (QDesc, OperationResult) {
        trace!("do_wait() qt={:?}", qt);
        let handle: SchedulerHandle = self.runtime.get_handle(qt.into()).unwrap();

        loop {
            self.runtime.poll();
            if handle.has_completed() {
                return self.take_operation(handle);
            }
        }
    }

    pub fn wait(&mut self, qt: QToken) -> dmtr_qresult_t {
        trace!("wait()");
        let (qd, result) = self.do_wait(qt);
        pack_result(&self.runtime, result, qd, qt.into())
    }

    pub fn wait2(&mut self, qt: QToken) -> (QDesc, OperationResult) {
        self.do_wait(qt)
    }

    pub fn wait_any(&mut self, _qts: &[QToken]) -> (usize, dmtr_qresult_t) {
        todo!();
    }

    pub fn wait_any2(&mut self, qts: &[QToken]) -> (usize, QDesc, OperationResult) {
        trace!("wait_any2 {:?}", qts);
        loop {
            self.runtime.poll();
            for (i, &qt) in qts.iter().enumerate() {
                let handle = self.runtime.get_handle(qt.into()).unwrap();
                if handle.has_completed() {
                    let (qd, r) = self.take_operation(handle);
                    return (i, qd, r);
                }
                handle.into_raw();
            }
        }
    }

    pub fn sgaalloc(&self, size: usize) -> Result<dmtr_sgarray_t, Fail> {
        trace!("sgalloc() size={:?}", size);
        Ok(self.runtime.alloc_sgarray(size))
    }

    pub fn sgafree(&self, sga: dmtr_sgarray_t) -> Result<(), Fail> {
        trace!("sgafree()");
        self.runtime.free_sgarray(sga);
        Ok(())
    }

    pub fn local_ipv4_addr(&self) -> Ipv4Addr {
        todo!();
    }

    pub fn rt(&self) -> &PosixRuntime {
        &self.runtime
    }

    fn take_operation(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let boxed_future: Box<dyn Any> = self.runtime.take(handle).as_any();

        let boxed_concrete_type = *boxed_future
            .downcast::<FutureOperation>()
            .expect("Wrong type!");

        boxed_concrete_type.expect_result()
    }
}

fn pack_result(rt: &PosixRuntime, result: OperationResult, qd: QDesc, qt: u64) -> dmtr_qresult_t {
    match result {
        OperationResult::Connect => dmtr_qresult_t {
            qr_opcode: dmtr_opcode_t::DMTR_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept(new_qd) => {
            let sin = unsafe { mem::zeroed() };
            let qr_value = dmtr_qr_value_t {
                ares: dmtr_accept_result_t {
                    qd: new_qd.into(),
                    addr: sin,
                },
            };
            dmtr_qresult_t {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_ACCEPT,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value,
            }
        },
        OperationResult::Push => dmtr_qresult_t {
            qr_opcode: dmtr_opcode_t::DMTR_OPC_PUSH,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Pop(addr, bytes) => {
            let mut sga: dmtr_sgarray_t = rt.into_sgarray(bytes);
            if let Some((ipv4, port16)) = addr {
                sga.sga_addr.sin_port = port16.into();
                sga.sga_addr.sin_addr.s_addr = u32::from_le_bytes(ipv4.octets());
            }
            let qr_value: dmtr_qr_value_t = dmtr_qr_value_t { sga };
            dmtr_qresult_t {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_POP,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value,
            }
        },
        OperationResult::Failed(e) => {
            warn!("Operation Failed: {:?}", e);
            dmtr_qresult_t {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_FAILED,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
