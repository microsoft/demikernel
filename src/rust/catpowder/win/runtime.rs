// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::transport::error::expect_last_wsa_error,
    catpowder::win::{
        api::XdpApi,
        ring::{RuleSet, RxRing, TxRing},
    },
    demi_sgarray_t, demi_sgaseg_t,
    demikernel::config::Config,
    inetstack::{
        consts::{MAX_HEADER_SIZE, RECEIVE_BATCH_SIZE},
        protocols::{layer1::PhysicalLayer, layer4::ephemeral::EphemeralPorts, Protocol},
    },
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, MemoryRuntime},
        Runtime, SharedObject,
    },
};
use arrayvec::ArrayVec;
use libc::c_void;
use std::{borrow::BorrowMut, mem, rc::Rc};
use windows::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER,
    Networking::WinSock::{
        closesocket, socket, WSACleanup, WSAIoctl, WSAStartup, AF_INET, INET_PORT_RANGE,
        INET_PORT_RESERVATION_INSTANCE, INVALID_SOCKET, IPPROTO_TCP, IPPROTO_UDP, SIO_ACQUIRE_PORT_RESERVATION, SOCKET,
        SOCK_DGRAM, SOCK_STREAM, WSADATA,
    },
    System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    },
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
    rx_rings: Vec<RxRing>,
    vf_rx_rings: Vec<RxRing>,
    reserved_socket: SOCKET,
    reserved_ports: Vec<u16>,
}
//======================================================================================================================
// Implementations
//======================================================================================================================
impl SharedCatpowderRuntime {
    /// Instantiates a new XDP runtime.
    pub fn new(config: &Config) -> Result<Self, Fail> {
        let ifindex: u32 = config.local_interface_index()?;

        let mut data: WSADATA = WSADATA::default();
        if unsafe { WSAStartup(0x202u16, &mut data as *mut WSADATA) } != 0 {
            return Err(expect_last_wsa_error());
        }

        let reserved_protocol: Option<Protocol> = config.xdp_reserved_port_protocol()?;
        let reserved_port_count: Option<u16> = config.xdp_reserved_port_count()?;

        let (reserved_socket, reserved_ports): (SOCKET, Vec<u16>) =
            if reserved_protocol.is_some() && reserved_port_count.is_some() {
                trace!(
                    "reserving {} ports with protocol {:?}",
                    reserved_port_count.unwrap(),
                    reserved_protocol.unwrap()
                );
                reserve_port_blocks(reserved_port_count.unwrap(), reserved_protocol.unwrap())?
            } else {
                trace!("reserved port options not set; no ports reserved");
                (INVALID_SOCKET, vec![])
            };

        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;

        let (tx_buffer_count, tx_ring_size) = config.tx_buffer_config()?;

        // Open TX and RX rings
        let tx: TxRing = TxRing::new(&mut api, tx_ring_size, tx_buffer_count, ifindex, 0)?;

        let cohost_mode = config.xdp_cohost_mode()?;
        let (mut tcp_ports, mut udp_ports) = if cohost_mode {
            let (tcp_ports, udp_ports) = config.xdp_cohost_ports()?;
            trace!(
                "XDP cohost mode enabled. TCP ports: {:?}, UDP ports: {:?}",
                tcp_ports,
                udp_ports
            );
            (tcp_ports, udp_ports)
        } else {
            trace!("XDP not cohosted; will redirect all traffic");
            (vec![], vec![])
        };

        if let Some(protocol) = reserved_protocol {
            match protocol {
                Protocol::Tcp => tcp_ports.extend(reserved_ports.iter().cloned()),
                Protocol::Udp => udp_ports.extend(reserved_ports.iter().cloned()),
            }
        }

        let ruleset: Rc<RuleSet> = if cohost_mode {
            RuleSet::new_cohost(
                config.local_ipv4_addr()?.into(),
                tcp_ports.as_slice(),
                udp_ports.as_slice(),
            )
        } else {
            RuleSet::new_redirect_all()
        };

        let queue_count: u32 = deduce_rss_settings(&mut api, ifindex)?;
        let mut rx_rings: Vec<RxRing> = Vec::with_capacity(queue_count as usize);
        let (rx_buffer_count, rx_ring_size) = config.rx_buffer_config()?;
        for queueid in 0..queue_count {
            rx_rings.push(RxRing::new(
                &mut api,
                rx_ring_size,
                rx_buffer_count,
                ifindex,
                queueid,
                ruleset.clone(),
            )?);
        }
        trace!("Created {} RX rings on interface {}", rx_rings.len(), ifindex);

        let vf_rx_rings: Vec<RxRing> = if let Ok(vf_if_index) = config.local_vf_interface_index() {
            // Optionally create VF RX rings
            let vf_queue_count: u32 = deduce_rss_settings(&mut api, vf_if_index)?;
            let mut vf_rx_rings: Vec<RxRing> = Vec::with_capacity(vf_queue_count as usize);
            for queueid in 0..vf_queue_count {
                vf_rx_rings.push(RxRing::new(
                    &mut api,
                    rx_ring_size,
                    rx_buffer_count,
                    vf_if_index,
                    queueid,
                    ruleset.clone(),
                )?);
            }
            trace!(
                "Created {} RX rings on VF interface {}.",
                vf_rx_rings.len(),
                vf_if_index
            );

            vf_rx_rings
        } else {
            vec![]
        };

        Ok(Self(SharedObject::new(CatpowderRuntimeInner {
            api,
            tx,
            rx_rings,
            vf_rx_rings,
            reserved_socket,
            reserved_ports,
        })))
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

        self.0.borrow_mut().tx.return_buffers();

        let me: &mut CatpowderRuntimeInner = &mut self.0.borrow_mut();
        me.tx.transmit_buffer(&mut me.api, pkt)?;

        Ok(())
    }

    /// Polls for received packets.
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail> {
        let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();

        let mut queue: usize = 0;
        for rx in self.0.borrow_mut().rx_rings.iter_mut() {
            let start_len: usize = ret.len() as usize;
            let remaining: u32 = (ret.capacity() - start_len) as u32;
            rx.process_rx(remaining, |dbuf: DemiBuffer| {
                trace!("receive(): non-VF, queue={}, pkt_size={:?}", queue, dbuf.len());
                ret.push(dbuf);
                Ok(())
            })?;

            if ret.len() > start_len {
                rx.provide_buffers();
            }

            if ret.is_full() {
                return Ok(ret);
            }
            queue += 1;
        }

        queue = 0;
        for rx in self.0.borrow_mut().vf_rx_rings.iter_mut() {
            let start_len: usize = ret.len() as usize;
            let remaining: u32 = (ret.capacity() - start_len) as u32;
            rx.process_rx(remaining, |dbuf: DemiBuffer| {
                trace!("receive(): VF, queue={}, pkt_size={:?}", queue, dbuf.len());
                ret.push(dbuf);
                Ok(())
            })?;

            if ret.len() > start_len {
                rx.provide_buffers();
            }

            if ret.is_full() {
                return Ok(ret);
            }
            queue += 1;
        }

        Ok(ret)
    }

    fn ephemeral_ports(&self) -> EphemeralPorts {
        let ports: &[u16] = self.0.reserved_ports.as_slice();
        if ports.len() == 0 {
            EphemeralPorts::default()
        } else {
            EphemeralPorts::new(ports).unwrap()
        }
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

fn reserve_port_blocks(port_count: u16, protocol: Protocol) -> Result<(SOCKET, Vec<u16>), Fail> {
    const MAX_HALVINGS: usize = 5;
    let mut ports: Vec<u16> = Vec::with_capacity(port_count as usize);

    let mut reservation_len: u16 = port_count;
    let mut halvings: usize = 0;

    let (sock_type, protocol) = match protocol {
        Protocol::Tcp => (SOCK_STREAM, IPPROTO_TCP.0),
        Protocol::Udp => (SOCK_DGRAM, IPPROTO_UDP.0),
    };

    let s: SOCKET = unsafe { socket(AF_INET.0.into(), sock_type, protocol) };
    if s == INVALID_SOCKET {
        return Err(expect_last_wsa_error());
    }

    while ports.len() < port_count as usize {
        trace!("reserve_port_blocks(): trying reservation length: {}", reservation_len);
        match reserve_ports(reservation_len, s) {
            Ok((start, count, _)) if count > 0 => {
                let end: u16 = start + (count - 1);
                trace!("reserve_port_blocks(): reserved ports: {}-{}", start, end);
                ports.extend(start..=end);
            },
            Ok(_) => {
                panic!("reserve_port_blocks(): reserved zero ports");
            },
            Err(e) => {
                halvings += 1;
                if halvings >= MAX_HALVINGS || reservation_len == 1 {
                    error!("reserve_port_blocks(): failed to reserve ports; giving up: {:?}", e);
                    let _ = unsafe { closesocket(s) };
                    return Err(e);
                } else {
                    trace!(
                        "reserve_port_blocks(): failed to reserve ports; halving reservation size: {:?}",
                        e
                    );
                    reservation_len /= 2;
                }
            },
        }
    }

    Ok((s, ports))
}

fn reserve_ports(port_count: u16, s: SOCKET) -> Result<(u16, u16, u64), Fail> {
    let port_range: INET_PORT_RANGE = INET_PORT_RANGE {
        StartPort: 0,
        NumberOfPorts: port_count,
    };

    let mut reservation: INET_PORT_RESERVATION_INSTANCE = INET_PORT_RESERVATION_INSTANCE::default();
    let mut bytes_out: u32 = 0;

    let result: i32 = unsafe {
        WSAIoctl(
            s,
            SIO_ACQUIRE_PORT_RESERVATION,
            Some(&port_range as *const INET_PORT_RANGE as *mut libc::c_void),
            std::mem::size_of::<INET_PORT_RANGE>() as u32,
            Some(&mut reservation as *mut INET_PORT_RESERVATION_INSTANCE as *mut libc::c_void),
            std::mem::size_of::<INET_PORT_RESERVATION_INSTANCE>() as u32,
            &mut bytes_out,
            None,
            None,
        )
    };

    if result != 0 {
        return Err(expect_last_wsa_error());
    }

    Ok((
        u16::from_be(reservation.Reservation.StartPort),
        reservation.Reservation.NumberOfPorts,
        reservation.Token.Token,
    ))
}

fn count_processor_cores() -> Result<usize, Fail> {
    let mut proc_info: SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX = SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX::default();
    let mut buffer_len: u32 = 0;

    if let Err(e) =
        unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, Some(&mut proc_info), &mut buffer_len) }
    {
        if e.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult() {
            let cause: String = format!("GetLogicalProcessorInformationEx failed: {:?}", e);
            return Err(Fail::new(libc::EFAULT, &cause));
        }
    } else {
        return Err(Fail::new(
            libc::EFAULT,
            "GetLogicalProcessorInformationEx did not return any information",
        ));
    }

    let mut buf: Vec<u8> = vec![0; buffer_len as usize];
    if let Err(e) = unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut buffer_len,
        )
    } {
        let cause: String = format!("GetLogicalProcessorInformationEx failed: {:?}", e);
        return Err(Fail::new(libc::EFAULT, &cause));
    }

    let mut core_count: usize = 0;
    let std::ops::Range {
        start: mut proc_core_info,
        end: proc_core_end,
    } = buf.as_ptr_range();
    while proc_core_info < proc_core_end && proc_core_info >= buf.as_ptr() {
        // Safety: the buffer is initialized to valid values by GetLogicalProcessorInformationEx, and the pointer is
        // not aliased. Bounds are checked above.
        let proc_info: &SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX =
            unsafe { &*(proc_core_info as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX) };
        if proc_info.Relationship == RelationProcessorCore {
            core_count += 1;
        }
        proc_core_info = proc_core_info.wrapping_add(proc_info.Size as usize);
    }

    return Ok(core_count);
}

/// Deduces the RSS settings for the given interface. Returns the number of valid RSS queues for the interface.
fn deduce_rss_settings(api: &mut XdpApi, ifindex: u32) -> Result<u32, Fail> {
    const DUMMY_QUEUE_LENGTH: u32 = 1;
    const DUMMY_BUFFER_COUNT: u32 = 1;
    let sys_proc_count: u32 = count_processor_cores()? as u32;

    // NB there will always be at least one queue available, hence starting the loop at 1. There should not be more
    // queues than the number of processors on the system.
    for queueid in 1..sys_proc_count {
        match TxRing::new(api, DUMMY_QUEUE_LENGTH, DUMMY_BUFFER_COUNT, ifindex, queueid) {
            Ok(_) => (),
            Err(e) => {
                warn!(
                    "Failed to create TX ring on queue {}: {:?}. This is only an error if {} is a valid RSS queue \
                     ID",
                    queueid, e, queueid
                );
                return Ok(queueid);
            },
        }
    }

    Ok(sys_proc_count)
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

        // Allocate buffer from sender pool.
        let mut buf: DemiBuffer = match self.0.tx.get_buffer() {
            None => return Err(Fail::new(libc::ENOBUFS, "out of buffers")),
            Some(buf) => buf,
        };

        if size > buf.len() - MAX_HEADER_SIZE {
            return Err(Fail::new(libc::EINVAL, "size too large for buffer"));
        }

        // Reserve space for headers.
        buf.adjust(MAX_HEADER_SIZE).expect("buffer size invariant violation");

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

impl Drop for SharedCatpowderRuntime {
    fn drop(&mut self) {
        if self.0.reserved_socket != INVALID_SOCKET {
            let _ = unsafe { closesocket(self.0.reserved_socket) };
        }

        let _ = unsafe { WSACleanup() };
    }
}
