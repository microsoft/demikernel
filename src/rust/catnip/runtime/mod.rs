// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

mod consts;
mod mempool;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnip::runtime::{
        consts::{DEFAULT_BODY_POOL_SIZE, DEFAULT_CACHE_SIZE, DEFAULT_MAX_BODY_SIZE},
        mempool::MemoryPool,
    },
    demikernel::config::Config,
    expect_some,
    inetstack::{
        consts::{MAX_HEADER_SIZE, RECEIVE_BATCH_SIZE},
        protocols::layer1::PhysicalLayer,
    },
    runtime::{
        fail::Fail,
        libdpdk::{
            rte_delay_us_block, rte_eal_init, rte_errno, rte_eth_conf, rte_eth_dev_configure, rte_eth_dev_count_avail,
            rte_eth_dev_get_mtu, rte_eth_dev_info, rte_eth_dev_info_get, rte_eth_dev_is_valid_port,
            rte_eth_dev_set_mtu, rte_eth_dev_start, rte_eth_find_next_owned_by, rte_eth_link, rte_eth_link_get_nowait,
            rte_eth_promiscuous_enable, rte_eth_rss_ip, rte_eth_rx_burst,
            rte_eth_rx_mq_mode_RTE_ETH_MQ_RX_RSS as RTE_ETH_MQ_RX_RSS, rte_eth_rx_offload_tcp_cksum,
            rte_eth_rx_offload_udp_cksum, rte_eth_rx_queue_setup, rte_eth_rxconf, rte_eth_tx_burst,
            rte_eth_tx_mq_mode_RTE_ETH_MQ_TX_NONE as RTE_ETH_MQ_TX_NONE, rte_eth_tx_offload_multi_segs,
            rte_eth_tx_offload_tcp_cksum, rte_eth_tx_offload_udp_cksum, rte_eth_tx_queue_setup, rte_eth_txconf,
            rte_mbuf, RTE_ETHER_MAX_JUMBO_FRAME_LEN, RTE_ETHER_MAX_LEN, RTE_ETH_DEV_NO_OWNER, RTE_ETH_LINK_FULL_DUPLEX,
            RTE_ETH_LINK_UP, RTE_PKTMBUF_HEADROOM,
        },
        memory::{DemiBuffer, DemiMemoryAllocator},
        SharedObject,
    },
    timer,
};
use ::arrayvec::ArrayVec;
use ::std::{
    ffi::CString,
    mem,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    time::Duration,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct DPDKRuntime {
    max_body_size: usize,
    mem_pool: MemoryPool,
    port_id: u16,
}

#[derive(Clone)]
pub struct SharedDPDKRuntime(SharedObject<DPDKRuntime>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl SharedDPDKRuntime {
    pub fn new(config: &Config) -> Result<Self, Fail> {
        Self::set_environment_variables();
        Self::dpdk_eal_init(&config.eal_init_args()?)?;
        let port_id: u16 = Self::dpdk_eal_find_port()?;

        let use_jumbo_frames: bool = config.enable_jumbo_frames()?;
        let max_body_size: usize = if use_jumbo_frames {
            (RTE_ETHER_MAX_JUMBO_FRAME_LEN + RTE_PKTMBUF_HEADROOM) as usize
        } else {
            DEFAULT_MAX_BODY_SIZE
        };

        let mem_pool: MemoryPool = Self::initialize_mempool(max_body_size)?;

        let tcp_offload: bool = config.tcp_checksum_offload().is_ok_and(|offload| offload);
        let udp_offload: bool = config.udp_checksum_offload().is_ok_and(|offload| offload);

        Self::dpdk_initialize_port(
            &mem_pool,
            port_id,
            use_jumbo_frames,
            config.mtu()?,
            tcp_offload,
            udp_offload,
        )?;

        Ok(Self(SharedObject::<DPDKRuntime>::new(DPDKRuntime {
            max_body_size,
            mem_pool,
            port_id,
        })))
    }

    fn set_environment_variables() {
        std::env::set_var("MLX5_SHUT_UP_BF", "1");
        std::env::set_var("MLX5_SINGLE_THREADED", "1");
        std::env::set_var("MLX4_SINGLE_THREADED", "1");
    }

    fn dpdk_eal_init(eal_init_args: &[CString]) -> Result<(), Fail> {
        let eal_init_refs = eal_init_args.iter().map(|s| s.as_ptr() as *mut u8).collect::<Vec<_>>();
        let ret: libc::c_int = unsafe { rte_eal_init(eal_init_refs.len() as i32, eal_init_refs.as_ptr() as *mut _) };
        if ret < 0 {
            let rte_errno: libc::c_int = unsafe { rte_errno() };
            let cause: String = format!("EAL initialization failed (rte_errno={:?})", rte_errno);
            error!("initialize_dpdk(): {}", cause);
            return Err(Fail::new(libc::EIO, &cause));
        }
        Ok(())
    }

    fn dpdk_eal_find_port() -> Result<u16, Fail> {
        let nb_ports: u16 = unsafe { rte_eth_dev_count_avail() };
        if nb_ports == 0 {
            return Err(Fail::new(libc::EIO, "No ethernet ports available"));
        }
        trace!("DPDK reports that {} ports (interfaces) are available.", nb_ports);

        let owner: u64 = RTE_ETH_DEV_NO_OWNER as u64;
        let port_id: u16 = unsafe { rte_eth_find_next_owned_by(0, owner) as u16 };
        Ok(port_id)
    }

    fn initialize_mempool(max_body_size: usize) -> Result<MemoryPool, Fail> {
        MemoryPool::new(
            CString::new("body_pool").unwrap(),
            max_body_size,
            DEFAULT_BODY_POOL_SIZE,
            DEFAULT_CACHE_SIZE,
        )
    }

    fn dpdk_initialize_port(
        mem_pool: &MemoryPool,
        port_id: u16,
        use_jumbo_frames: bool,
        mtu: u16,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Result<(), Fail> {
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

        let dev_info: rte_eth_dev_info = unsafe {
            let mut dev_info: MaybeUninit<rte_eth_dev_info> = MaybeUninit::zeroed();
            rte_eth_dev_info_get(port_id, dev_info.as_mut_ptr());
            dev_info.assume_init()
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

        if unsafe { rte_eth_dev_configure(port_id, rx_rings, tx_rings, &port_conf as *const _) } != 0 {
            let cause: String = format!("Failed to configure ethernet device");
            error!("initialize_dpdk_port(): {}", cause);
            return Err(Fail::new(libc::EIO, &cause));
        }

        unsafe {
            if rte_eth_dev_set_mtu(port_id, mtu) != 0 {
                let cause: String = format!("Failed to set mtu {:?}", mtu);
                error!("initialize_dpdk_port(): {}", cause);
                return Err(Fail::new(libc::EIO, &cause));
            }
            let mut dpdk_mtu: u16 = 0u16;
            if (rte_eth_dev_get_mtu(port_id, &mut dpdk_mtu as *mut _)) != 0 {
                let cause: String = format!("Failed to get mtu");
                error!("initialize_dpdk_port(): {}", cause);
                return Err(Fail::new(libc::EIO, &cause));
            }
            if dpdk_mtu != mtu {
                let cause: String = format!("Failed to set MTU to {}, got back {}", mtu, dpdk_mtu);
                error!("initialize_dpdk_port(): {}", cause);
                return Err(Fail::new(libc::EIO, &cause));
            }
        }

        let socket_id: u32 = 0;

        unsafe {
            for i in 0..rx_rings {
                if rte_eth_rx_queue_setup(port_id, i, nb_rxd, socket_id, &rx_conf as *const _, mem_pool.into_raw()) != 0
                {
                    let cause: String = format!("Failed to set up rx queue");
                    error!("initialize_dpdk_port(): {}", cause);
                    return Err(Fail::new(libc::EIO, &cause));
                }
            }
            for i in 0..tx_rings {
                if rte_eth_tx_queue_setup(port_id, i, nb_txd, socket_id, &tx_conf as *const _) != 0 {
                    let cause: String = format!("Failed to set up tx ring {:?}", i);
                    error!("initialize_dpdk_port(): {}", cause);
                    return Err(Fail::new(libc::EIO, &cause));
                }
            }
            if rte_eth_dev_start(port_id) != 0 {
                let cause: String = format!("Failed to set up ethernet device");
                error!("initialize_dpdk_port(): {}", cause);
                return Err(Fail::new(libc::EIO, &cause));
            }
            rte_eth_promiscuous_enable(port_id);
        }

        if unsafe { rte_eth_dev_is_valid_port(port_id) } == 0 {
            let cause: String = format!("Invalid port id");
            error!("initialize_dpdk_port(): {}", cause);
            return Err(Fail::new(libc::EIO, &cause));
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
                let cause: String = format!("Link never came up");
                error!("initialize_dpdk_port(): {}", cause);
                return Err(Fail::new(libc::EIO, &cause));
            }
            retry_count -= 1;
        }

        Ok(())
    }

    fn dpdk_allocate_mbuf(&self, size: usize) -> Result<DemiBuffer, Fail> {
        // Allocate a DPDK-managed buffer.
        let mbuf_ptr: *mut rte_mbuf = self.mem_pool.alloc_mbuf(Some(size))?;
        // Safety: `mbuf_ptr` is a valid pointer to a properly initialized `rte_mbuf` struct.
        Ok(unsafe { DemiBuffer::from_mbuf(mbuf_ptr) })
    }
}

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

impl PhysicalLayer for SharedDPDKRuntime {
    fn transmit(&mut self, pkt: DemiBuffer) -> Result<(), Fail> {
        timer!("catnip::runtime::transmit");
        // Grab the packet and copy it if necessary. In general, this copy will happen for small packets without
        // payloads because we allocate actual data-carrying application buffers from the DPDK pool.
        let outgoing_pkt: DemiBuffer = match pkt {
            buf if buf.is_dpdk_allocated() => buf,
            buf if buf.len() <= self.max_body_size => {
                let mut mbuf: DemiBuffer = self.dpdk_allocate_mbuf(buf.len())?;
                debug_assert_eq!(buf.len(), mbuf.len());
                mbuf.copy_from_slice(&buf);

                mbuf
            },
            buf => {
                let cause: String = format!(
                    "Cannot allocate a DPDK buffer that is large enough. Max size={:?} request size={:?}",
                    self.max_body_size,
                    buf.len()
                );
                warn!("{}", cause);
                return Err(Fail::new(libc::EINVAL, &cause));
            },
        };

        let mut mbuf_ptr: *mut rte_mbuf = expect_some!(outgoing_pkt.into_mbuf(), "mbuf cannot be empty");
        let num_sent: u16 = unsafe { rte_eth_tx_burst(self.port_id, 0, &mut mbuf_ptr, 1) };
        debug_assert_eq!(num_sent, 1);
        Ok(())
    }

    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail> {
        timer!("catnip::runtime::receive");

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

        Ok(out)
    }
}

impl DemiMemoryAllocator for SharedDPDKRuntime {
    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        // First allocate the underlying DemiBuffer.
        if size <= self.max_body_size {
            self.dpdk_allocate_mbuf(size)
        } else {
            // Allocate a heap-managed buffer.
            Ok(DemiBuffer::new_with_headroom(size as u16, MAX_HEADER_SIZE as u16))
        }
    }
}
