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
    demi_sgarray_t,
    demi_sgaseg_t,
    demikernel::config::Config,
    inetstack::protocols::{
        layer1::PhysicalLayer,
        MAX_HEADER_SIZE,
    },
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::consts::RECEIVE_BATCH_SIZE,
        Runtime,
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::libc::c_void;
use ::std::{
    borrow::{
        Borrow,
        BorrowMut,
    },
    mem,
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

    /// Instantiates a new XDP runtime.
    pub fn new(config: &Config) -> Result<Self, Fail> {
        let ifindex: u32 = config.local_interface_index()?;
        const QUEUEID: u32 = 0; // We do no use RSS, thus queue id is always 0.

        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;

        // Open TX and RX rings
        let tx: TxRing = TxRing::new(&mut api, Self::RING_LENGTH, ifindex, QUEUEID)?;
        let rx: RxRing = RxRing::new(&mut api, Self::RING_LENGTH, ifindex, QUEUEID)?;

        Ok(Self(SharedObject::new(CatpowderRuntimeInner { api, rx, tx })))
    }
}

impl PhysicalLayer for SharedCatpowderRuntime {
    /// Transmits a packet.
    fn transmit(&mut self, pkt: DemiBuffer) -> Result<(), Fail> {
        let pkt_size: usize = pkt.len();
        trace!("transmit(): pkt_size={:?}", pkt_size);
        if pkt_size >= u16::MAX as usize {
            let cause = format!("packet is too large: {:?}", pkt_size);
            warn!("{}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        let mut idx: u32 = 0;

        if self.0.borrow_mut().tx.reserve_tx(Self::RING_LENGTH, &mut idx) != Self::RING_LENGTH {
            let cause = format!("failed to reserve producer space for packet");
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        let mut buf: XdpBuffer = self.0.borrow_mut().tx.get_buffer(idx, pkt_size);
        buf.copy_from_slice(&pkt);

        self.0.borrow_mut().tx.submit_tx(Self::RING_LENGTH);

        // Notify socket.
        let mut outflags: i32 = xdp_rs::XSK_NOTIFY_RESULT_FLAGS::default();
        let flags: i32 =
            xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_POKE_TX | xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_WAIT_TX;

        if let Err(e) = self.0.borrow_mut().notify_socket(flags, u32::MAX, &mut outflags) {
            let cause = format!("failed to notify socket: {:?}", e);
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        if self
            .0
            .borrow_mut()
            .tx
            .reserve_tx_completion(Self::RING_LENGTH, &mut idx)
            != Self::RING_LENGTH
        {
            let cause = format!("failed to send packet");
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        self.0.borrow_mut().tx.release_tx_completion(Self::RING_LENGTH);
        Ok(())
    }

    /// Polls for received packets.
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail> {
        let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();
        let mut idx: u32 = 0;

        if self.0.borrow_mut().rx.reserve_rx(Self::RING_LENGTH, &mut idx) == Self::RING_LENGTH {
            let xdp_buffer: XdpBuffer = self.0.borrow().rx.get_buffer(idx);
            let out: Vec<u8> = xdp_buffer.into();

            let dbuf: DemiBuffer = DemiBuffer::from_slice(&out)?;

            ret.push(dbuf);

            self.0.borrow_mut().rx.release_rx(Self::RING_LENGTH);

            self.0.borrow_mut().rx.reserve_rx_fill(Self::RING_LENGTH, &mut idx);

            self.0.borrow_mut().rx.submit_rx_fill(Self::RING_LENGTH);
        }

        Ok(ret)
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
impl MemoryRuntime for SharedCatpowderRuntime {
    /// Allocates a scatter-gather array.
    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        // TODO: Allocate an array of buffers if requested size is too large for a single buffer.

        // We can't allocate a zero-sized buffer.
        if size == 0 {
            let cause: String = format!("cannot allocate a zero-sized buffer");
            error!("sgaalloc(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // We can't allocate more than a single buffer.
        if size > u16::MAX as usize {
            return Err(Fail::new(libc::EINVAL, "size too large for a single demi_sgaseg_t"));
        }

        // First allocate the underlying DemiBuffer.
        // Always allocate with header space for now even if we do not need it.
        let buf: DemiBuffer = DemiBuffer::new_with_headroom(size as u16, MAX_HEADER_SIZE as u16);

        // Create a scatter-gather segment to expose the DemiBuffer to the user.
        let data: *const u8 = buf.as_ptr();
        let sga_seg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data as *mut c_void,
            sgaseg_len: size as u32,
        };

        // Create and return a new scatter-gather array (which inherits the DemiBuffer's reference).
        Ok(demi_sgarray_t {
            sga_buf: buf.into_raw().as_ptr() as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sga_seg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }
}

/// Runtime trait implementation for XDP Runtime.
impl Runtime for SharedCatpowderRuntime {}
