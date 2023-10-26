// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod iouring;
mod queue;
mod runtime;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{
    queue::CatcollarQueue,
    runtime::{
        RequestId,
        SharedIoUringRuntime,
    },
};

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    pal::{
        constants::SOMAXCONN,
        data_structures::{
            SockAddr,
            SockAddrIn,
            Socklen,
        },
        linux,
    },
    runtime::{
        fail::Fail,
        limits,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::unwrap_socketaddr,
        queue::{
            downcast_queue_ptr,
            IoQueueTable,
            NetworkQueue,
            Operation,
            OperationResult,
            OperationTask,
            QDesc,
            QToken,
            QType,
        },
        types::{
            demi_accept_result_t,
            demi_opcode_t,
            demi_qr_value_t,
            demi_qresult_t,
            demi_sgarray_t,
        },
        DemiRuntime,
    },
    scheduler::{
        TaskHandle,
        Yielder,
    },
};
use ::std::{
    cell::{
        Ref,
        RefCell,
    },
    mem,
    net::{
        SocketAddr,
        SocketAddrV4,
    },
    os::unix::prelude::RawFd,
    pin::Pin,
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Catcollar LibOS
pub struct CatcollarLibOS {
    /// Table of queue descriptors.
    qtable: Rc<RefCell<IoQueueTable>>, // TODO: Move this into runtime module.
    /// Underlying runtime.
    runtime: SharedIoUringRuntime,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associate Functions for Catcollar LibOS
impl CatcollarLibOS {
    /// Instantiates a Catcollar LibOS.
    pub fn new(_config: &Config) -> Self {
        let qtable: Rc<RefCell<IoQueueTable>> = Rc::new(RefCell::new(IoQueueTable::new()));
        let runtime: SharedIoUringRuntime = SharedIoUringRuntime::default();
        Self { qtable, runtime }
    }

    /// Creates a socket.
    pub fn socket(&mut self, domain: libc::c_int, typ: libc::c_int, _protocol: libc::c_int) -> Result<QDesc, Fail> {
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        // Parse communication domain.
        if domain != libc::AF_INET {
            return Err(Fail::new(libc::ENOTSUP, "communication domain not supported"));
        }

        // Parse socket type and protocol.
        if (typ != libc::SOCK_STREAM) && (typ != libc::SOCK_DGRAM) {
            return Err(Fail::new(libc::ENOTSUP, "socket type not supported"));
        }

        // Create socket.
        match unsafe { libc::socket(domain, typ, 0) } {
            fd if fd >= 0 => {
                let qtype: QType = match typ {
                    libc::SOCK_STREAM => QType::TcpSocket,
                    libc::SOCK_DGRAM => QType::UdpSocket,
                    _ => return Err(Fail::new(libc::ENOTSUP, "socket type not supported")),
                };

                // Set socket options.
                unsafe {
                    if typ == libc::SOCK_STREAM {
                        if linux::set_tcp_nodelay(fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set TCP_NONDELAY option (errno={:?})", errno);
                        }
                    }
                    if linux::set_nonblock(fd) != 0 {
                        let errno: libc::c_int = *libc::__errno_location();
                        warn!("cannot set O_NONBLOCK option (errno={:?})", errno);
                    }
                    if linux::set_so_reuseport(fd) != 0 {
                        let errno: libc::c_int = *libc::__errno_location();
                        warn!("cannot set SO_REUSEPORT option (errno={:?})", errno);
                    }
                }

                trace!("socket: {:?}, domain: {:?}, typ: {:?}", fd, domain, typ);
                let mut queue: CatcollarQueue = CatcollarQueue::new(qtype);
                queue.set_fd(fd);
                Ok(self.qtable.borrow_mut().alloc::<CatcollarQueue>(queue))
            },
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                Err(Fail::new(errno, "failed to create socket"))
            },
        }
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let local: SocketAddrV4 = unwrap_socketaddr(local)?;

        // Check if we are binding to the wildcard port.
        if local.port() == 0 {
            let cause: String = format!("cannot bind to port 0 (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Check whether the address is in use.
        if self.addr_in_use(local) {
            let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }
        // Get reference to the underlying file descriptor.
        let fd: RawFd = self.get_queue_fd(&qd)?;

        // Bind underlying socket.
        let saddr: SockAddr = linux::socketaddrv4_to_sockaddr(&local);
        match unsafe { libc::bind(fd, &saddr as *const SockAddr, mem::size_of::<SockAddrIn>() as Socklen) } {
            stats if stats == 0 => {
                // Expect is safe here because we already looked up the queue in get_queue_fd().
                self.qtable
                    .borrow_mut()
                    .get_mut::<CatcollarQueue>(&qd)
                    .expect("queue should exist")
                    .set_addr(local);
                Ok(())
            },
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                error!("failed to bind socket (errno={:?})", errno);
                Err(Fail::new(errno, "operation failed"))
            },
        }
    }

    /// Sets a socket as a passive one.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // We just assert backlog here, because it was previously checked at PDPIX layer.
        debug_assert!((backlog > 0) && (backlog <= SOMAXCONN as usize));

        // Issue listen operation.
        let fd: RawFd = self.get_queue_fd(&qd)?;
        if unsafe { libc::listen(fd, backlog as i32) } != 0 {
            let errno: libc::c_int = unsafe { *libc::__errno_location() };
            error!("failed to listen ({:?})", errno);
            return Err(Fail::new(errno, "operation failed"));
        }
        Ok(())
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);

        let fd: RawFd = self.get_queue_fd(&qd)?;

        // Issue accept operation.
        let yielder: Yielder = Yielder::new();
        let coroutine: Pin<Box<Operation>> = Box::pin(Self::accept_coroutine(self.qtable.clone(), qd, fd, yielder));
        let task_id: String = format!("Catcollar::accept for qd={:?}", qd);
        Ok(self.runtime.insert_coroutine(&task_id, coroutine)?.get_task_id().into())
    }

    async fn accept_coroutine(
        qtable: Rc<RefCell<IoQueueTable>>,
        qd: QDesc,
        fd: RawFd,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Borrow the queue table to either update the queue metadata or free the queue on error.
        match Self::do_accept(fd, yielder).await {
            Ok((new_fd, addr)) => {
                let mut queue: CatcollarQueue = CatcollarQueue::new(QType::TcpSocket);
                queue.set_addr(addr);
                queue.set_fd(new_fd);
                let new_qd: QDesc = qtable.borrow_mut().alloc::<CatcollarQueue>(queue);
                (qd, OperationResult::Accept((new_qd, addr)))
            },
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    async fn do_accept(fd: RawFd, yielder: Yielder) -> Result<(RawFd, SocketAddrV4), Fail> {
        // Socket address of accept connection.
        let mut saddr: SockAddr = unsafe { mem::zeroed() };
        let mut address_len: Socklen = mem::size_of::<SockAddrIn>() as u32;

        loop {
            match unsafe { libc::accept(fd, &mut saddr as *mut SockAddr, &mut address_len) } {
                // Operation completed.
                new_fd if new_fd >= 0 => {
                    trace!("connection accepted ({:?})", new_fd);

                    // Set socket options.
                    unsafe {
                        if linux::set_tcp_nodelay(new_fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set TCP_NONDELAY option (errno={:?})", errno);
                        }
                        if linux::set_nonblock(new_fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set O_NONBLOCK option (errno={:?})", errno);
                        }
                        if linux::set_so_reuseport(new_fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set SO_REUSEPORT option (errno={:?})", errno);
                        }
                    }

                    let addr: SocketAddrV4 = linux::sockaddr_to_socketaddrv4(&saddr);
                    break Ok((new_fd, addr));
                },

                // Operation not completed, thus parse errno to find out what happened.
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };

                    // Operation in progress.
                    if DemiRuntime::should_retry(errno) {
                        if let Err(e) = yielder.yield_once().await {
                            let message: String = format!("accept(): operation canceled (err={:?})", e);
                            error!("{}", message);
                            break Err(Fail::new(libc::ECANCELED, &message));
                        }
                    } else {
                        // Operation failed.
                        let message: String = format!("accept(): operation failed (errno={:?})", errno);
                        error!("{}", message);
                        break Err(Fail::new(errno, &message));
                    }
                },
            }
        }
    }

    /// Establishes a connection to a remote endpoint.
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        // Issue connect operation.
        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let remote: SocketAddrV4 = unwrap_socketaddr(remote)?;
        let fd: RawFd = self.get_queue_fd(&qd)?;
        let yielder: Yielder = Yielder::new();
        let coroutine: Pin<Box<Operation>> = Box::pin(Self::connect_coroutine(qd, fd, remote, yielder));
        let task_id: String = format!("Catcollar::connect for qd={:?}", qd);
        Ok(self.runtime.insert_coroutine(&task_id, coroutine)?.get_task_id().into())
    }

    async fn connect_coroutine(
        qd: QDesc,
        fd: RawFd,
        remote: SocketAddrV4,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Handle the result.
        match Self::do_connect(fd, remote, yielder).await {
            Ok(()) => (qd, OperationResult::Connect),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    async fn do_connect(fd: RawFd, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        let saddr: SockAddr = linux::socketaddrv4_to_sockaddr(&remote);
        loop {
            match unsafe { libc::connect(fd, &saddr as *const SockAddr, mem::size_of::<SockAddrIn>() as Socklen) } {
                // Operation completed.
                stats if stats == 0 => {
                    trace!("connection established (fd={:?})", fd);
                    return Ok(());
                },
                // Operation not completed, thus parse errno to find out what happened.
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };

                    // Operation in progress.
                    if DemiRuntime::should_retry(errno) {
                        if let Err(e) = yielder.yield_once().await {
                            let message: String = format!("connect(): operation canceled (err={:?})", e);
                            error!("{}", message);
                            return Err(Fail::new(libc::ECANCELED, &message));
                        }
                    } else {
                        // Operation failed.
                        let message: String = format!("connect(): operation failed (errno={:?})", errno);
                        error!("{}", message);
                        return Err(Fail::new(errno, &message));
                    }
                },
            }
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        let fd: RawFd = self.get_queue_fd(&qd)?;
        match unsafe { libc::close(fd) } {
            stats if stats == 0 => {
                // Expect is safe here because we looked up the queue to schedule this coroutine and no other close
                // coroutine should be able to run due to state machine checks.
                self.qtable
                    .borrow_mut()
                    .free::<CatcollarQueue>(&qd)
                    .expect("queue should exist");
                Ok(())
            },
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                error!("failed to close socket (fd={:?}, errno={:?})", fd, errno);
                Err(Fail::new(errno, "operation failed"))
            },
        }
    }

    /// Asynchronous close
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("close() qd={:?}", qd);
        let fd: RawFd = self.get_queue_fd(&qd)?;
        let yielder: Yielder = Yielder::new();
        let coroutine: Pin<Box<Operation>> = Box::pin(Self::close_coroutine(self.qtable.clone(), qd, fd, yielder));
        let task_id: String = format!("Catcollar::close for qd={:?}", qd);
        Ok(self.runtime.insert_coroutine(&task_id, coroutine)?.get_task_id().into())
    }

    async fn close_coroutine(
        qtable: Rc<RefCell<IoQueueTable>>,
        qd: QDesc,
        fd: RawFd,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Handle the result: Borrow the qtable and free the queue metadata and queue descriptor if the
        // close was successful.
        match Self::do_close(fd, yielder).await {
            Ok(()) => {
                // Expect is safe here because we looked up the queue to schedule this coroutine and no other close
                // coroutine should be able to run due to state machine checks.
                qtable
                    .borrow_mut()
                    .free::<CatcollarQueue>(&qd)
                    .expect("queue shouild exist");
                (qd, OperationResult::Close)
            },
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    async fn do_close(fd: RawFd, yielder: Yielder) -> Result<(), Fail> {
        loop {
            match unsafe { libc::close(fd) } {
                // Operation completed.
                stats if stats == 0 => {
                    trace!("socket closed fd={:?}", fd);
                    return Ok(());
                },
                // Operation not completed, thus parse errno to find out what happened.
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };

                    // Operation was interrupted, retry?
                    if DemiRuntime::should_retry(errno) {
                        if let Err(e) = yielder.yield_once().await {
                            let message: String = format!("close(): operation canceled (err={:?})", e);
                            error!("{}", message);
                            return Err(Fail::new(libc::ECANCELED, &message));
                        }
                    } else {
                        // Operation failed.
                        let message: String = format!("close(): operation failed (errno={:?})", errno);
                        error!("{}", message);
                        return Err(Fail::new(errno, &message));
                    }
                },
            }
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;

        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }

        // Issue push operation.
        let fd: RawFd = self.get_queue_fd(&qd)?;
        // Issue operation.
        let yielder: Yielder = Yielder::new();
        let coroutine: Pin<Box<Operation>> = Box::pin(Self::push_coroutine(self.runtime.clone(), qd, fd, buf, yielder));
        let task_id: String = format!("Catcollar::push for qd={:?}", qd);
        Ok(self.runtime.insert_coroutine(&task_id, coroutine)?.get_task_id().into())
    }

    async fn push_coroutine(
        rt: SharedIoUringRuntime,
        qd: QDesc,
        fd: RawFd,
        buf: DemiBuffer,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        match Self::do_push(rt, fd, buf, yielder).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    async fn do_push(mut rt: SharedIoUringRuntime, fd: RawFd, buf: DemiBuffer, yielder: Yielder) -> Result<(), Fail> {
        let request_id: RequestId = rt.push(fd, buf.clone())?;
        loop {
            match rt.peek(request_id) {
                // Operation completed.
                Ok((_, size)) if size >= 0 => {
                    trace!("data pushed ({:?} bytes)", size);
                    return Ok(());
                },
                // Operation not completed, thus parse errno to find out what happened.
                Ok((None, size)) if size < 0 => {
                    let errno: i32 = -size;
                    // Operation in progress.
                    if DemiRuntime::should_retry(errno) {
                        if let Err(e) = yielder.yield_once().await {
                            let message: String = format!("push(): operation canceled (err={:?})", e);
                            error!("{}", message);
                            return Err(Fail::new(libc::ECANCELED, &message));
                        }
                    } else {
                        let message: String = format!("push(): operation failed (errno={:?})", errno);
                        error!("{}", message);
                        return Err(Fail::new(errno, &message));
                    }
                },
                // Operation failed.
                Err(e) => {
                    let message: String = format!("push(): operation failed (err={:?})", e);
                    error!("{}", message);
                    return Err(e);
                },
                // Should not happen.
                _ => panic!("push failed: unknown error"),
            }
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, remote: SocketAddr) -> Result<QToken, Fail> {
        trace!("pushto() qd={:?}", qd);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let remote: SocketAddrV4 = unwrap_socketaddr(remote)?;

        match self.runtime.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                // Issue push operation.
                let fd: RawFd = self.get_queue_fd(&qd)?;
                // Issue operation.
                let yielder: Yielder = Yielder::new();
                let coroutine: Pin<Box<Operation>> = Box::pin(Self::pushto_coroutine(
                    self.runtime.clone(),
                    qd,
                    fd,
                    remote,
                    buf,
                    yielder,
                ));
                let task_id: String = format!("Catcollar::pushto for qd={:?}", qd);
                Ok(self.runtime.insert_coroutine(&task_id, coroutine)?.get_task_id().into())
            },
            Err(e) => Err(e),
        }
    }

    async fn pushto_coroutine(
        rt: SharedIoUringRuntime,
        qd: QDesc,
        fd: RawFd,
        remote: SocketAddrV4,
        buf: DemiBuffer,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        match Self::do_pushto(rt, fd, remote, buf, yielder).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    async fn do_pushto(
        mut rt: SharedIoUringRuntime,
        fd: RawFd,
        remote: SocketAddrV4,
        buf: DemiBuffer,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        let request_id: RequestId = rt.pushto(fd, remote, buf.clone())?;
        loop {
            match rt.peek(request_id) {
                // Operation completed.
                Ok((_, size)) if size >= 0 => {
                    trace!("data pushed ({:?} bytes)", size);
                    return Ok(());
                },
                // Operation not completed, thus parse errno to find out what happened.
                Ok((None, size)) if size < 0 => {
                    let errno: i32 = -size;
                    // Operation in progress.
                    if DemiRuntime::should_retry(errno) {
                        if let Err(e) = yielder.yield_once().await {
                            let message: String = format!("pushto(): operation canceled (err={:?})", e);
                            error!("{}", message);
                            return Err(Fail::new(libc::ECANCELED, &message));
                        }
                    } else {
                        let message: String = format!("push(): operation failed (errno={:?})", errno);
                        error!("{}", message);
                        return Err(Fail::new(errno, &message));
                    }
                },
                // Operation failed.
                Err(e) => {
                    let message: String = format!("push(): operation failed (err={:?})", e);
                    error!("{}", message);
                    return Err(e);
                },
                // Should not happen.
                _ => panic!("push failed: unknown error"),
            }
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let buf: DemiBuffer = {
            let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);
            DemiBuffer::new(size as u16)
        };

        // Issue pop operation.
        // Issue push operation.
        let fd: RawFd = self.get_queue_fd(&qd)?;
        let yielder: Yielder = Yielder::new();
        let coroutine: Pin<Box<Operation>> = Box::pin(Self::pop_coroutine(self.runtime.clone(), qd, fd, buf, yielder));
        let task_id: String = format!("Catcollar::pop for qd={:?}", qd);
        Ok(self.runtime.insert_coroutine(&task_id, coroutine)?.get_task_id().into())
    }

    async fn pop_coroutine(
        rt: SharedIoUringRuntime,
        qd: QDesc,
        fd: RawFd,
        buf: DemiBuffer,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Handle the result: if successful, return the addr and buffer.
        match Self::do_pop(rt, fd, buf, yielder).await {
            Ok((addr, buf)) => (qd, OperationResult::Pop(addr, buf)),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    async fn do_pop(
        mut rt: SharedIoUringRuntime,
        fd: RawFd,
        buf: DemiBuffer,
        yielder: Yielder,
    ) -> Result<(Option<SocketAddrV4>, DemiBuffer), Fail> {
        let request_id: RequestId = rt.pop(fd, buf.clone())?;
        loop {
            match rt.peek(request_id) {
                // Operation completed.
                Ok((addr, size)) if size >= 0 => {
                    trace!("data received ({:?} bytes)", size);
                    let trim_size: usize = buf.len() - (size as usize);
                    let mut buf: DemiBuffer = buf.clone();
                    buf.trim(trim_size)?;
                    break Ok((addr, buf));
                },
                // Operation not completed, thus parse errno to find out what happened.
                Ok((None, size)) if size < 0 => {
                    let errno: i32 = -size;
                    if DemiRuntime::should_retry(errno) {
                        if let Err(e) = yielder.yield_once().await {
                            let message: String = format!("pop(): operation canceled (err={:?})", e);
                            error!("{}", message);
                            break Err(Fail::new(libc::ECANCELED, &message));
                        }
                    } else {
                        let message: String = format!("pop(): operation failed (errno={:?})", errno);
                        error!("{}", message);
                        break Err(Fail::new(errno, &message));
                    }
                },
                // Operation failed.
                Err(e) => {
                    let message: String = format!("pop(): operation failed (err={:?})", e);
                    error!("{}", message);
                    break Err(e);
                },
                // Should not happen.
                _ => panic!("pop failed: unknown error"),
            }
        }
    }

    pub fn poll(&mut self) {
        self.runtime.scheduler.poll()
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self.runtime.scheduler.from_task_id(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        let (qd, r): (QDesc, OperationResult) = self.take_result(handle);
        Ok(pack_result(&self.runtime, r, qd, qt.into()))
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        trace!("sgalloc() size={:?}", size);
        self.runtime.alloc_sgarray(size)
    }

    /// Frees a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        trace!("sgafree()");
        self.runtime.free_sgarray(sga)
    }

    /// Takes out the operation result descriptor associated with the target scheduler handle.
    fn take_result(&mut self, handle: TaskHandle) -> (QDesc, OperationResult) {
        let task: OperationTask = if let Some(task) = self.runtime.scheduler.remove(&handle) {
            OperationTask::from(task.as_any())
        } else {
            panic!("Removing task that does not exist (either was previously removed or never inserted)");
        };

        task.get_result().expect("The coroutine has not finished")
    }

    fn addr_in_use(&self, local: SocketAddrV4) -> bool {
        for (_, queue) in self.qtable.borrow().get_values() {
            if let Ok(catcollar_queue) = downcast_queue_ptr::<CatcollarQueue>(queue) {
                match catcollar_queue.local() {
                    Some(addr) if addr == local => return true,
                    _ => continue,
                }
            }
        }
        false
    }

    fn get_queue_fd(&self, qd: &QDesc) -> Result<RawFd, Fail> {
        let qtable: Ref<IoQueueTable> = self.qtable.borrow();
        match qtable.get::<CatcollarQueue>(&qd)?.get_fd() {
            Some(fd) => Ok(fd),
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get_cause_fd(): {}", &cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }
}
//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(rt: &SharedIoUringRuntime, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Connect => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_ret: 0,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept((new_qd, addr)) => {
            let saddr: SockAddr = linux::socketaddrv4_to_sockaddr(&addr);
            let qr_value: demi_qr_value_t = demi_qr_value_t {
                ares: demi_accept_result_t {
                    qd: new_qd.into(),
                    addr: saddr,
                },
            };
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_ACCEPT,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_ret: 0,
                qr_value,
            }
        },
        OperationResult::Push => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_ret: 0,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Pop(addr, bytes) => match rt.into_sgarray(bytes) {
            Ok(mut sga) => {
                if let Some(addr) = addr {
                    sga.sga_addr = linux::socketaddrv4_to_sockaddr(&addr);
                }
                let qr_value: demi_qr_value_t = demi_qr_value_t { sga };
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_POP,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_ret: 0,
                    qr_value,
                }
            },
            Err(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_ret: e.errno as i64,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
        },
        OperationResult::Close => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CLOSE,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_ret: 0,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Failed(e) => {
            warn!("Operation Failed: {:?}", e);
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_ret: e.errno as i64,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
