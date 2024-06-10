use super::socket::{
    XdpApi,
    XdpRing,
    XdpSocket,
};

use crate::{
    catpowder::win::umemreg::UmemReg,
    runtime::{
        fail::Fail,
        limits,
    },
};

#[allow(dead_code)]
pub struct TxRing {
    pub mem: UmemReg,
    pub socket: XdpSocket,
    pub tx_ring: XdpRing,
    pub tx_completion_ring: XdpRing,
}

impl TxRing {
    pub fn new(api: &mut XdpApi, index: u32, queueid: u32) -> Result<Self, Fail> {
        trace!("Creating XDP socket.");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        let mem: UmemReg = UmemReg::new(1, limits::RECVBUF_SIZE_MAX as u32);

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

        let tx_ring: XdpRing = XdpRing::ring_initialize(&ring_info.Tx);
        let tx_completion_ring: XdpRing = XdpRing::ring_initialize(&ring_info.Completion);

        trace!("XDP program created.");
        Ok(Self {
            mem,
            socket,
            tx_ring,
            tx_completion_ring,
        })
    }
}
