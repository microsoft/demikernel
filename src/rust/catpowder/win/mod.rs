// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod socket;

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
        MaybeUninit,
    },
};

use crate::{
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
use socket::{
    Ring,
    XdpApi,
    XdpSocket,
};
use windows::Win32::{
    Foundation::HANDLE,
    System::Threading::QUEUE_USER_APC_CALLBACK_DATA_CONTEXT,
};
use xdp_rs::{
    XDP_HOOK_ID,
    XSK_BUFFER_DESCRIPTOR,
    XSK_RING_INFO,
    XSK_SOCKOPT_RX_RING_SIZE,
    _XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
    _XDP_HOOK_LAYER_XDP_HOOK_L2,
    _XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
};

//======================================================================================================================
// Structures
//======================================================================================================================

struct CatpowderRuntimeInner {
    tx: TxRing,
    rx: RxRing,
}

struct RxRing {
    program: HANDLE,
    mem: Box<xdp_rs::XSK_UMEM_REG>,
    socket: XdpSocket,
    rx_ring: Ring,
    rx_fill_ring: Ring,
}

#[allow(dead_code)]
struct TxRing {
    mem: Box<xdp_rs::XSK_UMEM_REG>,
    socket: XdpSocket,
    tx_ring: Ring,
    tx_completion_ring: Ring,
}

/// Underlying network transport.
#[derive(Clone)]
pub struct CatpowderRuntime {
    api: XdpApi,
    idx: u32,
    inner: SharedObject<CatpowderRuntimeInner>,
}

/// A network transport  built on top of Windows XDP.
#[derive(Clone)]
pub struct SharedXdpTransport(SharedObject<CatpowderRuntime>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl RxRing {
    fn new(api: &mut XdpApi, index: u32, queueid: u32) -> Result<Self, Fail> {
        trace!("Creating XDP socket.");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        let mut rx_buffer = Vec::<u8>::with_capacity(limits::RECVBUF_SIZE_MAX);

        let mem = Box::new(xdp_rs::XSK_UMEM_REG {
            TotalSize: limits::RECVBUF_SIZE_MAX as u64,
            ChunkSize: limits::RECVBUF_SIZE_MAX as u32,
            Headroom: 0,
            Address: rx_buffer.as_mut_ptr() as *mut core::ffi::c_void,
        });

        trace!("Registering UMEM.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_UMEM_REG,
            mem.as_ref() as *const xdp_rs::XSK_UMEM_REG as *const core::ffi::c_void,
            std::mem::size_of::<xdp_rs::XSK_UMEM_REG>() as u32,
        )?;
        const RING_SIZE: u32 = 1;
        trace!(
            "rx.address={:?}",
            mem.as_ref() as *const xdp_rs::XSK_UMEM_REG as *const core::ffi::c_void
        );

        trace!("Setting RX ring size.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RX_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Setting RX Fill ring size.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RX_FILL_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Binding RX queue.");
        socket.bind(api, index, queueid, xdp_rs::_XSK_BIND_FLAGS_XSK_BIND_FLAG_RX)?;

        trace!("Activating XDP socket.");
        socket.activate(api, xdp_rs::_XSK_ACTIVATE_FLAGS_XSK_ACTIVATE_FLAG_NONE)?;

        trace!("Getting RX ring info.");
        let mut ring_info: xdp_rs::XSK_RING_INFO_SET = unsafe { std::mem::zeroed() };
        let mut option_length: u32 = std::mem::size_of::<xdp_rs::XSK_RING_INFO_SET>() as u32;
        socket.getsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RING_INFO,
            &mut ring_info as *mut xdp_rs::XSK_RING_INFO_SET as *mut core::ffi::c_void,
            &mut option_length as *mut u32,
        )?;

        let mut rx_fill_ring: Ring = Ring::ring_initialize(&ring_info.Fill);
        let rx_ring: Ring = Ring::ring_initialize(&ring_info.Rx);

        trace!("Reserving RX ring buffer.");
        let mut ring_index: u32 = 0;
        rx_fill_ring.ring_producer_reserve(1, &mut ring_index);

        let b = rx_fill_ring.ring_get_element(ring_index) as *mut u64;
        unsafe { *b = 0 };

        trace!("Submitting RX ring buffer.");
        rx_fill_ring.ring_producer_submit(1);

        trace!("Setting RX Fill ring.");

        // Create XDP program.
        const XDP_INSPECT_RX: XDP_HOOK_ID = XDP_HOOK_ID {
            Layer: _XDP_HOOK_LAYER_XDP_HOOK_L2,
            Direction: _XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
            SubLayer: _XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
        };

        let redirect: xdp_rs::XDP_REDIRECT_PARAMS = {
            let mut redirect: xdp_rs::_XDP_REDIRECT_PARAMS = unsafe { mem::zeroed() };
            redirect.TargetType = xdp_rs::_XDP_REDIRECT_TARGET_TYPE_XDP_REDIRECT_TARGET_TYPE_XSK;
            redirect.Target = socket.socket;
            redirect
        };

        let rules: xdp_rs::XDP_RULE = unsafe {
            let mut rule: xdp_rs::XDP_RULE = std::mem::zeroed();
            rule.Match = xdp_rs::_XDP_MATCH_TYPE_XDP_MATCH_ALL;
            rule.Action = xdp_rs::_XDP_RULE_ACTION_XDP_PROGRAM_ACTION_REDIRECT;
            // TODO: Set redirect.
            // TODO: Set pattern
            // Perform bitwise copu from redirect to rule.
            rule.__bindgen_anon_1 =
                mem::transmute_copy::<xdp_rs::XDP_REDIRECT_PARAMS, xdp_rs::_XDP_RULE__bindgen_ty_1>(&redirect);

            rule
        };
        trace!("Creating XDP program.");
        let mut program: HANDLE = HANDLE::default();
        socket.create_program(api, &rules, index, &XDP_INSPECT_RX, queueid, 0, &mut program)?;

        trace!("XDP program created.");
        Ok(Self {
            program,
            mem,
            socket,
            rx_ring,
            rx_fill_ring,
        })
    }
}

impl TxRing {
    fn new(api: &mut XdpApi, index: u32, queueid: u32) -> Result<Self, Fail> {
        trace!("Creating XDP socket.");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        let mut buffer = Vec::<u8>::with_capacity(limits::RECVBUF_SIZE_MAX);

        let mem = Box::new(xdp_rs::XSK_UMEM_REG {
            TotalSize: limits::RECVBUF_SIZE_MAX as u64,
            ChunkSize: limits::RECVBUF_SIZE_MAX as u32,
            Headroom: 0,
            Address: buffer.as_mut_ptr() as *mut core::ffi::c_void,
        });

        trace!("tx.address={:?}", mem.as_ref().Address);

        trace!("Registering UMEM.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_UMEM_REG,
            mem.as_ref() as *const xdp_rs::XSK_UMEM_REG as *const core::ffi::c_void,
            std::mem::size_of::<xdp_rs::XSK_UMEM_REG>() as u32,
        )?;
        const RING_SIZE: u32 = 1;

        trace!("Setting TX ring size.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_TX_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Setting TX completion ring size.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_TX_COMPLETION_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Binding TX queue.");
        socket.bind(api, index, queueid, xdp_rs::_XSK_BIND_FLAGS_XSK_BIND_FLAG_TX)?;

        trace!("Activating XDP socket.");
        socket.activate(api, xdp_rs::_XSK_ACTIVATE_FLAGS_XSK_ACTIVATE_FLAG_NONE)?;

        trace!("Getting TX ring info.");
        let mut ring_info: xdp_rs::XSK_RING_INFO_SET = unsafe { std::mem::zeroed() };
        let mut option_length: u32 = std::mem::size_of::<xdp_rs::XSK_RING_INFO_SET>() as u32;
        socket.getsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RING_INFO,
            &mut ring_info as *mut xdp_rs::XSK_RING_INFO_SET as *mut core::ffi::c_void,
            &mut option_length as *mut u32,
        )?;

        let tx_ring: Ring = Ring::ring_initialize(&ring_info.Tx);
        let tx_completion_ring: Ring = Ring::ring_initialize(&ring_info.Completion);

        trace!("XDP program created.");
        Ok(Self {
            mem,
            socket,
            tx_ring,
            tx_completion_ring,
        })
    }
}

impl CatpowderRuntime {}

impl NetworkRuntime for CatpowderRuntime {
    fn new(igconfig: &Config) -> Result<Self, Fail> {
        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;
        let index: u32 = 5; // Todo: read this from config file.

        let queueid: u32 = 0;

        let rx: RxRing = RxRing::new(&mut api, index, queueid)?;
        let tx: TxRing = TxRing::new(&mut api, index, queueid)?;

        Ok(Self {
            api,
            idx: index,
            inner: SharedObject::new(CatpowderRuntimeInner { rx, tx }),
        })
    }

    fn transmit(&mut self, pkt: Box<dyn PacketBuf>) {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();

        assert!(header_size + body_size < u16::MAX as usize);
        let mut buf: DemiBuffer = DemiBuffer::new((header_size + body_size) as u16);

        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }

        println!("{:?}", &buf[..]);

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
        trace!("idx={:?}", idx);

        let b = self.inner.borrow_mut().tx.tx_ring.ring_get_element(idx) as *mut XSK_BUFFER_DESCRIPTOR;

        assert!(buf.len() <= self.inner.borrow_mut().tx.mem.TotalSize as usize);
        unsafe {
            let slice: &[u8] = &buf;
            let src = slice.as_ptr() as *const u8;
            let dst = self.inner.borrow_mut().tx.mem.Address as *mut u8;
            (*b).Length = buf.len() as u32;
            // let prev_addr = (*b).Address.__bindgen_anon_1.BaseAddress();
            // (*b).Address.__bindgen_anon_1.set_BaseAddress(dst as u64);
            // (*b).Address.__bindgen_anon_1.set_Offset(0);
            // let new_addr = (*b).Address.__bindgen_anon_1.BaseAddress() as *mut u8;
            // (*b).Address.AddressAndOffset = new_addr as u64;
            // let addr = (*b).Address.AddressAndOffset as *mut u8;
            // trace!(
            //     "tx.mem.Address={:?}, prev_addr={:?}, new_addr={:?}, addr={:?}, buf.len()={:?}",
            //     dst,
            //     prev_addr,
            //     new_addr,
            //     addr,
            //     buf.len()
            // );
            std::ptr::copy(src, dst, buf.len());
            let slice: &[u8] = core::slice::from_raw_parts(dst, buf.len());
            println!("{:?}", slice);
        }

        trace!("coutn={:?}", count);

        self.inner.borrow_mut().tx.tx_ring.ring_producer_submit(count);

        trace!("notify socket");

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

        loop {
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
                trace!("sent");
                break;
            }

            let mut outflags = xdp_rs::XSK_NOTIFY_RESULT_FLAGS::default();
            let _ = self.inner.borrow().tx.socket.notify_socket(
                &mut self.api,
                xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_POKE_TX | xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_WAIT_TX,
                1000,
                &mut outflags,
            );
            trace!("Waiting for TX completion.");
        }
    }

    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();
        let count: u32 = 1;
        let mut idx: u32 = 0;

        // let mut outflags = xdp_rs::XSK_NOTIFY_RESULT_FLAGS::default();
        // let _ = self.inner.borrow().rx.socket.notify_socket(
        //     &mut self.api,
        //     xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_POKE_RX | xdp_rs::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_WAIT_RX,
        //     0,
        //     &mut outflags,
        // );

        if self
            .inner
            .borrow_mut()
            .rx
            .rx_ring
            .ring_consumer_reserve(count, &mut idx)
            == 1
        {
            trace!("receive()");
            let mut out: [u8; limits::RECVBUF_SIZE_MAX] = [0; limits::RECVBUF_SIZE_MAX];

            // Get Rx buffer.
            let b = self.inner.borrow().rx.rx_ring.ring_get_element(idx) as *const XSK_BUFFER_DESCRIPTOR;
            let len = unsafe { (*b).Length };

            assert!(len as u64 <= self.inner.borrow().rx.mem.TotalSize);
            unsafe {
                let src = self.inner.borrow().rx.mem.Address as *const u8;
                let dest = out.as_mut_ptr();
                let prev_addr = (*b).Address.__bindgen_anon_1.BaseAddress();
                trace!("rx.mem.Address={:?}, prev_addr={:?}", src, prev_addr,);
                std::ptr::copy_nonoverlapping(src, dest, len as usize);
            }

            let bytes: [u8; limits::RECVBUF_SIZE_MAX] =
                unsafe { mem::transmute::<[u8; limits::RECVBUF_SIZE_MAX], [u8; limits::RECVBUF_SIZE_MAX]>(out) };
            let mut dbuf: DemiBuffer = expect_ok!(DemiBuffer::from_slice(&bytes), "'bytes' should fit");

            expect_ok!(
                dbuf.trim(limits::RECVBUF_SIZE_MAX - len as usize),
                "'bytes' <= RECVBUF_SIZE_MAX"
            );

            ret.push(dbuf);

            self.inner.borrow_mut().rx.rx_ring.ring_consumer_release(count);

            // Reserve RX ring buffer.
            let mut ring_index: u32 = 0;
            self.inner
                .borrow_mut()
                .rx
                .rx_fill_ring
                .ring_producer_reserve(count, &mut ring_index);

            let b = self.inner.borrow_mut().rx.rx_fill_ring.ring_get_element(ring_index) as *mut u64;
            unsafe { *b = 0 };

            // Submit RX ring buffer.
            self.inner.borrow_mut().rx.rx_fill_ring.ring_producer_submit(count);
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
