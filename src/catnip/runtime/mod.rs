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
    inetstack::{consts::MAX_BATCH_SIZE_NUM_PACKETS, protocols::layer1::PhysicalLayer},
    runtime::{
        fail::Fail,
        libdpdk::{
            rte_delay_us_block, rte_eal_init, rte_errno, rte_eth_conf, rte_eth_dev_configure, rte_eth_dev_count_avail,
            rte_eth_dev_get_mtu, rte_eth_dev_info_get, rte_eth_dev_is_valid_port, rte_eth_dev_set_mtu,
            rte_eth_dev_start, rte_eth_find_next_owned_by, rte_eth_link_get_nowait, rte_eth_promiscuous_enable,
            rte_eth_rx_burst, rte_eth_rx_mq_mode_RTE_ETH_MQ_RX_RSS as RTE_ETH_MQ_RX_RSS,
            rte_eth_rx_offload_tcp_cksum, rte_eth_rx_offload_udp_cksum, rte_eth_rx_queue_setup, rte_eth_rxconf,
            rte_eth_tx_burst, rte_eth_tx_mq_mode_RTE_ETH_MQ_TX_NONE as RTE_ETH_MQ_TX_NONE,
            rte_eth_tx_offload_multi_segs, rte_eth_tx_offload_tcp_cksum, rte_eth_tx_offload_udp_cksum,
            rte_eth_tx_queue_setup, rte_eth_txconf, rte_mbuf, RTE_ETHER_MAX_JUMBO_FRAME_LEN, RTE_ETHER_MAX_LEN,
            RTE_ETH_DEV_NO_OWNER, RTE_ETH_LINK_FULL_DUPLEX, RTE_ETH_LINK_UP, RTE_PKTMBUF_HEADROOM,
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

        let port_id = Self::dpdk_eal_find_port()?;

        let jumbo = config.enable_jumbo_frames()?;
        let max_body_size = if jumbo {
            (RTE_ETHER_MAX_JUMBO_FRAME_LEN + RTE_PKTMBUF_HEADROOM) as usize
        } else {
            DEFAULT_MAX_BODY_SIZE
        };

        let mem_pool = Self::initialize_mempool(max_body_size)?;

        let tcp_offload = config.tcp_checksum_offload().is_ok_and(|x| x);
        let udp_offload = config.udp_checksum_offload().is_ok_and(|x| x);

        Self::dpdk_initialize_port(&mem_pool, port_id, jumbo, config.mtu()?, tcp_offload, udp_offload)?;

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
        let argv = eal_init_args.iter().map(|s| s.as_ptr() as *mut u8).collect::<Vec<_>>();
        let ret = unsafe { rte_eal_init(argv.len() as i32, argv.as_ptr() as *mut _) };

        if ret < 0 {
            let err = unsafe { rte_errno() };
            let msg = format!("EAL init failed (rte_errno={:?})", err);
            error!("{}", msg);
            return Err(Fail::new(libc::EIO, &msg));
        }

        Ok(())
    }

    fn dpdk_eal_find_port() -> Result<u16, Fail> {
        let n = unsafe { rte_eth_dev_count_avail() };
        if n == 0 {
            return Err(Fail::new(libc::EIO, "no ethernet ports available"));
        }

        trace!("{} DPDK ports are available.", n);

        let port = unsafe { rte_eth_find_next_owned_by(0, RTE_ETH_DEV_NO_OWNER as u64) };
        Ok(port as u16)
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
        port: u16,
        jumbo: bool,
        mtu: u16,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Result<(), Fail> {
        let (rx_rings, tx_rings) = (1, 1);
        let (rx_ring_size, tx_ring_size) = (2048, 2048);
        let (nb_rxd, nb_txd) = (rx_ring_size, tx_ring_size);

        // RX thresholds
        let (rx_pthresh, rx_hthresh, rx_wthresh) = (8, 8, 0);

        // TX thresholds
        let (tx_pthresh, tx_hthresh, tx_wthresh) = (0, 0, 0);

        // Get device info
        let dev_info = unsafe {
            let mut info = MaybeUninit::zeroed();
            rte_eth_dev_info_get(port, info.as_mut_ptr());
            info.assume_init()
        };

        println!("dev_info: {:?}", dev_info);

        // Port config
        let mut port_conf: rte_eth_conf = unsafe { MaybeUninit::zeroed().assume_init() };
        port_conf.rxmode.max_lro_pkt_size = if jumbo {
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
        port_conf.rx_adv_conf.rss_conf.rss_hf = dev_info.flow_type_rss_offloads;

        port_conf.txmode.mq_mode = RTE_ETH_MQ_TX_NONE;
        if tcp_checksum_offload {
            port_conf.txmode.offloads |= unsafe { rte_eth_tx_offload_tcp_cksum() as u64 };
        }
        if udp_checksum_offload {
            port_conf.txmode.offloads |= unsafe { rte_eth_tx_offload_udp_cksum() as u64 };
        }
        port_conf.txmode.offloads |= unsafe { rte_eth_tx_offload_multi_segs() as u64 };

        // RX config
        let mut rx_conf: rte_eth_rxconf = unsafe { MaybeUninit::zeroed().assume_init() };
        rx_conf.rx_thresh.pthresh = rx_pthresh;
        rx_conf.rx_thresh.hthresh = rx_hthresh;
        rx_conf.rx_thresh.wthresh = rx_wthresh;
        rx_conf.rx_free_thresh = 32;

        // TX config
        let mut tx_conf: rte_eth_txconf = unsafe { MaybeUninit::zeroed().assume_init() };
        tx_conf.tx_thresh.pthresh = tx_pthresh;
        tx_conf.tx_thresh.hthresh = tx_hthresh;
        tx_conf.tx_thresh.wthresh = tx_wthresh;
        tx_conf.tx_free_thresh = 32;

        if unsafe { rte_eth_dev_configure(port, rx_rings, tx_rings, &port_conf as *const _) } != 0 {
            let msg = format!("failed to configure ethernet device");
            error!("initialize_dpdk_port(): {}", msg);
            return Err(Fail::new(libc::EIO, &msg));
        }

        unsafe {
            if rte_eth_dev_set_mtu(port, mtu) != 0 {
                let msg = format!("failed to set mtu {:?}", mtu);
                error!("initialize_dpdk_port(): {}", msg);
                return Err(Fail::new(libc::EIO, &msg));
            }

            let mut dpdk_mtu = 0u16;
            if (rte_eth_dev_get_mtu(port, &mut dpdk_mtu)) != 0 {
                let msg = format!("failed to get mtu");
                error!("initialize_dpdk_port(): {}", msg);
                return Err(Fail::new(libc::EIO, &msg));
            }

            if dpdk_mtu != mtu {
                let msg = format!("failed to set MTU to {}, got back {}", mtu, dpdk_mtu);
                error!("initialize_dpdk_port(): {}", msg);
                return Err(Fail::new(libc::EIO, &msg));
            }
        }

        let socket_id = 0;

        unsafe {
            for i in 0..rx_rings {
                if rte_eth_rx_queue_setup(port, i, nb_rxd, socket_id, &rx_conf as *const _, mem_pool.into_raw()) != 0 {
                    let msg = format!("failed to set up rx queue");
                    error!("initialize_dpdk_port(): {}", msg);
                    return Err(Fail::new(libc::EIO, &msg));
                }
            }
            for i in 0..tx_rings {
                if rte_eth_tx_queue_setup(port, i, nb_txd, socket_id, &tx_conf as *const _) != 0 {
                    let msg = format!("failed to set up tx ring {:?}", i);
                    error!("initialize_dpdk_port(): {}", msg);
                    return Err(Fail::new(libc::EIO, &msg));
                }
            }
            if rte_eth_dev_start(port) != 0 {
                let msg = "failed to set up ethernet device";
                error!("initialize_dpdk_port(): {}", msg);
                return Err(Fail::new(libc::EIO, msg));
            }
            rte_eth_promiscuous_enable(port);
        }

        if unsafe { rte_eth_dev_is_valid_port(port) } == 0 {
            let msg = "invalid port id";
            error!("initialize_dpdk_port(): {}", msg);
            return Err(Fail::new(libc::EIO, msg));
        }

        let delay = Duration::from_millis(100);
        let mut retries = 90;

        loop {
            unsafe {
                let mut link = MaybeUninit::zeroed();
                rte_eth_link_get_nowait(port, link.as_mut_ptr());
                let link = link.assume_init();

                if link.link_status() as u32 == RTE_ETH_LINK_UP {
                    let duplex = if link.link_duplex() as u32 == RTE_ETH_LINK_FULL_DUPLEX {
                        "full"
                    } else {
                        "half"
                    };
                    eprintln!(
                        "Port {} Link Up - speed {} Mbps - {} duplex",
                        port, link.link_speed, duplex
                    );
                    break;
                }
                rte_delay_us_block(delay.as_micros() as u32);
            }

            if retries == 0 {
                let msg = "link never came up";
                error!("initialize_dpdk_port(): {}", msg);
                return Err(Fail::new(libc::EIO, msg));
            }
            retries -= 1;
        }

        Ok(())
    }

    fn dpdk_allocate_mbuf(&self, size: usize) -> Result<DemiBuffer, Fail> {
        let ptr = self.mem_pool.alloc_mbuf(Some(size))?;
        // Safety: ptr is a valid rte_mbuf from mem_pool
        Ok(unsafe { DemiBuffer::from_mbuf(ptr) })
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
    fn transmit(&mut self, packets: ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>) -> Result<(), Fail> {
        timer!("catnip::runtime::transmit");

        // In general, this copy will happen for small packets without payloads because we allocate actual
        // data-carrying application buffers from the DPDK pool.
        let count = packets.len();
        let mut mbufs: [*mut rte_mbuf; MAX_BATCH_SIZE_NUM_PACKETS] = unsafe { mem::zeroed() };

        for (i, packet) in packets.into_iter().enumerate() {
            mbufs[i] = if packet.is_dpdk_allocated() {
                packet
                    .into_mbuf()
                    .ok_or(Fail::new(libc::EINVAL, "failed to extract DPDK mbuf"))?
            } else if packet.len() <= self.max_body_size {
                let mut mbuf = self.dpdk_allocate_mbuf(packet.len())?;
                debug_assert_eq!(packet.len(), mbuf.len());
                mbuf.copy_from_slice(&packet);
                mbuf.into_mbuf()
                    .ok_or(Fail::new(libc::EINVAL, "failed to convert copied buffer to mbuf"))?
            } else {
                return Err(Fail::new(libc::EINVAL, "packet too large for DPDK buffer"));
            }
        }

        let sent = unsafe { rte_eth_tx_burst(self.port_id, 0, mbufs.as_mut_ptr(), count as u16) };
        debug_assert_eq!(sent, 1);
        Ok(())
    }

    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>, Fail> {
        timer!("catnip::runtime::receive");

        let mut buffers = ArrayVec::new();
        let mut raw_mbufs: [*mut rte_mbuf; MAX_BATCH_SIZE_NUM_PACKETS] = unsafe { mem::zeroed() };

        let count = unsafe {
            rte_eth_rx_burst(
                self.port_id,
                0,
                raw_mbufs.as_mut_ptr(),
                MAX_BATCH_SIZE_NUM_PACKETS as u16,
            )
        };

        assert!(count as usize <= MAX_BATCH_SIZE_NUM_PACKETS);

        for &mbuf in &raw_mbufs[..count as usize] {
            // Safety: `packet` is a valid pointer to a properly initialized `rte_mbuf` struct.
            let buffer = unsafe { DemiBuffer::from_mbuf(mbuf) };
            buffers.push(buffer);
        }

        Ok(buffers)
    }
}

impl DemiMemoryAllocator for SharedDPDKRuntime {
    fn max_buffer_size_bytes(&self) -> usize {
        self.max_body_size
    }

    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        debug_assert!(size < self.max_body_size);
        self.dpdk_allocate_mbuf(size)
    }
}
