// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod memory;

//==============================================================================
// Imports
//==============================================================================

use self::memory::{
    consts::DEFAULT_MAX_BODY_SIZE,
    MemoryManager,
};
use crate::{
    inetstack::protocols::ethernet2::MIN_PAYLOAD_SIZE,
    collections::{
        dpdk_spinlock::DPDKSpinLock,
        dpdk_ring::DPDKRing,
    },
    runtime::{
        libdpdk::{
            rte_eal_init,
            rte_eth_conf,
            rte_socket_id,
            rte_eth_dev_configure,
            rte_eth_dev_count_avail,
            rte_eth_dev_get_mtu,
            rte_eth_dev_info_get,
            rte_eth_dev_set_mtu,
            rte_eth_dev_start,
            rte_eth_macaddr_get,
            rte_eth_find_next_owned_by,
            rte_eth_rx_mq_mode_RTE_ETH_MQ_RX_RSS as RTE_ETH_MQ_RX_RSS,
            rte_eth_rx_mq_mode_RTE_ETH_MQ_RX_NONE as RTE_ETH_MQ_RX_NONE,
            rte_eth_tx_mq_mode_RTE_ETH_MQ_TX_NONE as RTE_ETH_MQ_TX_NONE,
            rte_eth_rx_queue_setup,
            rte_eth_rxconf,
            rte_eth_txconf,
            rte_eth_tx_queue_setup,
            rte_ether_addr,
            rte_eth_rss_ip,
            rte_eth_rss_tcp,
            rte_eth_rss_udp,
            RTE_ETHER_MAX_LEN,
            RTE_ETH_DEV_NO_OWNER,
            RTE_PKTMBUF_HEADROOM,
            RTE_ETHER_MAX_JUMBO_FRAME_LEN,
            rte_eth_rx_offload_ip_cksum,
            rte_eth_tx_offload_ip_cksum,
            rte_eth_rx_offload_tcp_cksum,
            rte_eth_tx_offload_tcp_cksum,
            rte_eth_rx_offload_udp_cksum,
            rte_eth_tx_offload_udp_cksum,
            rte_eth_rx_burst,
            rte_eth_tx_burst,
            rte_mbuf,
            rte_pktmbuf_chain,
        },
        memory::DemiBuffer,
        network::{
            config::{
                ArpConfig,
                TcpConfig,
                UdpConfig,
            },
            consts::RECEIVE_BATCH_SIZE,
            types::MacAddress,
            NetworkRuntime,
            PacketBuf,
        },
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::std::mem;

use ::anyhow::{
    bail,
    format_err,
    Error,
};
use ::std::{
    ffi::CString,
    mem::MaybeUninit,
    net::Ipv4Addr,
    ops::{
        Deref,
        DerefMut,
    },
    sync::Arc,
    time::Duration,
};

//==============================================================================
// Macros
//==============================================================================

macro_rules! expect_zero {
    ($name:ident ( $($arg: expr),* $(,)* )) => {{
        let ret = $name($($arg),*);
        if ret == 0 {
            Ok(0)
        } else {
            Err(format_err!("{} failed with {:?}", stringify!($name), ret))
        }
    }};
}

//==============================================================================
// Structures
//==============================================================================

/// DPDK Runtime
pub struct DPDKRuntime {
    mm: Arc<MemoryManager>,
    port_id: u16,
    queue_id: u16,
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    arp_config: ArpConfig,
    tcp_config: TcpConfig,
    udp_config: UdpConfig,
    // Specific for cFCFS
    rxq_lock: *mut DPDKSpinLock,
    q_ptr: *mut DPDKRing,
}

#[derive(Clone)]
pub struct SharedDPDKRuntime(SharedObject<DPDKRuntime>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for DPDK Runtime
impl SharedDPDKRuntime {
    pub fn new(
        ipv4_addr: Ipv4Addr,
        arp_table: std::collections::HashMap<Ipv4Addr, MacAddress>,
        disable_arp: bool,
        mss: usize,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
        port_id: u16,
        queue_id: u16,
        rxq_lock: *mut DPDKSpinLock,
        q_ptr: *mut DPDKRing,
        mm: Arc<MemoryManager>,
    ) -> Self {
        let link_addr: MacAddress = unsafe {
            let mut m: MaybeUninit<rte_ether_addr> = MaybeUninit::zeroed();
            // TODO: Why does bindgen say this function doesn't return an int?
            rte_eth_macaddr_get(port_id, m.as_mut_ptr());
            MacAddress::new(m.assume_init().addr_bytes)
        };

        let arp_config = ArpConfig::new(
            Some(Duration::from_secs(15)),
            Some(Duration::from_secs(20)),
            Some(5),
            Some(arp_table),
            Some(disable_arp),
        );

        let tcp_config = TcpConfig::new(
            Some(mss),
            None,
            None,
            Some(0xffff),
            Some(0),
            None,
            Some(tcp_checksum_offload),
            Some(tcp_checksum_offload),
        );

        let udp_config = UdpConfig::new(Some(udp_checksum_offload), Some(udp_checksum_offload));

        Self(SharedObject::<DPDKRuntime>::new(DPDKRuntime {
            mm,
            port_id,
            queue_id,
            link_addr,
            ipv4_addr,
            arp_config,
            tcp_config,
            udp_config,
            rxq_lock,
            q_ptr,
        }))
    }

    /// Initializes DPDK.
    pub fn initialize_dpdk(
        eal_init_args: &[CString],
        use_jumbo_frames: bool,
        _mtu: u16,
        _tcp_checksum_offload: bool,
        _udp_checksum_offload: bool,
    ) -> Result<(MemoryManager, u16, MacAddress), Error> {
        // std::env::set_var("MLX5_SHUT_UP_BF", "1");
        // std::env::set_var("MLX5_SINGLE_THREADED", "1");
        // std::env::set_var("MLX4_SINGLE_THREADED", "1");
        let eal_init_refs = eal_init_args.iter().map(|s| s.as_ptr() as *mut u8).collect::<Vec<_>>();
        let ret: libc::c_int = unsafe { rte_eal_init(eal_init_refs.len() as i32, eal_init_refs.as_ptr() as *mut _) };
        if ret < 0 {
            let rte_errno: libc::c_int = unsafe { dpdk_rs::rte_errno() };
            bail!("EAL initialization failed (rte_errno={:?})", rte_errno);
        }
        let nb_ports: u16 = unsafe { rte_eth_dev_count_avail() };
        if nb_ports == 0 {
            bail!("No ethernet ports available");
        }
        eprintln!("DPDK reports that {} ports (interfaces) are available.", nb_ports);

        let max_body_size: usize = if use_jumbo_frames {
            (RTE_ETHER_MAX_JUMBO_FRAME_LEN + RTE_PKTMBUF_HEADROOM) as usize
        } else {
            DEFAULT_MAX_BODY_SIZE
        };

        let memory_manager = MemoryManager::new(max_body_size)?;

        let owner: u64 = RTE_ETH_DEV_NO_OWNER as u64;
        let port_id: u16 = unsafe { rte_eth_find_next_owned_by(0, owner) as u16 };

        // TODO: Where is this function?
        // if unsafe { rte_lcore_count() } > 1 {
        //     eprintln!("WARNING: Too many lcores enabled. Only 1 used.");
        // }

        let local_link_addr: MacAddress = unsafe {
            let mut m: MaybeUninit<rte_ether_addr> = MaybeUninit::zeroed();
            // TODO: Why does bindgen say this function doesn't return an int?
            rte_eth_macaddr_get(port_id, m.as_mut_ptr());
            MacAddress::new(m.assume_init().addr_bytes)
        };
        if local_link_addr.is_nil() || !local_link_addr.is_unicast() {
            Err(format_err!("Invalid mac address"))?;
        }

        Ok((memory_manager, port_id, local_link_addr))
    }

    /// Initializes a DPDK port.
    pub fn initialize_dpdk_port(
        port_id: u16,
        rx_queues: u16,
        tx_queues: u16,
        memory_manager: &MemoryManager,
        use_jumbo_frames: bool,
        mtu: u16,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Result<(), Error> {
        let rx_ring_size: u16 = 2048;
        let tx_ring_size: u16 = 2048;

        let dev_info: dpdk_rs::rte_eth_dev_info = unsafe {
            let mut d: MaybeUninit<dpdk_rs::rte_eth_dev_info> = MaybeUninit::zeroed();
            rte_eth_dev_info_get(port_id, d.as_mut_ptr());
            d.assume_init()
        };

        println!("dev_info: {:?}", dev_info);
        let mut port_conf: rte_eth_conf = unsafe { MaybeUninit::zeroed().assume_init() };

        port_conf.rxmode.mq_mode = if rx_queues > 1 {
            RTE_ETH_MQ_RX_RSS
        } else {
            RTE_ETH_MQ_RX_NONE
        };
        port_conf.txmode.mq_mode = RTE_ETH_MQ_TX_NONE;

        port_conf.rxmode.max_lro_pkt_size = if use_jumbo_frames {
            RTE_ETHER_MAX_JUMBO_FRAME_LEN
        } else {
            RTE_ETHER_MAX_LEN
        };
        port_conf.rx_adv_conf.rss_conf.rss_key = 0 as *mut _;
        port_conf.rx_adv_conf.rss_conf.rss_hf = unsafe { (rte_eth_rss_ip() | rte_eth_rss_udp() | rte_eth_rss_tcp()) as u64 } & dev_info.flow_type_rss_offloads;

        if tcp_checksum_offload {
            port_conf.rxmode.offloads |= unsafe { (rte_eth_rx_offload_ip_cksum() | rte_eth_rx_offload_tcp_cksum()) as u64 };
            port_conf.txmode.offloads |= unsafe { (rte_eth_tx_offload_ip_cksum() | rte_eth_tx_offload_tcp_cksum()) as u64 };
        }
        if udp_checksum_offload {
            port_conf.rxmode.offloads |= unsafe { (rte_eth_rx_offload_ip_cksum() | rte_eth_rx_offload_udp_cksum()) as u64 };
            port_conf.txmode.offloads |= unsafe { (rte_eth_tx_offload_ip_cksum() | rte_eth_tx_offload_udp_cksum()) as u64 };
        }

        unsafe {
            expect_zero!(rte_eth_dev_configure(
                port_id,
                rx_queues,
                tx_queues,
                &port_conf as *const _,
            ))?;
        }

        unsafe {
            expect_zero!(rte_eth_dev_set_mtu(port_id, mtu))?;
            let mut dpdk_mtu: u16 = 0u16;
            expect_zero!(rte_eth_dev_get_mtu(port_id, &mut dpdk_mtu as *mut _))?;
            if dpdk_mtu != mtu {
                bail!("Failed to set MTU to {}, got back {}", mtu, dpdk_mtu);
            }
        }

        let socket_id: u32 = unsafe { rte_socket_id() };

        let mut rx_conf: rte_eth_rxconf = dev_info.default_rxconf;
        rx_conf.offloads = port_conf.rxmode.offloads;
        rx_conf.rx_drop_en = 1;

        let mut tx_conf: rte_eth_txconf = dev_info.default_txconf;
        tx_conf.offloads = port_conf.txmode.offloads;

        unsafe {
            for i in 0..rx_queues {
                expect_zero!(rte_eth_rx_queue_setup(
                    port_id,
                    i,
                    rx_ring_size,
                    socket_id,
                    &rx_conf as *const _,
                    memory_manager.body_pool(),
                ))?;
            }
            for i in 0..tx_queues {
                expect_zero!(rte_eth_tx_queue_setup(
                    port_id,
                    i,
                    tx_ring_size,
                    socket_id,
                    &tx_conf as *const _,
                ))?;
            }
            expect_zero!(rte_eth_dev_start(port_id))?;
        }

        Ok(())
    }

    pub fn get_link_addr(&self) -> MacAddress {
        self.link_addr
    }

    pub fn get_ip_addr(&self) -> Ipv4Addr {
        self.ipv4_addr
    }

    pub fn get_arp_config(&self) -> ArpConfig {
        self.arp_config.clone()
    }

    pub fn get_udp_config(&self) -> UdpConfig {
        self.udp_config.clone()
    }

    pub fn get_tcp_config(&self) -> TcpConfig {
        self.tcp_config.clone()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Deref for SharedDPDKRuntime {
    type Target = DPDKRuntime;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedDPDKRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for DPDK Runtime
impl NetworkRuntime for SharedDPDKRuntime {
    fn transmit(&mut self, buf: Box<dyn PacketBuf>) {
        // TODO: Consider an important optimization here: If there is data in this packet (i.e. not just headers), and
        // that data is in a DPDK-owned mbuf, and there is "headroom" in that mbuf to hold the packet headers, just
        // prepend the headers into that mbuf and save the extra header mbuf allocation that we currently always do.

        // TODO: cleanup unwrap() and expect() from this code when this function returns a Result.

        // Alloc header mbuf, check header size.
        // Serialize header.
        // Decide if we can inline the data --
        //   1) How much space is left?
        //   2) Is the body small enough?
        // If we can inline, copy and return.
        // If we can't inline...
        //   1) See if the body is managed => take
        //   2) Not managed => alloc body
        // Chain body buffer.

        // First, allocate a header mbuf and write the header into it.
        let mut header_mbuf: DemiBuffer = match self.mm.alloc_header_mbuf() {
            Ok(mbuf) => mbuf,
            Err(e) => panic!("failed to allocate header mbuf: {:?}", e.cause),
        };
        let header_size = buf.header_size();
        assert!(header_size <= header_mbuf.len());
        buf.write_header(&mut header_mbuf[..header_size]);

        if let Some(body) = buf.take_body() {
            // Next, see how much space we have remaining and inline the body if we have room.
            let inline_space = header_mbuf.len() - header_size;

            // Chain a buffer.
            if body.len() > inline_space {
                assert!(header_size + body.len() >= MIN_PAYLOAD_SIZE);

                // We're only using the header_mbuf for, well, the header.
                header_mbuf.trim(header_mbuf.len() - header_size).unwrap();

                // Get the body mbuf.
                let body_mbuf: *mut rte_mbuf = if body.is_dpdk_allocated() {
                    // The body is already stored in an MBuf, just extract it from the DemiBuffer.
                    body.into_mbuf().expect("'body' should be DPDK-allocated")
                } else {
                    // The body is not dpdk-allocated, allocate a DPDKBuffer and copy the body into it.
                    let mut mbuf: DemiBuffer = match self.mm.alloc_body_mbuf() {
                        Ok(mbuf) => mbuf,
                        Err(e) => panic!("failed to allocate body mbuf: {:?}", e.cause),
                    };
                    assert!(mbuf.len() >= body.len());
                    mbuf[..body.len()].copy_from_slice(&body[..]);
                    mbuf.trim(mbuf.len() - body.len()).unwrap();
                    mbuf.into_mbuf().expect("mbuf should not be empty")
                };

                let mut header_mbuf_ptr: *mut rte_mbuf = header_mbuf.into_mbuf().expect("mbuf should not be empty");
                // Safety: rte_pktmbuf_chain is a FFI that is safe to call as both of its args are valid MBuf pointers.
                unsafe {
                    // Attach the body MBuf onto the header MBuf's buffer chain.
                    assert_eq!(rte_pktmbuf_chain(header_mbuf_ptr, body_mbuf), 0);
                }
                let num_sent = unsafe { rte_eth_tx_burst(self.port_id, self.queue_id, &mut header_mbuf_ptr, 1) };
                assert_eq!(num_sent, 1);
            }
            // Otherwise, write in the inline space.
            else {
                let body_buf = &mut header_mbuf[header_size..(header_size + body.len())];
                body_buf.copy_from_slice(&body[..]);

                if header_size + body.len() < MIN_PAYLOAD_SIZE {
                    let padding_bytes = MIN_PAYLOAD_SIZE - (header_size + body.len());
                    let padding_buf = &mut header_mbuf[(header_size + body.len())..][..padding_bytes];
                    for byte in padding_buf {
                        *byte = 0;
                    }
                }

                let frame_size = std::cmp::max(header_size + body.len(), MIN_PAYLOAD_SIZE);
                header_mbuf.trim(header_mbuf.len() - frame_size).unwrap();

                let mut header_mbuf_ptr: *mut rte_mbuf = header_mbuf.into_mbuf().expect("mbuf cannot be empty");
                let num_sent = unsafe { rte_eth_tx_burst(self.port_id, self.queue_id, &mut header_mbuf_ptr, 1) };
                assert_eq!(num_sent, 1);
            }
        }
        // No body on our packet, just send the headers.
        else {
            if header_size < MIN_PAYLOAD_SIZE {
                let padding_bytes = MIN_PAYLOAD_SIZE - header_size;
                let padding_buf = &mut header_mbuf[header_size..][..padding_bytes];
                for byte in padding_buf {
                    *byte = 0;
                }
            }
            let frame_size = std::cmp::max(header_size, MIN_PAYLOAD_SIZE);
            header_mbuf.trim(header_mbuf.len() - frame_size).unwrap();
            let mut header_mbuf_ptr: *mut rte_mbuf = header_mbuf.into_mbuf().expect("mbuf cannot be empty");
            let num_sent = unsafe { rte_eth_tx_burst(self.port_id, self.queue_id, &mut header_mbuf_ptr, 1) };
            assert_eq!(num_sent, 1);
        }
    }

    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut out: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();

        if unsafe { (*self.rxq_lock).trylock() } {
            let mut packets: [*mut rte_mbuf; RECEIVE_BATCH_SIZE] = unsafe { mem::zeroed() };
            let nb_rx: u16 = unsafe { rte_eth_rx_burst(self.port_id, 0, packets.as_mut_ptr(), RECEIVE_BATCH_SIZE as u16) };
            debug_assert!(nb_rx as usize <= RECEIVE_BATCH_SIZE);

            if nb_rx != 0 {
                log::warn!("[w{:?}]: received {:?} packet(s).", self.queue_id, nb_rx);
                for &packet in &packets[..nb_rx as usize] {
                    if let Err(e) = unsafe { (*self.q_ptr).enqueue(packet) } {
                        panic!("Error on enqueue incoming packets: {:?}", e)
                    }
                }
            }
            unsafe { (*self.rxq_lock).unlock() };
        }

        if let Some(packet) = unsafe { (*self.q_ptr).dequeue() } {
            log::warn!("[w{:?}]: dequeued 1 packet.", self.queue_id);
            let buf: DemiBuffer = unsafe { DemiBuffer::from_mbuf(packet) };
            out.push(buf);
        }

        out
    }

    fn get_arp_config(&self) -> ArpConfig {
        self.arp_config.clone()
    }

    fn get_udp_config(&self) -> UdpConfig {
        self.udp_config.clone()
    }

    fn get_tcp_config(&self) -> TcpConfig {
        self.tcp_config.clone()
    }
}
