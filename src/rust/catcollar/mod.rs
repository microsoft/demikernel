// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;
mod iouring;
mod queue;
mod runtime;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{
    queue::CatcollarQueue,
    runtime::IoUringRuntime,
};

//======================================================================================================================
// Imports
//======================================================================================================================

use self::futures::{
    accept::AcceptFuture,
    close::CloseFuture,
    connect::ConnectFuture,
    pop::PopFuture,
    push::PushFuture,
    pushto::PushtoFuture,
};
use crate::{
    demikernel::config::Config,
    pal::linux,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        queue::{
            IoQueueTable,
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
    },
    scheduler::SchedulerHandle,
};
use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    mem,
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
    pin::Pin,
    rc::Rc,
};

//======================================================================================================================
// Constants
//======================================================================================================================

// Size of receive buffers.
const CATCOLLAR_RECVBUF_SIZE: usize = 9000;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Catcollar LibOS
pub struct CatcollarLibOS {
    /// Table of queue descriptors.
    qtable: Rc<RefCell<IoQueueTable<CatcollarQueue>>>, // TODO: Move this into runtime module.
    /// Underlying runtime.
    runtime: IoUringRuntime,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associate Functions for Catcollar LibOS
impl CatcollarLibOS {
    /// Instantiates a Catcollar LibOS.
    pub fn new(_config: &Config) -> Self {
        let qtable: Rc<RefCell<IoQueueTable<CatcollarQueue>>> =
            Rc::new(RefCell::new(IoQueueTable::<CatcollarQueue>::new()));
        let runtime: IoUringRuntime = IoUringRuntime::new();
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
                Ok(self.qtable.borrow_mut().alloc(queue))
            },
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                Err(Fail::new(errno, "failed to create socket"))
            },
        }
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);
        let mut qtable: RefMut<IoQueueTable<CatcollarQueue>> = self.qtable.borrow_mut();
        // Check if we are binding to the wildcard port.
        if local.port() == 0 {
            let cause: String = format!("cannot bind to port 0 (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Check if queue descriptor is valid.
        if qtable.get(&qd).is_none() {
            let cause: String = format!("invalid queue descriptor {:?}", qd);
            error!("bind(): {}", &cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }

        // Check wether the address is in use.
        for (_, queue) in qtable.get_values() {
            if let Some(addr) = queue.get_addr() {
                if addr == local {
                    let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
                    error!("bind(): {}", &cause);
                    return Err(Fail::new(libc::EADDRINUSE, &cause));
                }
            }
        }

        // Get a mutable reference to the queue table.
        // It is safe to unwrap because we checked before that the queue descriptor is valid.
        let queue: &mut CatcollarQueue = qtable.get_mut(&qd).expect("queue descriptor should be in queue table");

        // Get reference to the underlying file descriptor.
        // It is safe to unwrap because when creating a queue we assigned it a valid file descritor.
        let fd: RawFd = queue.get_fd().expect("queue should have a file descriptor");

        // Bind underlying socket.
        let sockaddr: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&local);
        match unsafe {
            libc::bind(
                fd,
                (&sockaddr as *const libc::sockaddr_in) as *const libc::sockaddr,
                mem::size_of_val(&sockaddr) as u32,
            )
        } {
            stats if stats == 0 => {
                queue.set_addr(local);
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

        // Issue listen operation.
        match self.qtable.borrow().get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    if unsafe { libc::listen(fd, backlog as i32) } != 0 {
                        let errno: libc::c_int = unsafe { *libc::__errno_location() };
                        error!("failed to listen ({:?})", errno);
                        return Err(Fail::new(errno, "operation failed"));
                    }
                    Ok(())
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);
        let mut qtable: RefMut<IoQueueTable<CatcollarQueue>> = self.qtable.borrow_mut();

        let fd: RawFd = match qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => fd,
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        // Issue accept operation.
        let new_qd: QDesc = qtable.alloc(CatcollarQueue::new(QType::TcpSocket));
        let future: AcceptFuture = AcceptFuture::new(fd);
        let qtable_ptr: Rc<RefCell<IoQueueTable<CatcollarQueue>>> = self.qtable.clone();
        let coroutine: Pin<Box<Operation>> = Box::pin(async move {
            // Wait for the accept routine to complete.
            let result: Result<(RawFd, SocketAddrV4), Fail> = future.await;
            // Borrow the queue table to either update the queue metadata or free the queue on error.
            let mut qtable_: RefMut<IoQueueTable<CatcollarQueue>> = qtable_ptr.borrow_mut();
            match result {
                Ok((new_fd, addr)) => {
                    let queue: &mut CatcollarQueue = qtable_
                        .get_mut(&new_qd)
                        .expect("New qd should have been already allocated");
                    queue.set_addr(addr);
                    queue.set_fd(new_fd);
                    (qd, OperationResult::Accept((new_qd, addr)))
                },
                Err(e) => {
                    qtable_.free(&new_qd);
                    (qd, OperationResult::Failed(e))
                },
            }
        });
        let task_id: String = format!("Catcollar::accept for qd={:?}", qd);
        let task: OperationTask = OperationTask::new(task_id, coroutine);
        let handle: SchedulerHandle = match self.runtime.scheduler.insert(task) {
            Some(handle) => handle,
            None => {
                qtable.free(&new_qd);
                return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine"));
            },
        };
        Ok(handle.into_raw().into())
    }

    /// Establishes a connection to a remote endpoint.
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        // Issue connect operation.
        match self.qtable.borrow().get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let future: ConnectFuture = ConnectFuture::new(fd, remote);
                    let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                        // Wait for connect to finish.
                        let result: Result<(), Fail> = future.await;
                        // Handle the result.
                        match result {
                            Ok(()) => (qd, OperationResult::Connect),
                            Err(e) => (qd, OperationResult::Failed(e)),
                        }
                    });
                    let task_id: String = format!("Catcollar::connect for qd={:?}", qd);
                    let task: OperationTask = OperationTask::new(task_id, coroutine);
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(task) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    Ok(handle.into_raw().into())
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        let mut qtable: RefMut<IoQueueTable<CatcollarQueue>> = self.qtable.borrow_mut();
        match qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => match unsafe { libc::close(fd) } {
                    stats if stats == 0 => (),
                    _ => {
                        let errno: libc::c_int = unsafe { *libc::__errno_location() };
                        error!("failed to close socket (fd={:?}, errno={:?})", fd, errno);
                        return Err(Fail::new(errno, "operation failed"));
                    },
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };
        qtable.free(&qd);
        Ok(())
    }

    /// Asynchronous close
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("close() qd={:?}", qd);

        match self.qtable.borrow().get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let future: CloseFuture = CloseFuture::new(fd);
                    let qtable_ptr: Rc<RefCell<IoQueueTable<CatcollarQueue>>> = self.qtable.clone();
                    let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                        // Wait for close to complete.
                        let result: Result<(), Fail> = future.await;
                        // Handle the result: Borrow the qtable and free the queue metadata and queue descriptor if the
                        // close was successful.
                        match result {
                            Ok(()) => {
                                let mut qtable_ = qtable_ptr.borrow_mut();
                                qtable_.free(&qd);
                                (qd, OperationResult::Close)
                            },
                            Err(e) => (qd, OperationResult::Failed(e)),
                        }
                    });
                    let task_id: String = format!("Catcollar::close for qd={:?}", qd);
                    let task: OperationTask = OperationTask::new(task_id, coroutine);
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(task) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    Ok(handle.into_raw().into())
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
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
        match self.qtable.borrow().get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    // Issue operation.
                    let future: PushFuture = PushFuture::new(self.runtime.clone(), fd, buf);
                    let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                        // Wait for the push to complete
                        let result: Result<(), Fail> = future.await;
                        // Handle the result.
                        match result {
                            Ok(()) => (qd, OperationResult::Push),
                            Err(e) => (qd, OperationResult::Failed(e)),
                        }
                    });
                    let task_id: String = format!("Catcollar::push for qd={:?}", qd);
                    let task: OperationTask = OperationTask::new(task_id, coroutine);
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(task) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    Ok(handle.into_raw().into())
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("pushto() qd={:?}", qd);

        match self.runtime.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                // Issue pushto operation.
                match self.qtable.borrow().get(&qd) {
                    Some(queue) => match queue.get_fd() {
                        Some(fd) => {
                            // Issue operation.
                            let future: PushtoFuture = PushtoFuture::new(self.runtime.clone(), fd, remote, buf);
                            let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                                // Wait for pushto to complete.
                                let result: Result<(), Fail> = future.await;
                                // Handle result.
                                match result {
                                    Ok(()) => (qd, OperationResult::Push),
                                    Err(e) => (qd, OperationResult::Failed(e)),
                                }
                            });
                            let task_id: String = format!("Catcollar::pushto for qd={:?}", qd);
                            let task: OperationTask = OperationTask::new(task_id, coroutine);
                            let handle: SchedulerHandle = match self.runtime.scheduler.insert(task) {
                                Some(handle) => handle,
                                None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                            };
                            Ok(handle.into_raw().into())
                        },
                        None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
                    },
                    None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // Check if the pop size is valid.
        if size.is_some() && size.unwrap() == 0 {
            let cause: String = format!("invalid pop size (size={:?})", size);
            error!("pop(): {:?}", &cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        let buf: DemiBuffer = {
            let size: usize = size.unwrap_or(CATCOLLAR_RECVBUF_SIZE as usize);
            DemiBuffer::new(size as u16)
        };

        // Issue pop operation.
        match self.qtable.borrow().get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let future: PopFuture = PopFuture::new(self.runtime.clone(), fd, buf);
                    let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                        // Wait for pop to complete.
                        let result: Result<(Option<SocketAddrV4>, DemiBuffer), Fail> = future.await;
                        // Handle the result: if successful, return the addr and buffer.
                        match result {
                            Ok((addr, buf)) => (qd, OperationResult::Pop(addr, buf)),
                            Err(e) => (qd, OperationResult::Failed(e)),
                        }
                    });
                    let task_id: String = format!("Catcollar::pop for qd={:?}", qd);
                    let task: OperationTask = OperationTask::new(task_id, coroutine);
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(task) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    let qt: QToken = handle.into_raw().into();
                    Ok(qt)
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    pub fn poll(&self) {
        self.runtime.scheduler.poll()
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<SchedulerHandle, Fail> {
        match self.runtime.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&mut self, handle: SchedulerHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
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
    fn take_result(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let task: OperationTask = OperationTask::from(self.runtime.scheduler.take(handle).as_any());
        task.get_result().expect("The coroutine has not finished")
    }
}
//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(rt: &IoUringRuntime, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Connect => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_ret: 0,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept((new_qd, addr)) => {
            let saddr: libc::sockaddr = {
                let sin: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&addr);
                unsafe { mem::transmute::<libc::sockaddr_in, libc::sockaddr>(sin) }
            };
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
                if let Some(endpoint) = addr {
                    let saddr: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&endpoint);
                    sga.sga_addr = unsafe { mem::transmute::<libc::sockaddr_in, libc::sockaddr>(saddr) };
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
                    qr_ret: e.errno,
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
                qr_ret: e.errno,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
