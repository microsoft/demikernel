// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    demi_sgarray_t,
    demikernel::libos::network::libos::SharedNetworkLibOS,
    inetstack::{
        test_helpers::SharedTestRuntime,
        SharedInetStack,
    },
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::types::MacAddress,
        OperationResult,
        QDesc,
        QToken,
        SharedDemiRuntime,
    },
};
use ::socket2::{
    Domain,
    Protocol,
    Type,
};
use ::std::{
    collections::{
        HashMap,
        VecDeque,
    },
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
    time::{
        Duration,
        Instant,
    },
};

/// A default amount of time to wait on an operation to complete. This was chosen arbitrarily.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

pub struct SharedEngine(SharedNetworkLibOS<SharedInetStack<SharedTestRuntime>>);

impl SharedEngine {
    pub fn new(test_rig: SharedTestRuntime, now: Instant) -> Result<Self, Fail> {
        // Instantiate all of the layers.
        // Shared Demikernel runtime.
        let runtime: SharedDemiRuntime = SharedDemiRuntime::new(now);
        let transport: SharedInetStack<SharedTestRuntime> = SharedInetStack::new_test(
            runtime.clone(),
            test_rig.clone(),
            test_rig.get_link_addr(),
            test_rig.get_ip_addr(),
        )?;

        Ok(Self(SharedNetworkLibOS::<SharedInetStack<SharedTestRuntime>>::new(
            runtime, transport,
        )))
    }

    pub fn pop_frame(&mut self) -> DemiBuffer {
        self.get_transport().get_network().pop_frame()
    }

    pub fn pop_all_frames(&mut self) -> VecDeque<DemiBuffer> {
        self.get_transport().get_network().pop_all_frames()
    }

    pub fn advance_clock(&mut self, now: Instant) {
        self.get_runtime().advance_clock(now)
    }

    pub fn receive(&mut self, bytes: DemiBuffer) -> Result<(), Fail> {
        // We no longer do processing in this function, so we will not know if the packet is dropped or not.
        self.get_transport().receive(bytes)?;
        // So poll the scheduler to do the processing.
        self.get_runtime().poll();
        self.get_runtime().poll();

        Ok(())
    }

    pub async fn ipv4_ping(&mut self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.get_transport().ping(dest_ipv4_addr, timeout).await
    }

    pub fn udp_pushto(&mut self, qd: QDesc, buf: DemiBuffer, to: SocketAddrV4) -> Result<QToken, Fail> {
        let data: demi_sgarray_t = self.get_transport().into_sgarray(buf)?;
        self.pushto(qd, &data, to.into())
    }

    pub fn udp_pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        self.pop(qd, None)
    }

    pub fn udp_socket(&mut self) -> Result<QDesc, Fail> {
        self.socket(Domain::IPV4, Type::DGRAM, Protocol::UDP)
    }

    pub fn udp_bind(&mut self, socket_fd: QDesc, endpoint: SocketAddrV4) -> Result<(), Fail> {
        self.bind(socket_fd, endpoint.into())
    }

    pub fn udp_close(&mut self, socket_fd: QDesc) -> Result<(), Fail> {
        let qt = self.async_close(socket_fd)?;
        match self.wait(qt)? {
            (_, OperationResult::Close) => Ok(()),
            _ => unreachable!("close did not succeed"),
        }
    }

    pub fn tcp_socket(&mut self) -> Result<QDesc, Fail> {
        self.socket(Domain::IPV4, Type::STREAM, Protocol::TCP)
    }

    pub fn tcp_connect(&mut self, socket_fd: QDesc, remote_endpoint: SocketAddrV4) -> Result<QToken, Fail> {
        self.connect(socket_fd, remote_endpoint.into())
    }

    pub fn tcp_bind(&mut self, socket_fd: QDesc, endpoint: SocketAddrV4) -> Result<(), Fail> {
        self.bind(socket_fd, endpoint.into())
    }

    pub fn tcp_accept(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        self.accept(fd)
    }

    pub fn tcp_push(&mut self, socket_fd: QDesc, buf: DemiBuffer) -> Result<QToken, Fail> {
        let data: demi_sgarray_t = self.get_transport().into_sgarray(buf)?;
        self.push(socket_fd, &data)
    }

    pub fn tcp_pop(&mut self, socket_fd: QDesc) -> Result<QToken, Fail> {
        self.pop(socket_fd, None)
    }

    pub fn tcp_async_close(&mut self, socket_fd: QDesc) -> Result<QToken, Fail> {
        self.async_close(socket_fd)
    }

    pub fn tcp_listen(&mut self, socket_fd: QDesc, backlog: usize) -> Result<(), Fail> {
        self.listen(socket_fd, backlog)
    }

    pub async fn arp_query(&mut self, ipv4_addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        self.get_transport().arp_query(ipv4_addr).await
    }

    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.get_transport().export_arp_cache()
    }

    pub fn poll(&self) {
        self.get_runtime().poll()
    }

    pub fn wait(&self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
        // First check if the task has already completed.
        if let Some(result) = self.get_runtime().get_completed_task(&qt) {
            return Ok(result);
        }

        // Otherwise, run the scheduler.
        // Put the QToken into a single element array.
        let qt_array: [QToken; 1] = [qt];
        let start: Instant = Instant::now();

        // Call run_any() until the task finishes.
        while Instant::now() <= start + DEFAULT_TIMEOUT {
            // Run for one quanta and if one of our queue tokens completed, then return.
            if let Some((offset, qd, qr)) = self.get_runtime().run_any(&qt_array) {
                debug_assert_eq!(offset, 0);
                return Ok((qd, qr));
            }
        }

        Err(Fail::new(libc::ETIMEDOUT, "wait timed out"))
    }
}

impl Deref for SharedEngine {
    type Target = SharedNetworkLibOS<SharedInetStack<SharedTestRuntime>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SharedEngine {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
