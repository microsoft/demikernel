// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{
            RxRing,
            TxRing,
            XdpBuffer,
        },
    },
    demikernel::config::Config,
    expect_ok,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::{
            consts::RECEIVE_BATCH_SIZE,
            NetworkRuntime,
            PacketBuf,
        },
        Runtime,
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::std::borrow::{
    Borrow,
    BorrowMut,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS built on top of Windows XDP.
#[derive(Clone)]
pub struct SharedCatpowderRuntime(SharedObject<CatpowderRuntimeInner>);

/// The inner state of the Catpowder runtime.
struct CatpowderRuntimeInner {
    api: XdpApi,
    tx: TxRing,
    rx: RxRing,
}
//======================================================================================================================
// Implementations
//======================================================================================================================
impl SharedCatpowderRuntime {
    ///
    /// Number of buffers in the rings.
    ///
    /// **NOTE:** To introduce batch support (i.e., having more than one buffer in the ring), we need to revisit current
    /// implementation. It is not all about just changing this constant.
    ///
    const RING_LENGTH: u32 = 1;
}

impl NetworkRuntime for SharedCatpowderRuntime {
    /// Instantiates a new XDP runtime.
    fn new(config: &Config) -> Result<Self, Fail> {
        let ifindex: u32 = config.local_interface_index()?;
        const QUEUEID: u32 = 0; // We do no use RSS, thus queue id is always 0.

        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;

        // Open TX and RX rings
        let tx: TxRing = TxRing::new(&mut api, Self::RING_LENGTH, ifindex, QUEUEID)?;
        let rx: RxRing = RxRing::new(&mut api, Self::RING_LENGTH, ifindex, QUEUEID)?;

        Ok(Self(SharedObject::new(CatpowderRuntimeInner { api, rx, tx })))
    }

    /// Transmits a packet.
    fn transmit(&mut self, pkt: Box<dyn PacketBuf>) {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();
        trace!("transmit(): header_size={:?}, body_size={:?}", header_size, body_size);

        if header_size + body_size >= u16::MAX as usize {
            warn!("packet is too large: {:?}", header_size + body_size);
            return;
        }

        let mut idx: u32 = 0;

        if self.0.borrow_mut().tx.reserve_tx(Self::RING_LENGTH, &mut idx) != Self::RING_LENGTH {
            warn!("failed to reserve producer space for packet");
            return;
        }

        let mut buf: XdpBuffer = self.0.borrow_mut().tx.get_buffer(idx, header_size + body_size);

        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }

        self.0.borrow_mut().tx.submit_tx(Self::RING_LENGTH);

        // Notify socket.
        let mut outflags: i32 = xdp_rs::XSK_NOTIFY_RESULT_FLAGS::default();
        let flags: i32 =
            xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_POKE_TX | xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_WAIT_TX;

        if let Err(e) = self.0.borrow_mut().notify_socket(flags, u32::MAX, &mut outflags) {
            warn!("failed to notify socket: {:?}", e);
            return;
        }

        if self
            .0
            .borrow_mut()
            .tx
            .reserve_tx_completion(Self::RING_LENGTH, &mut idx)
            != Self::RING_LENGTH
        {
            warn!("failed to send packet");
            return;
        }

        self.0.borrow_mut().tx.release_tx_completion(Self::RING_LENGTH);
    }

    /// Polls for received packets.
    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();
        let mut idx: u32 = 0;

        if self.0.borrow_mut().rx.reserve_rx(Self::RING_LENGTH, &mut idx) == Self::RING_LENGTH {
            let xdp_buffer: XdpBuffer = self.0.borrow().rx.get_buffer(idx);
            let out: Vec<u8> = xdp_buffer.into();

            let dbuf: DemiBuffer = expect_ok!(DemiBuffer::from_slice(&out), "'bytes' should fit");

            ret.push(dbuf);

            self.0.borrow_mut().rx.release_rx(Self::RING_LENGTH);

            self.0.borrow_mut().rx.reserve_rx_fill(Self::RING_LENGTH, &mut idx);

            self.0.borrow_mut().rx.submit_rx_fill(Self::RING_LENGTH);
        }

        ret
    }
}

impl CatpowderRuntimeInner {
    /// Notifies the socket that there are packets to be transmitted.
    fn notify_socket(&mut self, flags: i32, timeout: u32, outflags: &mut i32) -> Result<(), Fail> {
        self.tx.notify_socket(&mut self.api, flags, timeout, outflags)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory runtime trait implementation for XDP Runtime.
impl MemoryRuntime for SharedCatpowderRuntime {}

/// Runtime trait implementation for XDP Runtime.
impl Runtime for SharedCatpowderRuntime {}
