// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::{num::NonZeroU32, rc::Rc};

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{RuleSet, RxRing, TxRing},
        socket::XdpSocket,
    },
    demikernel::config::Config,
    runtime::fail::Fail,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// State for the XDP interface.
pub struct Interface {
    /// Currently only one TX is created for all sends on the interface.
    pub tx_ring: TxRing,
    /// RX rings for the interface, one per RSS queue.
    pub rx_rings: Vec<RxRing>,
    /// Sockets for the interface, one for each *Ring member above, with a description of the socket.
    pub sockets: Vec<(String, XdpSocket)>,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl Interface {
    /// Creates a new interface for the given configuration. The interface creates [queue_count] RX
    /// rings.
    pub fn new(
        api: &mut XdpApi,
        ifindex: u32,
        queue_count: NonZeroU32,
        ruleset: Rc<RuleSet>,
        config: &Config,
    ) -> Result<Self, Fail> {
        let (tx_buffer_count, tx_ring_size) = config.tx_buffer_config()?;
        let (rx_buffer_count, rx_ring_size) = config.rx_buffer_config()?;
        let mtu: u16 = config.mtu()?;

        let (tx_ring_size, tx_buffer_count): (NonZeroU32, NonZeroU32) =
            validate_ring_config(tx_ring_size, tx_buffer_count, "tx")?;
        let (rx_ring_size, rx_buffer_count): (NonZeroU32, NonZeroU32) =
            validate_ring_config(rx_ring_size, rx_buffer_count, "rx")?;

        let always_poke: bool = config.xdp_always_poke_tx()?;

        let mut rx_rings: Vec<RxRing> = Vec::with_capacity(queue_count.get() as usize);
        let mut sockets: Vec<(String, XdpSocket)> = Vec::new();

        let tx_ring: TxRing = TxRing::new(
            api,
            tx_ring_size.get(),
            tx_buffer_count.get(),
            mtu,
            ifindex,
            0,
            always_poke,
        )?;
        sockets.push((format!("if {} tx 0", ifindex), tx_ring.socket().clone()));

        for queueid in 0..queue_count.get() {
            let mut ring: RxRing = RxRing::new(
                api,
                rx_ring_size.get(),
                rx_buffer_count.get(),
                mtu,
                ifindex,
                queueid,
                ruleset.clone(),
            )?;
            ring.provide_buffers();
            sockets.push((format!("if {} rx {}", ifindex, queueid), ring.socket().clone()));
            rx_rings.push(ring);
        }

        trace!("Created {} RX rings on interface {}", rx_rings.len(), ifindex);

        Ok(Self {
            tx_ring,
            rx_rings,
            sockets,
        })
    }

    pub fn return_tx_buffers(&mut self) {
        self.tx_ring.return_buffers();
    }

    pub fn provide_rx_buffers(&mut self) {
        for ring in self.rx_rings.iter_mut() {
            ring.provide_buffers();
        }
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

/// Validates the ring size and buffer count for the given configuration.
fn validate_ring_config(ring_size: u32, buf_count: u32, config: &str) -> Result<(NonZeroU32, NonZeroU32), Fail> {
    let ring_size: NonZeroU32 = NonZeroU32::try_from(ring_size)
        .map_err(Fail::from)
        .and_then(|v: NonZeroU32| {
            if !v.is_power_of_two() {
                let cause: String = format!("{}_ring_size must be a power of two: {}", config, v.get());
                Err(Fail::new(libc::EINVAL, &cause))
            } else {
                Ok(v)
            }
        })?;

    let buf_count: NonZeroU32 = if buf_count < ring_size.get() {
        let cause: String = format!(
            "{}_buffer_count must be greater than or equal to {}_ring_size",
            config, config
        );
        return Err(Fail::new(libc::EINVAL, &cause));
    } else {
        // Safety: since buffer_count >= ring_size, we can safely create a NonZeroU32.
        unsafe { NonZeroU32::new_unchecked(buf_count) }
    };

    Ok((ring_size, buf_count))
}
