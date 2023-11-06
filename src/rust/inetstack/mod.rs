// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::{
        arp::SharedArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        udp::{
            queue::SharedUdpQueue,
            SharedUdpPeer,
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
            Operation,
            OperationResult,
            OperationTask,
            QDesc,
            QToken,
            QType,
        },
        SharedBox,
        SharedDemiRuntime,
    },
    scheduler::TaskHandle,
};
use ::libc::c_int;
use ::std::{
    net::{
        Ipv4Addr,
        SocketAddr,
        SocketAddrV4,
    },
    pin::Pin,
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
    arp: SharedArpPeer<N>,
    ipv4: Peer<N>,
    runtime: SharedDemiRuntime,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    local_link_addr: MacAddress,
    ts_iters: usize,
}

impl<const N: usize> InetStack<N> {
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        udp_config: UdpConfig,
        tcp_config: TcpConfig,
        rng_seed: [u8; 32],
        arp_config: ArpConfig,
    ) -> Result<Self, Fail> {
        let arp: SharedArpPeer<N> = SharedArpPeer::new(
            runtime.clone(),
            transport.clone(),
            local_link_addr,
            local_ipv4_addr,
            arp_config,
        )?;
        let ipv4: Peer<N> = Peer::new(
            runtime.clone(),
            transport.clone(),
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
            runtime,
            transport,
            local_link_addr,
            ts_iters: 0,
        })
    }

    //======================================================================================================================
    // Associated Functions
    //======================================================================================================================

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
            SOCK_STREAM => self.ipv4.tcp.socket(),
            SOCK_DGRAM => self.ipv4.udp.socket(),
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

        match self.runtime.get_queue_type(&qd)? {
            QType::TcpSocket => self.ipv4.tcp.bind(qd, local),
            QType::UdpSocket => self.ipv4.udp.bind(qd, local),
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
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

        match self.runtime.get_queue_type(&qd)? {
            QType::TcpSocket => self.ipv4.tcp.listen(qd, backlog),
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
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
        match self.runtime.get_queue_type(&qd)? {
            QType::TcpSocket => {
                let coroutine: Pin<Box<Operation>> = self.ipv4.tcp.accept(qd);
                let task_id: String = format!("Inetstack::TCP::accept for qd={:?}", qd);
                let handle: TaskHandle = self.runtime.insert_coroutine(task_id.as_str(), coroutine)?;
                Ok(handle.get_task_id().into())
            },
            // This queue descriptor does not concern a TCP socket.
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
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

        match self.runtime.get_queue_type(&qd)? {
            QType::TcpSocket => {
                let coroutine: Pin<Box<Operation>> = self.ipv4.tcp.connect(qd, remote);
                let task_id: String = format!("Inetstack::TCP::connect for qd={:?}", qd);
                let handle: TaskHandle = self.runtime.insert_coroutine(task_id.as_str(), coroutine)?;
                Ok(handle.get_task_id().into())
            },
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
        }
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

        match self.runtime.get_queue_type(&qd)? {
            QType::TcpSocket => self.ipv4.tcp.close(qd),
            QType::UdpSocket => self.ipv4.udp.close(qd),
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
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

        let (task_id, coroutine): (String, Pin<Box<Operation>>) = match self.runtime.get_queue_type(&qd)? {
            QType::TcpSocket => {
                let task_id: String = format!("Inetstack::TCP::close for qd={:?}", qd);
                let coroutine: Pin<Box<Operation>> = self.ipv4.tcp.async_close(qd);
                (task_id, coroutine)
            },
            QType::UdpSocket => {
                self.ipv4.udp.close(qd)?;
                let task_id: String = format!("Inetstack::UDP::close for qd={:?}", qd);
                let mut runtime: SharedDemiRuntime = self.runtime.clone();
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Expect is safe here because we looked up the queue to schedule this coroutine and no
                    // other close coroutine should be able to run due to state machine checks.
                    runtime
                        .free_queue::<SharedUdpQueue<N>>(&qd)
                        .expect("queue should exist");
                    (qd, OperationResult::Close)
                });
                (task_id, coroutine)
            },
            _ => return Err(Fail::new(libc::EINVAL, "invalid queue type")),
        };

        let handle: TaskHandle = self.runtime.insert_coroutine(task_id.as_str(), coroutine)?;
        let qt: QToken = handle.get_task_id().into();
        trace!("async_close() qt={:?}", qt);
        Ok(qt)
    }

    /// Pushes a buffer to a TCP socket.
    /// TODO: Rename this function to push() once we have a common representation across all libOSes.
    pub fn do_push(&mut self, qd: QDesc, buf: DemiBuffer) -> Result<TaskHandle, Fail> {
        match self.runtime.get_queue_type(&qd)? {
            QType::TcpSocket => {
                let coroutine: Pin<Box<Operation>> = self.ipv4.tcp.push(qd, buf);
                let task_id: String = format!("Inetstack::TCP::push for qd={:?}", qd);
                self.runtime.insert_coroutine(task_id.as_str(), coroutine)
            },
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
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
        let handle: TaskHandle = self.do_push(qd, buf)?;
        let qt: QToken = handle.get_task_id().into();
        trace!("push2() qt={:?}", qt);
        Ok(qt)
    }

    /// Pushes a buffer to a UDP socket.
    /// TODO: Rename this function to pushto() once we have a common buffer representation across all libOSes.
    pub fn do_pushto(&mut self, qd: QDesc, buf: DemiBuffer, to: SocketAddr) -> Result<TaskHandle, Fail> {
        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let to: SocketAddrV4 = unwrap_socketaddr(to)?;

        match self.runtime.get_queue_type(&qd)? {
            QType::UdpSocket => {
                self.ipv4.udp.pushto(qd, buf, to)?;
                let coroutine: Pin<Box<Operation>> = Box::pin(async move { (qd, OperationResult::Push) });
                let task_id: String = format!("Inetstack::UDP::pushto for qd={:?}", qd);
                self.runtime.insert_coroutine(task_id.as_str(), coroutine)
            },
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
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
        // Issue operation.
        let handle: TaskHandle = self.do_pushto(qd, buf, remote)?;
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

        let (task_id, coroutine): (String, Pin<Box<Operation>>) = match self.runtime.get_queue_type(&qd)? {
            QType::TcpSocket => {
                let task_id: String = format!("Inetstack::TCP::pop for qd={:?}", qd);
                let coroutine: Pin<Box<Operation>> = self.ipv4.tcp.pop(qd, size);
                (task_id, coroutine)
            },
            QType::UdpSocket => {
                let task_id: String = format!("Inetstack::UDP::pop for qd={:?}", qd);
                let mut udp: SharedUdpPeer<N> = self.ipv4.udp.clone();
                let coroutine: Pin<Box<Operation>> = Box::pin(async move { udp.pop_coroutine(qd, size).await });
                (task_id, coroutine)
            },
            _ => return Err(Fail::new(libc::EINVAL, "invalid queue type")),
        };

        let handle: TaskHandle = self.runtime.insert_coroutine(task_id.as_str(), coroutine)?;
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
        let handle: TaskHandle = self.runtime.from_task_id(qt.into())?;

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
                let handle: TaskHandle = self.runtime.from_task_id(qt.into())?;

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
        let task: OperationTask = self.runtime.remove_coroutine(&handle);
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
            self.runtime.poll();
        }

        {
            #[cfg(feature = "profiler")]
            timer!("inetstack::poll_bg_work::for");

            for _ in 0..MAX_RECV_ITERS {
                let batch = {
                    #[cfg(feature = "profiler")]
                    timer!("inetstack::poll_bg_work::for::receive");

                    self.transport.receive()
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
                        self.runtime.poll();
                    }
                }
            }
        }

        if self.ts_iters == 0 {
            self.runtime.advance_clock(Instant::now());
        }
        self.ts_iters = (self.ts_iters + 1) % TIMER_RESOLUTION;
    }
}
