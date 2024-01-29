// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::runtime::SharedDummyRuntime;
use ::demikernel::{
    demi_sgarray_t,
    demikernel::libos::network::libos::SharedNetworkLibOS,
    inetstack::SharedInetStack,
    runtime::{
        fail::Fail,
        logging,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::{
            config::{
                ArpConfig,
                TcpConfig,
                UdpConfig,
            },
            types::MacAddress,
        },
        QDesc,
        QToken,
        SharedDemiRuntime,
    },
    OperationResult,
};
use crossbeam_channel::{
    Receiver,
    Sender,
};
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    ops::{
        Deref,
        DerefMut,
    },
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// A default amount of time to wait on an operation to complete. This was chosen arbitrarily to be quite small to make
/// timeouts fast.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1);

pub struct DummyLibOS(SharedNetworkLibOS<SharedInetStack<SharedDummyRuntime>>);

//==============================================================================
// Associated Functons
//==============================================================================

impl DummyLibOS {
    /// Initializes the libOS.
    pub fn new(
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
        tx: Sender<DemiBuffer>,
        rx: Receiver<DemiBuffer>,
        arp: HashMap<Ipv4Addr, MacAddress>,
    ) -> Result<Self, Fail> {
        let runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        let arp_config: ArpConfig = ArpConfig::new(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(1)),
            Some(2),
            Some(arp.clone()),
            Some(false),
        );
        let udp_config: UdpConfig = UdpConfig::default();
        let tcp_config: TcpConfig = TcpConfig::default();
        let network: SharedDummyRuntime = SharedDummyRuntime::new(rx, tx, arp_config, tcp_config, udp_config);

        logging::initialize();
        let transport = SharedInetStack::new_test(runtime.clone(), network, link_addr, ipv4_addr)?;
        Ok(Self(SharedNetworkLibOS::<SharedInetStack<SharedDummyRuntime>>::new(
            runtime, transport,
        )))
    }

    /// Cooks a buffer.
    pub fn cook_data(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        let fill_char: u8 = b'a';

        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
        for a in &mut buf[..] {
            *a = fill_char;
        }
        let data: demi_sgarray_t = self.get_transport().into_sgarray(buf)?;
        Ok(data)
    }

    #[allow(dead_code)]
    pub fn wait(&mut self, qt: QToken, timeout: Option<Duration>) -> Result<(QDesc, OperationResult), Fail> {
        // First check if the task has already completed.
        if let Some(result) = self.get_runtime().get_completed_task(&qt) {
            return Ok(result);
        }

        // Otherwise, actually run the scheduler.
        // Put the QToken into a single element array.
        let qt_array: [QToken; 1] = [qt];
        let start: Instant = Instant::now();

        // Call run_any() until the task finishes.
        while Instant::now() <= start + timeout.unwrap_or(DEFAULT_TIMEOUT) {
            // Run for one quanta and if one of our queue tokens completed, then return.
            if let Some((offset, qd, qr)) = self.get_runtime().run_any(&qt_array) {
                debug_assert_eq!(offset, 0);
                return Ok((qd, qr));
            }
        }

        Err(Fail::new(libc::ETIMEDOUT, "wait timed out"))
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for DummyLibOS {
    type Target = SharedNetworkLibOS<SharedInetStack<SharedDummyRuntime>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DummyLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
