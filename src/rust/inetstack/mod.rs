// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        tcp::{
            operations::{
                AcceptFuture,
                CloseFuture,
                ConnectFuture,
                PopFuture,
                PushFuture,
            },
            queue::TcpQueue,
        },
        udp::{
            queue::UdpQueue,
            UdpPeer,
        },
        Peer,
    },
    pal::constants::{
        AF_INET_VALUE,
        SOCK_DGRAM,
        SOCK_STREAM,
    },
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
        network::{
            config::{
                ArpConfig,
                TcpConfig,
                UdpConfig,
            },
            types::MacAddress,
            unwrap_socketaddr,
            NetworkRuntime,
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
        timer::TimerRc,
    },
    scheduler::{
        Scheduler,
        TaskHandle,
    },
};
use ::libc::c_int;
use ::std::{
    cell::RefCell,
    net::{
        Ipv4Addr,
        SocketAddr,
        SocketAddrV4,
    },
    pin::Pin,
    rc::Rc,
    time::Instant,
};

#[cfg(feature = "profiler")]
use crate::timer;

//==============================================================================
// Exports
//==============================================================================

#[cfg(test)]
pub mod test_helpers;

pub mod collections;
pub mod futures;
pub mod options;
pub mod protocols;

//======================================================================================================================
// Constants
//======================================================================================================================

const TIMER_RESOLUTION: usize = 64;
const MAX_RECV_ITERS: usize = 2;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct InetStack<const N: usize> {
    arp: ArpPeer<N>,
    ipv4: Peer<N>,
    qtable: Rc<RefCell<IoQueueTable>>,
    rt: Rc<dyn NetworkRuntime<N>>,
    local_link_addr: MacAddress,
    scheduler: Scheduler,
    clock: TimerRc,
    ts_iters: usize,
}

impl<const N: usize> InetStack<N> {
    pub fn new(
        rt: Rc<dyn NetworkRuntime<N>>,
        scheduler: Scheduler,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        udp_config: UdpConfig,
        tcp_config: TcpConfig,
        rng_seed: [u8; 32],
        arp_config: ArpConfig,
    ) -> Result<Self, Fail> {
        let qtable: Rc<RefCell<IoQueueTable>> = Rc::new(RefCell::new(IoQueueTable::new()));
        let arp: ArpPeer<N> = ArpPeer::new(
            rt.clone(),
            scheduler.clone(),
            clock.clone(),
            local_link_addr,
            local_ipv4_addr,
            arp_config,
        )?;
        let ipv4: Peer<N> = Peer::new(
            rt.clone(),
            scheduler.clone(),
            qtable.clone(),
            clock.clone(),
            local_link_addr,
            local_ipv4_addr,
            udp_config,
            tcp_config,
            arp.clone(),
            rng_seed,
        )?;
        Ok(Self {
            arp,
            ipv4,
            qtable,
            rt,
            local_link_addr,
            scheduler,
            clock,
            ts_iters: 0,
        })
    }

    //======================================================================================================================
    // Associated Functions
    //======================================================================================================================

    ///
    /// **Brief**
    ///
    /// Looks up queue type based on queue descriptor
    ///
    fn lookup_qtype(&self, &qd: &QDesc) -> Option<QType> {
        match self.qtable.borrow().get_type(&qd) {
            Ok(qtype) => Some(qtype),
            Err(_) => None,
        }
    }

    ///
    /// **Brief**
    ///
    /// Creates an endpoint for communication and returns a file descriptor that
    /// refers to that endpoint. The file descriptor returned by a successful
    /// call will be the lowest numbered file descriptor not currently open for
    /// the process.
    ///
    /// The domain argument specifies a communication domain; this selects the
    /// protocol family which will be used for communication. These families are
    /// defined in the libc crate. Currently, the following families are supported:
    ///
    /// - AF_INET Internet Protocol Version 4 (IPv4)
    ///
    /// **Return Vale**
    ///
    /// Upon successful completion, a file descriptor for the newly created
    /// socket is returned. Upon failure, `Fail` is returned instead.
    ///
    pub fn socket(&mut self, domain: c_int, socket_type: c_int, _protocol: c_int) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::socket");
        trace!(
            "socket(): domain={:?} type={:?} protocol={:?}",
            domain,
            socket_type,
            _protocol
        );
        if domain != AF_INET_VALUE as i32 {
            return Err(Fail::new(libc::ENOTSUP, "address family not supported"));
        }
        match socket_type {
            SOCK_STREAM => self.ipv4.tcp.do_socket(),
            SOCK_DGRAM => self.ipv4.udp.do_socket(),
            _ => Err(Fail::new(libc::ENOTSUP, "socket type not supported")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Binds the socket referred to by `qd` to the local endpoint specified by
    /// `local`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn bind(&mut self, qd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::bind");
        trace!("bind(): qd={:?} local={:?}", qd, local);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let local: SocketAddrV4 = unwrap_socketaddr(local)?;

        match self.lookup_qtype(&qd) {
            Some(QType::TcpSocket) => self.ipv4.tcp.bind(qd, local),
            Some(QType::UdpSocket) => self.ipv4.udp.do_bind(qd, local),
            Some(_) => Err(Fail::new(libc::EINVAL, "invalid queue type")),
            None => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Marks the socket referred to by `qd` as a socket that will be used to
    /// accept incoming connection requests using [accept](Self::accept). The `qd` should
    /// refer to a socket of type `SOCK_STREAM`. The `backlog` argument defines
    /// the maximum length to which the queue of pending connections for `qd`
    /// may grow. If a connection request arrives when the queue is full, the
    /// client may receive an error with an indication that the connection was
    /// refused.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::listen");
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // FIXME: https://github.com/demikernel/demikernel/issues/584
        if backlog == 0 {
            return Err(Fail::new(libc::EINVAL, "invalid backlog length"));
        }

        match self.lookup_qtype(&qd) {
            Some(QType::TcpSocket) => self.ipv4.tcp.listen(qd, backlog),
            Some(_) => Err(Fail::new(libc::EINVAL, "invalid queue type")),
            None => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Accepts an incoming connection request on the queue of pending
    /// connections for the listening socket referred to by `qd`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, a queue token is returned. This token can be
    /// used to wait for a connection request to arrive. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::accept");
        trace!("accept(): {:?}", qd);

        // Search for target queue descriptor.
        match self.lookup_qtype(&qd) {
            Some(QType::TcpSocket) => {
                let (new_qd, future): (QDesc, AcceptFuture<N>) = self.ipv4.tcp.do_accept(qd);
                let qtable_ptr: Rc<RefCell<IoQueueTable>> = self.qtable.clone();
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Wait for accept to complete.
                    let result: Result<(QDesc, SocketAddrV4), Fail> = future.await;
                    // Handle result: If unsuccessful, free the new queue descriptor.
                    match result {
                        Ok((_, addr)) => (qd, OperationResult::Accept((new_qd, addr))),
                        Err(e) => {
                            // It is safe to call expect here because we looked up the queue to schedule this coroutine
                            // and no other accept coroutine should be able to run due to state machine checks.
                            qtable_ptr
                                .borrow_mut()
                                .free::<TcpQueue<N>>(&new_qd)
                                .expect("queue should have been allocated");
                            (qd, OperationResult::Failed(e))
                        },
                    }
                });
                let task_id: String = format!("Inetstack::TCP::accept for qd={:?}", qd);
                let task: OperationTask = OperationTask::new(task_id, coroutine);
                let handle: TaskHandle = match self.scheduler.insert(task) {
                    Some(handle) => handle,
                    None => {
                        return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine"));
                    },
                };
                Ok(handle.get_task_id().into())
            },
            // This queue descriptor does not concern a TCP socket.
            Some(_) => Err(Fail::new(libc::EINVAL, "invalid queue type")),
            // The queue descriptor was not found.
            None => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Connects the socket referred to by `qd` to the remote endpoint specified by `remote`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, a queue token is returned. This token can be
    /// used to push and pop data to/from the queue that connects the local and
    /// remote endpoints. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::connect");
        trace!("connect(): qd={:?} remote={:?}", qd, remote);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let remote: SocketAddrV4 = unwrap_socketaddr(remote)?;

        let task: OperationTask = match self.lookup_qtype(&qd) {
            Some(QType::TcpSocket) => {
                let future: ConnectFuture<N> = self.ipv4.tcp.connect(qd, remote)?;
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Wait for connect to complete.
                    let result: Result<(), Fail> = future.await;
                    // Handle result.
                    match result {
                        Ok(()) => (qd, OperationResult::Connect),
                        Err(e) => (qd, OperationResult::Failed(e)),
                    }
                });
                let task_id: String = format!("Inetstack::TCP::connect for qd={:?}", qd);
                OperationTask::new(task_id, coroutine)
            },
            Some(_) => return Err(Fail::new(libc::EINVAL, "invalid queue type")),
            None => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };

        let handle: TaskHandle = match self.scheduler.insert(task) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.get_task_id().into();
        trace!("connect() qt={:?}", qt);
        Ok(qt)
    }

    ///
    /// **Brief**
    ///
    /// Closes a connection referred to by `qd`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::close");
        trace!("close(): qd={:?}", qd);

        match self.lookup_qtype(&qd) {
            Some(QType::TcpSocket) => self.ipv4.tcp.do_close(qd),
            Some(QType::UdpSocket) => self.ipv4.udp.do_close(qd),
            Some(_) => Err(Fail::new(libc::EINVAL, "invalid queue type")),
            None => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Asynchronously closes a connection referred to by `qd`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. This qtoken can be used to wait until the close
    /// completes shutting down the connection. Upon failure, `Fail` is returned instead.
    ///
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::async_close");
        trace!("async_close(): qd={:?}", qd);

        let qtable_ptr: Rc<RefCell<IoQueueTable>> = self.qtable.clone();
        let (task_id, coroutine): (String, Pin<Box<Operation>>) = match self.lookup_qtype(&qd) {
            Some(QType::TcpSocket) => {
                let future: CloseFuture<N> = self.ipv4.tcp.do_async_close(qd)?;
                let task_id: String = format!("Inetstack::TCP::close for qd={:?}", qd);
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    let result: Result<(), Fail> = future.await;
                    match result {
                        Ok(()) => {
                            // Expect is safe here because we looked up the queue to schedule this coroutine and no
                            // other close coroutine should be able to run due to state machine checks.
                            qtable_ptr
                                .borrow_mut()
                                .free::<TcpQueue<N>>(&qd)
                                .expect("queue should exist");
                            (qd, OperationResult::Close)
                        },
                        Err(e) => (qd, OperationResult::Failed(e)),
                    }
                });
                (task_id, coroutine)
            },
            Some(QType::UdpSocket) => {
                self.ipv4.udp.do_close(qd)?;
                let task_id: String = format!("Inetstack::TCP::close for qd={:?}", qd);
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Expect is safe here because we looked up the queue to schedule this coroutine and no
                    // other close coroutine should be able to run due to state machine checks.
                    qtable_ptr
                        .borrow_mut()
                        .free::<UdpQueue>(&qd)
                        .expect("queue should exist");
                    (qd, OperationResult::Close)
                });
                (task_id, coroutine)
            },
            Some(_) => return Err(Fail::new(libc::EINVAL, "invalid queue type")),
            None => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };

        let handle: TaskHandle = match self.scheduler.insert(OperationTask::new(task_id, coroutine)) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.get_task_id().into();
        trace!("async_close() qt={:?}", qt);
        Ok(qt)
    }

    /// Pushes a buffer to a TCP socket.
    /// TODO: Rename this function to push() once we have a common representation across all libOSes.
    pub fn do_push(&mut self, qd: QDesc, buf: DemiBuffer) -> Result<OperationTask, Fail> {
        match self.lookup_qtype(&qd) {
            Some(QType::TcpSocket) => {
                let future: PushFuture = self.ipv4.tcp.push(qd, buf);
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Wait for push to complete.
                    let result: Result<(), Fail> = future.await;
                    // Handle result.
                    match result {
                        Ok(()) => (qd, OperationResult::Push),
                        Err(e) => (qd, OperationResult::Failed(e)),
                    }
                });
                let task_id: String = format!("Inetstack::TCP::push for qd={:?}", qd);
                Ok(OperationTask::new(task_id, coroutine))
            },
            Some(_) => Err(Fail::new(libc::EINVAL, "invalid queue type")),
            None => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    /// Pushes raw data to a TCP socket.
    /// TODO: Move this function to demikernel repo once we have a common buffer representation across all libOSes.
    pub fn push2(&mut self, qd: QDesc, data: &[u8]) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::push2");
        trace!("push2(): qd={:?}", qd);

        // Convert raw data to a buffer representation.
        let buf: DemiBuffer = DemiBuffer::from_slice(data)?;
        if buf.is_empty() {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }

        // Issue operation.
        let task: OperationTask = self.do_push(qd, buf)?;
        let handle: TaskHandle = match self.scheduler.insert(task) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.get_task_id().into();
        trace!("push2() qt={:?}", qt);
        Ok(qt)
    }

    /// Pushes a buffer to a UDP socket.
    /// TODO: Rename this function to pushto() once we have a common buffer representation across all libOSes.
    pub fn do_pushto(&mut self, qd: QDesc, buf: DemiBuffer, to: SocketAddr) -> Result<OperationTask, Fail> {
        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let to: SocketAddrV4 = unwrap_socketaddr(to)?;

        match self.lookup_qtype(&qd) {
            Some(QType::UdpSocket) => {
                self.ipv4.udp.do_pushto(qd, buf, to)?;
                let coroutine: Pin<Box<Operation>> = Box::pin(async move { (qd, OperationResult::Push) });
                let task_id: String = format!("Inetstack::UDP::pushto for qd={:?}", qd);
                Ok(OperationTask::new(task_id, coroutine))
            },
            Some(_) => Err(Fail::new(libc::EINVAL, "invalid queue type")),
            None => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    /// Pushes raw data to a UDP socket.
    /// TODO: Move this function to demikernel repo once we have a common buffer representation across all libOSes.
    pub fn pushto2(&mut self, qd: QDesc, data: &[u8], remote: SocketAddr) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::pushto2");
        trace!("pushto2(): qd={:?}", qd);

        // Convert raw data to a buffer representation.
        let buf: DemiBuffer = DemiBuffer::from_slice(data)?;
        if buf.is_empty() {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }
        let task: OperationTask = self.do_pushto(qd, buf, remote)?;
        // Issue operation.
        let handle: TaskHandle = match self.scheduler.insert(task) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.get_task_id().into();
        trace!("pushto2() qt={:?}", qt);
        Ok(qt)
    }

    /// Create a pop request to write data from IO connection represented by `qd` into a buffer
    /// allocated by the application.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::pop");

        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let (task_id, coroutine): (String, Pin<Box<Operation>>) = match self.lookup_qtype(&qd) {
            Some(QType::TcpSocket) => {
                let task_id: String = format!("Inetstack::TCP::pop for qd={:?}", qd);
                let future: PopFuture<N> = self.ipv4.tcp.pop(qd, size);
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Wait for pop to complete.
                    let result: Result<DemiBuffer, Fail> = future.await;
                    // Handle result.
                    match result {
                        Ok(buf) => (qd, OperationResult::Pop(None, buf)),
                        Err(e) => (qd, OperationResult::Failed(e)),
                    }
                });
                (task_id, coroutine)
            },
            Some(QType::UdpSocket) => {
                let task_id: String = format!("Inetstack::UDP::pop for qd={:?}", qd);
                let coroutine: Pin<Box<Operation>> = Box::pin(UdpPeer::<N>::do_pop(self.qtable.clone(), qd, size));
                (task_id, coroutine)
            },
            Some(_) => return Err(Fail::new(libc::EINVAL, "invalid queue type")),
            None => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };

        let handle: TaskHandle = match self.scheduler.insert(OperationTask::new(task_id, coroutine)) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.get_task_id().into();
        trace!("pop() qt={:?}", qt);
        Ok(qt)
    }

    /// Waits for an operation to complete.
    /// This function is deprecated, do not use.
    /// FIXME: https://github.com/microsoft/demikernel/issues/889
    pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::wait2");
        trace!("wait2(): qt={:?}", qt);

        // Retrieve associated schedule handle.
        let handle: TaskHandle = match self.scheduler.from_task_id(qt.into()) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        };

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll_bg_work();

            // The operation has completed, so extract the result and return.
            if handle.has_completed() {
                trace!("wait2() qt={:?} completed!", qt);
                return Ok(self.take_operation(handle));
            }
        }
    }

    /// Waits for any operation to complete.
    /// This function is deprecated, do not use.
    /// FIXME: https://github.com/microsoft/demikernel/issues/890
    pub fn wait_any2(&mut self, qts: &[QToken]) -> Result<(usize, QDesc, OperationResult), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::wait_any2");
        trace!("wait_any2(): qts={:?}", qts);

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll_bg_work();

            // Search for any operation that has completed.
            for (i, &qt) in qts.iter().enumerate() {
                // Retrieve associated schedule handle.
                // TODO: move this out of the loop.
                let handle: TaskHandle = match self.scheduler.from_task_id(qt.into()) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
                };

                // Found one, so extract the result and return.
                if handle.has_completed() {
                    let (qd, r): (QDesc, OperationResult) = self.take_operation(handle);
                    return Ok((i, qd, r));
                }
            }
        }
    }

    /// Given a handle representing a task in our scheduler. Return the results of this future
    /// and the file descriptor for this connection.
    ///
    /// This function will panic if the specified future had not completed or is _background_ future.
    pub fn take_operation(&mut self, handle: TaskHandle) -> (QDesc, OperationResult) {
        let task: OperationTask = if let Some(task) = self.scheduler.remove(&handle) {
            OperationTask::from(task.as_any())
        } else {
            panic!("Removing task that does not exist (either was previously removed or never inserted)");
        };

        task.get_result().expect("Coroutine not finished")
    }

    /// New incoming data has arrived. Route it to the correct parse out the Ethernet header and
    /// allow the correct protocol to handle it. The underlying protocol will futher parse the data
    /// and inform the correct task that its data has arrived.
    fn do_receive(&mut self, bytes: DemiBuffer) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::engine::receive");
        let (header, payload) = Ethernet2Header::parse(bytes)?;
        debug!("Engine received {:?}", header);
        if self.local_link_addr != header.dst_addr()
            && !header.dst_addr().is_broadcast()
            && !header.dst_addr().is_multicast()
        {
            return Err(Fail::new(libc::EINVAL, "physical destination address mismatch"));
        }
        match header.ether_type() {
            EtherType2::Arp => self.arp.receive(payload),
            EtherType2::Ipv4 => self.ipv4.receive(payload),
            EtherType2::Ipv6 => Ok(()), // Ignore for now.
        }
    }

    /// Scheduler will poll all futures that are ready to make progress.
    /// Then ask the runtime to receive new data which we will forward to the engine to parse and
    /// route to the correct protocol.
    pub fn poll_bg_work(&mut self) {
        #[cfg(feature = "profiler")]
        timer!("inetstack::poll_bg_work");
        {
            #[cfg(feature = "profiler")]
            timer!("inetstack::poll_bg_work::poll");
            self.scheduler.poll();
        }

        {
            #[cfg(feature = "profiler")]
            timer!("inetstack::poll_bg_work::for");

            for _ in 0..MAX_RECV_ITERS {
                let batch = {
                    #[cfg(feature = "profiler")]
                    timer!("inetstack::poll_bg_work::for::receive");

                    self.rt.receive()
                };

                {
                    #[cfg(feature = "profiler")]
                    timer!("inetstack::poll_bg_work::for::for");

                    if batch.is_empty() {
                        break;
                    }

                    for pkt in batch {
                        if let Err(e) = self.do_receive(pkt) {
                            warn!("Dropped packet: {:?}", e);
                        }
                        // TODO: This is a workaround for https://github.com/demikernel/inetstack/issues/149.
                        self.scheduler.poll();
                    }
                }
            }
        }

        if self.ts_iters == 0 {
            self.clock.advance_clock(Instant::now());
        }
        self.ts_iters = (self.ts_iters + 1) % TIMER_RESOLUTION;
    }
}
