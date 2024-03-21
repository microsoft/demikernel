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
    demikernel::config::Config,
    expect_some,
    inetstack::protocols::ethernet2::MIN_PAYLOAD_SIZE,
    runtime::{
        fail::Fail,
        libdpdk::{
            rte_delay_us_block,
            rte_eal_init,
            rte_eth_conf,
            rte_eth_dev_configure,
            rte_eth_dev_count_avail,
            rte_eth_dev_get_mtu,
            rte_eth_dev_info_get,
            rte_eth_dev_is_valid_port,
            rte_eth_dev_set_mtu,
            rte_eth_dev_start,
            rte_eth_find_next_owned_by,
            rte_eth_link,
            rte_eth_link_get_nowait,
            rte_eth_macaddr_get,
            rte_eth_promiscuous_enable,
            rte_eth_rss_ip,
            rte_eth_rx_burst,
            rte_eth_rx_mq_mode_RTE_ETH_MQ_RX_RSS as RTE_ETH_MQ_RX_RSS,
            rte_eth_rx_offload_tcp_cksum,
            rte_eth_rx_offload_udp_cksum,
            rte_eth_rx_queue_setup,
            rte_eth_rxconf,
            rte_eth_tx_burst,
            rte_eth_tx_mq_mode_RTE_ETH_MQ_TX_NONE as RTE_ETH_MQ_TX_NONE,
            rte_eth_tx_offload_multi_segs,
            rte_eth_tx_offload_tcp_cksum,
            rte_eth_tx_offload_udp_cksum,
            rte_eth_tx_queue_setup,
            rte_eth_txconf,
            rte_ether_addr,
            rte_mbuf,
            RTE_ETHER_MAX_JUMBO_FRAME_LEN,
            RTE_ETHER_MAX_LEN,
            RTE_ETH_DEV_NO_OWNER,
            RTE_ETH_LINK_FULL_DUPLEX,
            RTE_ETH_LINK_UP,
            RTE_PKTMBUF_HEADROOM,
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
    mm: MemoryManager,
    port_id: u16,
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    arp_config: ArpConfig,
    tcp_config: TcpConfig,
    udp_config: UdpConfig,
}

#[derive(Clone)]
pub struct SharedDPDKRuntime(SharedObject<DPDKRuntime>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for DPDK Runtime
impl SharedDPDKRuntime {
    pub fn new(config: Config) -> Result<Self, Fail> {
        let (mm, port_id, link_addr) = Self::initialize_dpdk(
            &config.eal_init_args(),
            config.use_jumbo_frames(),
            config.mtu()?,
            config.tcp_checksum_offload(),
            config.udp_checksum_offload(),
        )
        .unwrap();

        let arp_config = ArpConfig::new(
            Some(Duration::from_secs(15)),
            Some(Duration::from_secs(20)),
            Some(5),
            Some(config.arp_table()),
            Some(config.disable_arp()),
        );

        let tcp_config = TcpConfig::new(
            Some(config.mss()?),
            None,
            None,
            Some(0xffff),
            Some(0),
            None,
            Some(config.tcp_checksum_offload()),
            Some(config.udp_checksum_offload()),
        );

        let udp_config = UdpConfig::new(Some(config.udp_checksum_offload()), Some(config.udp_checksum_offload()));

        Ok(Self(SharedObject::<DPDKRuntime>::new(DPDKRuntime {
            mm,
            port_id,
            link_addr,
            ipv4_addr: config.local_ipv4_addr(),
            arp_config,
            tcp_config,
            udp_config,
        })))
    }

    /// Initializes DPDK.
    fn initialize_dpdk(
        eal_init_args: &[CString],
        use_jumbo_frames: bool,
        mtu: u16,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Result<(MemoryManager, u16, MacAddress), Error> {
        std::env::set_var("MLX5_SHUT_UP_BF", "1");
        std::env::set_var("MLX5_SINGLE_THREADED", "1");
        std::env::set_var("MLX4_SINGLE_THREADED", "1");
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
        Self::initialize_dpdk_port(
            port_id,
            &memory_manager,
            use_jumbo_frames,
            mtu,
            tcp_checksum_offload,
            udp_checksum_offload,
        )?;

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
    fn initialize_dpdk_port(
        port_id: u16,
        memory_manager: &MemoryManager,
        use_jumbo_frames: bool,
        mtu: u16,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Result<(), Error> {
        let rx_rings: u16 = 1;
        let tx_rings: u16 = 1;
        let rx_ring_size: u16 = 2048;
        let tx_ring_size: u16 = 2048;
        let nb_rxd: u16 = rx_ring_size;
        let nb_txd: u16 = tx_ring_size;

        let rx_pthresh: u8 = 8;
        let rx_hthresh: u8 = 8;
        let rx_wthresh: u8 = 0;

        let tx_pthresh: u8 = 0;
        let tx_hthresh: u8 = 0;
        let tx_wthresh: u8 = 0;

        let dev_info: dpdk_rs::rte_eth_dev_info = unsafe {
            let mut d: MaybeUninit<dpdk_rs::rte_eth_dev_info> = MaybeUninit::zeroed();
            rte_eth_dev_info_get(port_id, d.as_mut_ptr());
            d.assume_init()
        };

        println!("dev_info: {:?}", dev_info);
        let mut port_conf: rte_eth_conf = unsafe { MaybeUninit::zeroed().assume_init() };
        port_conf.rxmode.max_lro_pkt_size = if use_jumbo_frames {
            RTE_ETHER_MAX_JUMBO_FRAME_LEN
        } else {
            RTE_ETHER_MAX_LEN
        };
        if tcp_checksum_offload {
            port_conf.rxmode.offloads |= unsafe { rte_eth_rx_offload_tcp_cksum() as u64 };
        }
        if udp_checksum_offload {
            port_conf.rxmode.offloads |= unsafe { rte_eth_rx_offload_udp_cksum() as u64 };
        }
        port_conf.rxmode.mq_mode = RTE_ETH_MQ_RX_RSS;
        port_conf.rx_adv_conf.rss_conf.rss_hf = unsafe { rte_eth_rss_ip() as u64 } | dev_info.flow_type_rss_offloads;

        port_conf.txmode.mq_mode = RTE_ETH_MQ_TX_NONE;
        if tcp_checksum_offload {
            port_conf.txmode.offloads |= unsafe { rte_eth_tx_offload_tcp_cksum() as u64 };
        }
        if udp_checksum_offload {
            port_conf.txmode.offloads |= unsafe { rte_eth_tx_offload_udp_cksum() as u64 };
        }
        port_conf.txmode.offloads |= unsafe { rte_eth_tx_offload_multi_segs() as u64 };

        let mut rx_conf: rte_eth_rxconf = unsafe { MaybeUninit::zeroed().assume_init() };
        rx_conf.rx_thresh.pthresh = rx_pthresh;
        rx_conf.rx_thresh.hthresh = rx_hthresh;
        rx_conf.rx_thresh.wthresh = rx_wthresh;
        rx_conf.rx_free_thresh = 32;

        let mut tx_conf: rte_eth_txconf = unsafe { MaybeUninit::zeroed().assume_init() };
        tx_conf.tx_thresh.pthresh = tx_pthresh;
        tx_conf.tx_thresh.hthresh = tx_hthresh;
        tx_conf.tx_thresh.wthresh = tx_wthresh;
        tx_conf.tx_free_thresh = 32;

        unsafe {
            expect_zero!(rte_eth_dev_configure(
                port_id,
                rx_rings,
                tx_rings,
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

        let socket_id: u32 = 0;

        unsafe {
            for i in 0..rx_rings {
                expect_zero!(rte_eth_rx_queue_setup(
                    port_id,
                    i,
                    nb_rxd,
                    socket_id,
                    &rx_conf as *const _,
                    memory_manager.body_pool(),
                ))?;
            }
            for i in 0..tx_rings {
                expect_zero!(rte_eth_tx_queue_setup(
                    port_id,
                    i,
                    nb_txd,
                    socket_id,
                    &tx_conf as *const _
                ))?;
            }
            expect_zero!(rte_eth_dev_start(port_id))?;
            rte_eth_promiscuous_enable(port_id);
        }

        if unsafe { rte_eth_dev_is_valid_port(port_id) } == 0 {
            bail!("Invalid port");
        }

        let sleep_duration: Duration = Duration::from_millis(100);
        let mut retry_count: i32 = 90;

        loop {
            unsafe {
                let mut link: MaybeUninit<rte_eth_link> = MaybeUninit::zeroed();
                rte_eth_link_get_nowait(port_id, link.as_mut_ptr());
                let link: rte_eth_link = link.assume_init();
                if link.link_status() as u32 == RTE_ETH_LINK_UP {
                    let duplex: &str = if link.link_duplex() as u32 == RTE_ETH_LINK_FULL_DUPLEX {
                        "full"
                    } else {
                        "half"
                    };
                    eprintln!(
                        "Port {} Link Up - speed {} Mbps - {} duplex",
                        port_id, link.link_speed, duplex
                    );
                    break;
                }
                rte_delay_us_block(sleep_duration.as_micros() as u32);
            }
            if retry_count == 0 {
                bail!("Link never came up");
            }
            retry_count -= 1;
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
    fn transmit(&mut self, mut buf: Box<dyn PacketBuf>) {
        let headers_size: usize = buf.header_size();
        debug_assert!(headers_size < u16::MAX as usize);

        let mut mbuf: DemiBuffer = match buf.take_body() {
            Some(mut body) => {
                debug_assert_eq!(body.is_dpdk_allocated(), true);
                match body.prepend(headers_size.try_into().unwrap()) {
                    Ok(()) => body,
                    Err(e) => panic!("failed to prepend mbuf: {:?}", e.cause),
                }
            }
            None => {
                let frame_size: usize = std::cmp::max(headers_size, MIN_PAYLOAD_SIZE);
                let mut mbuf: DemiBuffer = match self.mm.alloc_header_mbuf() {
                    Ok(mbuf) => mbuf,
                    Err(e) => panic!("failed to allocate mbuf: {:?}", e.cause),
                };

                if headers_size < MIN_PAYLOAD_SIZE {
                    let padding_bytes: usize = MIN_PAYLOAD_SIZE - headers_size;
                    let padding_buf: &mut [u8] = &mut mbuf[headers_size..][..padding_bytes];
                    for byte in padding_buf {
                        *byte = 0;
                    }
                }

                mbuf
            }
        };

        let headers_ptr: &mut [u8] = &mut mbuf[..headers_size];
        buf.write_header(headers_ptr);

        let mut mbuf_ptr: *mut rte_mbuf = expect_some!(mbuf.into_mbuf(), "mbuf cannot be empty");
        let num_sent: u16 = unsafe { rte_eth_tx_burst(self.port_id, 0, &mut mbuf_ptr, 1) };
        debug_assert_eq!(num_sent, 1);
    }

    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut out = ArrayVec::new();

        let mut packets: [*mut rte_mbuf; RECEIVE_BATCH_SIZE] = unsafe { mem::zeroed() };
        let nb_rx = unsafe { rte_eth_rx_burst(self.port_id, 0, packets.as_mut_ptr(), RECEIVE_BATCH_SIZE as u16) };
        assert!(nb_rx as usize <= RECEIVE_BATCH_SIZE);

        {
            for &packet in &packets[..nb_rx as usize] {
                // Safety: `packet` is a valid pointer to a properly initialized `rte_mbuf` struct.
                let buf: DemiBuffer = unsafe { DemiBuffer::from_mbuf(packet) };
                out.push(buf);
            }
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
