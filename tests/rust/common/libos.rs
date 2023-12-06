// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::runtime::SharedDummyRuntime;
use ::demikernel::{
    inetstack::SharedInetStack,
    runtime::{
        fail::Fail,
        logging,
        memory::DemiBuffer,
        network::{
            config::{
                ArpConfig,
                TcpConfig,
                UdpConfig,
            },
            types::MacAddress,
            NetworkRuntime,
        },
        SharedBox,
        SharedDemiRuntime,
    },
};
use crossbeam_channel::{
    Receiver,
    Sender,
};
use demikernel::runtime::network::consts::RECEIVE_BATCH_SIZE;
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    time::Duration,
};

//==============================================================================
// Structures
//==============================================================================

pub struct DummyLibOS {}

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
    ) -> Result<SharedInetStack<RECEIVE_BATCH_SIZE>, Fail> {
        let runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        let transport: SharedDummyRuntime = SharedDummyRuntime::new(rx, tx);
        let arp_config: ArpConfig = ArpConfig::new(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(1)),
            Some(2),
            Some(arp.clone()),
            Some(false),
        );
        let udp_config: UdpConfig = UdpConfig::default();
        let tcp_config: TcpConfig = TcpConfig::default();
        let rng_seed: [u8; 32] = [0; 32];
        logging::initialize();
        SharedInetStack::new(
            runtime,
            SharedBox::<dyn NetworkRuntime<RECEIVE_BATCH_SIZE>>::new(Box::new(transport)),
            link_addr,
            ipv4_addr,
            udp_config,
            tcp_config,
            rng_seed,
            arp_config,
        )
    }

    /// Cooks a buffer.
    pub fn cook_data(size: usize) -> DemiBuffer {
        let fill_char: u8 = b'a';

        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
        for a in &mut buf[..] {
            *a = fill_char;
        }
        buf
    }
}
