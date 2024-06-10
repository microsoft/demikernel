// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::{
    borrow::{
        Borrow,
        BorrowMut,
    },
    mem::{
        self,
    },
};

use crate::{
    catpowder::win::{
        buffer::XdpBuffer,
        tx_ring::TxRing,
    },
    demikernel::config::Config,
    expect_ok,
    runtime::{
        fail::Fail,
        limits,
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
use arrayvec::ArrayVec;
use socket::XdpApi;

use super::{
    rx_ring::RxRing,
    socket,
};

//======================================================================================================================
// Structures
//======================================================================================================================

struct CatpowderRuntimeInner {
    tx: TxRing,
    rx: RxRing,
}
/// Underlying network transport.
#[derive(Clone)]
pub struct CatpowderRuntime {
    api: XdpApi,
    inner: SharedObject<CatpowderRuntimeInner>,
}

/// A network transport  built on top of Windows XDP.
#[derive(Clone)]
pub struct SharedXdpTransport(SharedObject<CatpowderRuntime>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================
impl CatpowderRuntime {}

impl NetworkRuntime for CatpowderRuntime {
    fn new(igconfig: &Config) -> Result<Self, Fail> {
        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;

        // TODO: read the following from the config file.
        let index: u32 = 5;
        let queueid: u32 = 0;

        let rx: RxRing = RxRing::new(&mut api, index, queueid)?;
        let tx: TxRing = TxRing::new(&mut api, index, queueid)?;

        Ok(Self {
            api,
            inner: SharedObject::new(CatpowderRuntimeInner { rx, tx }),
        })
    }

    fn transmit(&mut self, pkt: Box<dyn PacketBuf>) {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();
        trace!("header_size={:?}, body_size={:?}", header_size, body_size);

        assert!(header_size + body_size < u16::MAX as usize);
        let mut buf: DemiBuffer = DemiBuffer::new((header_size + body_size) as u16);

        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }

        let count: u32 = 1;
        let mut idx: u32 = 0;

        assert!(
            self.inner
                .borrow_mut()
                .tx
                .tx_ring
                .ring_producer_reserve(count, &mut idx)
                == 1
        );

        let b = self.inner.borrow_mut().tx.tx_ring.ring_get_element(idx) as *mut xdp_rs::XSK_BUFFER_DESCRIPTOR;

        assert!(buf.len() <= self.inner.borrow_mut().tx.mem.chunk_size() as usize);
        unsafe {
            let slice: &[u8] = &buf;
            let src = slice.as_ptr() as *const u8;
            let dst = self.inner.borrow_mut().tx.mem.get_address() as *mut u8;
            (*b).Length = buf.len() as u32;
            std::ptr::copy(src, dst, buf.len());
        }

        self.inner.borrow_mut().tx.tx_ring.ring_producer_submit(count);

        // Notify socket.
        let mut outflags = xdp_rs::XSK_NOTIFY_RESULT_FLAGS::default();
        self.inner
            .borrow()
            .tx
            .socket
            .notify_socket(
                &mut self.api,
                xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_POKE_TX | xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_WAIT_TX,
                u32::MAX,
                &mut outflags,
            )
            .unwrap();

        if self
            .inner
            .borrow_mut()
            .tx
            .tx_completion_ring
            .ring_consumer_reserve(count, &mut idx)
            == 1
        {
            self.inner
                .borrow_mut()
                .tx
                .tx_completion_ring
                .ring_consumer_release(count);
            return;
        }

        warn!("failed to send packet");
    }

    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();
        let count: u32 = 1;
        let mut idx: u32 = 0;

        if self.inner.borrow_mut().rx.consumer_reserve(count, &mut idx) == 1 {
            let mut out: [u8; limits::RECVBUF_SIZE_MAX] = [0; limits::RECVBUF_SIZE_MAX];

            // Get Rx buffer.
            let b: XdpBuffer = self.inner.borrow().rx.get_element(idx);

            assert!(b.len() <= self.inner.borrow().rx.mem.chunk_size() as usize);
            unsafe {
                let src = self.inner.borrow().rx.mem.get_address() as *const u8;
                let dest = out.as_mut_ptr();
                std::ptr::copy_nonoverlapping(src, dest, b.len());
            }

            let bytes: [u8; limits::RECVBUF_SIZE_MAX] =
                unsafe { mem::transmute::<[u8; limits::RECVBUF_SIZE_MAX], [u8; limits::RECVBUF_SIZE_MAX]>(out) };
            let mut dbuf: DemiBuffer = expect_ok!(DemiBuffer::from_slice(&bytes), "'bytes' should fit");

            expect_ok!(
                dbuf.trim(limits::RECVBUF_SIZE_MAX - b.len()),
                "'bytes' <= RECVBUF_SIZE_MAX"
            );

            ret.push(dbuf);

            self.inner.borrow_mut().rx.consumer_release(count);

            // Reserve RX ring buffer.
            let mut ring_index: u32 = 0;
            self.inner.borrow_mut().rx.producer_reserve(count, &mut ring_index);

            // let b = self.inner.borrow_mut().rx.rx_fill_ring.ring_get_element(ring_index) as *mut u64;
            // unsafe { *b = 0 };

            // Submit RX ring buffer.
            self.inner.borrow_mut().rx.producer_submit(count);
        }

        ret
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory runtime trait implementation for XDP Runtime.
impl MemoryRuntime for CatpowderRuntime {}

/// Runtime trait implementation for XDP Runtime.
impl Runtime for CatpowderRuntime {}
