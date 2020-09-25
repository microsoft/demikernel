#![allow(non_camel_case_types)]
#![feature(try_blocks)]

mod bindings;

use catnip::Options;
use catnip::protocols::arp;
use catnip::engine::Engine;
use catnip::protocols::ethernet2::MacAddress;
use catnip::interop::fail_to_errno;
use catnip::logging;
use std::time::{Duration, Instant};
use std::slice;
use std::fs::File;
use std::mem::MaybeUninit;
use std::net::Ipv4Addr;
use std::io::Read;
use std::cell::RefCell;
use clap::{App, Arg};
use std::ffi::{CStr, CString};
use yaml_rust::{YamlLoader, Yaml};
use anyhow::{Error, bail, format_err};
use self::bindings::{
    rte_ether_addr,
    rte_eth_dev_is_valid_port,
    rte_eth_macaddr_get,
    ETH_LINK_UP,
    ETH_LINK_FULL_DUPLEX,
    rte_delay_us_block,
    rte_eth_dev_flow_ctrl_get,
    rte_eth_link_get_nowait,
    rte_eth_link,
    rte_eth_dev_flow_ctrl_set,
    rte_eth_fc_conf,
    rte_eth_dev_start,
    rte_eth_rx_queue_setup,
    rte_eth_tx_queue_setup,
    rte_eth_promiscuous_enable,
    RTE_ETH_DEV_NO_OWNER,
    rte_eth_fc_mode_RTE_FC_NONE as RTE_FC_NONE,
    RTE_MAX_ETHPORTS,
    RTE_ETHER_MAX_LEN,
    rte_eth_dev_configure,
    rte_eth_conf,
    rte_eth_rxconf,
    rte_eth_txconf,
    rte_eth_find_next_owned_by,
    rte_mempool,
    rte_eal_init,
    rte_eth_dev_count_avail,
    rte_pktmbuf_pool_create,
    RTE_MBUF_DEFAULT_BUF_SIZE,
    rte_socket_id,
    rte_eth_dev_info,
    rte_eth_dev_info_get,
    rte_eth_rx_mq_mode_ETH_MQ_RX_RSS as ETH_MQ_RX_RSS,
    ETH_RSS_IP,
    rte_eth_tx_mq_mode_ETH_MQ_TX_NONE as ETH_MQ_TX_NONE,
};

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

// TODO: Investigate using bindgen to avoid the copy paste here.
use libc::{
    c_int,
    c_char,
    sockaddr,
    socklen_t,
    mode_t,
    c_void,
    sockaddr_in,
};

pub type dmtr_qtoken_t = u64;

#[derive(Copy, Clone)]
pub struct dmtr_sgaseg_t {
    sgaseg_buf: *mut c_void,
    sgaseg_len: u32,
}

pub const DMTR_SGARRAY_MAXSIZE: usize = 1;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct dmtr_sgarray_t {
    sga_buf: *mut c_void,
    sga_numsegs: u32,
    sga_segs: [dmtr_sgaseg_t; DMTR_SGARRAY_MAXSIZE],
    sga_addr: sockaddr_in,
}

#[repr(C)]
pub enum dmtr_opcode_t {
    DMTR_OPC_INVALID = 0,
    DMTR_OPC_PUSH,
    DMTR_OPC_POP,
    DMTR_OPC_ACCEPT,
    DMTR_OPC_CONNECT,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct dmtr_accept_result_t {
    qd: c_int,
    addr: sockaddr_in,
}

#[repr(C)]
union dmtr_qr_value_t {
    sga: dmtr_sgarray_t,
    ares: dmtr_accept_result_t,
}

#[repr(C)]
pub struct dmtr_qresult_t {
    qr_opcode: dmtr_opcode_t,
    qr_qd: c_int,
    qr_qt: dmtr_qtoken_t,
    qr_value: dmtr_qr_value_t,
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = RefCell::new(None);
}
fn with_engine<T>(f: impl FnOnce(&mut Engine) -> T) -> T {
    ENGINE.with(|e| {
        let mut tls_engine = e.borrow_mut();
        f(tls_engine.as_mut().expect("Uninitialized engine"))
    })
}

#[no_mangle]
pub extern "C" fn catnip_libos_noop() {
    println!("hey there!");
}

#[no_mangle]
pub extern "C" fn dmtr_init(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let r: Result<_, Error> = try {
        if argc == 0 || argv.is_null() {
            Err(format_err!("Arguments not provided"))?;
        }
        let argument_ptrs = unsafe { slice::from_raw_parts(argv, argc as usize) };
        let arguments: Vec<_> = argument_ptrs.into_iter()
            .map(|&p| unsafe { CStr::from_ptr(p).to_str().expect("Non-UTF8 argument") })
            .collect();

        let matches = App::new("libos-catnip")
            .arg(
                Arg::with_name("config")
                    .short("c")
                    .long("config-path")
                    .value_name("FILE")
                    .help("YAML file for DPDK configuration")
                    .takes_value(true)
            )
            .get_matches_from(&arguments);

        let config_path = matches.value_of("config")
            .ok_or_else(|| format_err!("--config-path argument not provided"))?;

        let mut config_s = String::new();
        File::open(config_path)?.read_to_string(&mut config_s)?;
        let config = YamlLoader::load_from_str(&config_s)?;

        let config_obj = match &config[..] {
            &[ref c] => c,
            _ => Err(format_err!("Wrong number of config objects"))?,
        };

        let local_ipv4_addr: Ipv4Addr = config_obj["catnip"]["my_ipv4_addr"]
            .as_str()
            .ok_or_else(|| format_err!("Couldn't find my_ipv4_addr in config"))?
            .parse()?;
        if local_ipv4_addr.is_unspecified() || local_ipv4_addr.is_broadcast() {
            Err(format_err!("Invalid IPv4 address"))?;
        }

        let eal_init_args = match config_obj["dpdk"]["eal_init"] {
            Yaml::Array(ref arr) => {
                arr.iter()
                    .map(|a| {
                        a.as_str()
                            .ok_or_else(|| format_err!("Non string argument"))
                            .and_then(|s| CString::new(s).map_err(|e| e.into()))
                    })
                    .collect::<Result<Vec<_>, Error>>()?
            },
            _ => Err(format_err!("Malformed YAML config"))?,
        };

        let unused = -1;
        let eal_init_refs = eal_init_args.iter()
            .map(|s| s.as_ptr() as *mut u8)
            .collect::<Vec<_>>();
        unsafe {
            self::bindings::rte_eal_init(eal_init_refs.len() as i32, eal_init_refs.as_ptr() as *mut _);
        }
        let nb_ports = unsafe { self::bindings::rte_eth_dev_count_avail() };
        if nb_ports == 0 {
            Err(format_err!("No ethernet ports available"))?;
        }
        eprintln!("DPDK reports that {} ports (interfaces) are available.", nb_ports);

        let name = CString::new("default_mbuf_pool").unwrap();
        let num_mbufs = 8191;
        let mbuf_cache_size = 250;
        let mbuf_pool = unsafe {
            rte_pktmbuf_pool_create(
                name.as_ptr(),
                (num_mbufs * nb_ports) as u32,
                mbuf_cache_size,
                0,
                RTE_MBUF_DEFAULT_BUF_SIZE as u16,
                rte_socket_id() as i32,
            )
        };
        if mbuf_pool.is_null() {
            Err(format_err!("rte_pktmbuf_pool_create failed"))?;
        }

        let mut port_id = 0;
        {
            let owner = RTE_ETH_DEV_NO_OWNER as u64;
            let mut p = unsafe { rte_eth_find_next_owned_by(0, owner) as u16 };

            while p < RTE_MAX_ETHPORTS as u16 {
                // TODO: This is pretty hax, we clearly only support one port.
                port_id = p;
                init_dpdk_port(p, mbuf_pool)?;
                p = unsafe { rte_eth_find_next_owned_by(p + 1, owner) as u16 };
            }
        }

        // TODO: Where is this function?
        // if unsafe { rte_lcore_count() } > 1 {
        //     eprintln!("WARNING: Too many lcores enabled. Only 1 used.");
        // }

        let local_link_addr = unsafe {
            let mut m: MaybeUninit<rte_ether_addr> = MaybeUninit::zeroed();
            // TODO: Why does bindgen say this function doesn't return an int?
            rte_eth_macaddr_get(port_id, m.as_mut_ptr());
            MacAddress::new(m.assume_init().addr_bytes)
        };
        if local_link_addr.is_nil() || !local_link_addr.is_unicast() {
            Err(format_err!("Invalid mac address"))?;
        }

        logging::initialize();

        let mut options = Options::default();
        options.my_ipv4_addr = local_ipv4_addr;
        options.my_link_addr = local_link_addr;
        Engine::from_options(Instant::now(), options)?
    };
    let engine = match r {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Initialization failure: {:?}", e);
            return libc::EINVAL;
        },
    };

    ENGINE.with(move |e| {
        let mut tls_engine = e.borrow_mut();
        assert!(tls_engine.is_none());
        *tls_engine = Some(engine);
    });

    0
}

fn init_dpdk_port(port_id: u16, mbuf_pool: *mut rte_mempool) -> Result<(), Error> {
    let rx_rings = 1;
    let tx_rings = 1;
    let rx_ring_size = 128;
    let tx_ring_size = 512;
    let nb_rxd = rx_ring_size;
    let nb_txd = tx_ring_size;

    let rx_pthresh = 0;
    let rx_hthresh = 0;
    let rx_wthresh = 0;

    let tx_pthresh = 0;
    let tx_hthresh = 0;
    let tx_wthresh = 0;

    let dev_info = unsafe {
        let mut d = MaybeUninit::zeroed();
        rte_eth_dev_info_get(port_id, d.as_mut_ptr());
        d.assume_init()
    };

    let mut port_conf: rte_eth_conf = unsafe { MaybeUninit::zeroed().assume_init() };
    port_conf.rxmode.max_rx_pkt_len = RTE_ETHER_MAX_LEN;
    port_conf.rxmode.mq_mode = ETH_MQ_RX_RSS;
    port_conf.rx_adv_conf.rss_conf.rss_hf = ETH_RSS_IP as u64 | dev_info.flow_type_rss_offloads;
    port_conf.txmode.mq_mode = ETH_MQ_TX_NONE;

    let mut rx_conf: rte_eth_rxconf = unsafe { MaybeUninit::zeroed().assume_init() };
    rx_conf.rx_thresh.pthresh = rx_pthresh;
    rx_conf.rx_thresh.hthresh = rx_hthresh;
    rx_conf.rx_thresh.wthresh = rx_wthresh;
    rx_conf.rx_free_thresh = 32;

    let mut tx_conf: rte_eth_txconf = unsafe { MaybeUninit::zeroed().assume_init() };
    tx_conf.tx_thresh.pthresh = tx_pthresh;
    tx_conf.tx_thresh.hthresh = tx_hthresh;
    tx_conf.tx_thresh.wthresh = tx_wthresh;

    unsafe {
        expect_zero!(
            rte_eth_dev_configure(
                port_id,
                rx_rings,
                tx_rings,
                &port_conf as *const _,
            )
        )?;
    }

    let socket_id = 0;

    unsafe {
        for i in 0..rx_rings {
            expect_zero!(rte_eth_rx_queue_setup(port_id, i, nb_rxd, socket_id, &rx_conf as *const _, mbuf_pool))?;
        }
        for i in 0..tx_rings {
            expect_zero!(rte_eth_tx_queue_setup(port_id, i, nb_txd, socket_id, &tx_conf as *const _))?;
        }
        expect_zero!(rte_eth_dev_start(port_id))?;
        rte_eth_promiscuous_enable(port_id);
    }

    let mut fc_conf = unsafe {
        let mut f = MaybeUninit::zeroed();
        expect_zero!(rte_eth_dev_flow_ctrl_get(port_id, f.as_mut_ptr()))?;
        f.assume_init()
    };
    fc_conf.mode = RTE_FC_NONE;
    unsafe { expect_zero!(rte_eth_dev_flow_ctrl_set(port_id, &mut fc_conf as *mut _))? };

    if unsafe { rte_eth_dev_is_valid_port(port_id) } == 0 {
        bail!("Invalid port");
    }

    let sleep_duration = Duration::from_millis(100);
    let mut retry_count = 90;

    loop {
        unsafe {
            let mut link: MaybeUninit<rte_eth_link> = MaybeUninit::zeroed();
            rte_eth_link_get_nowait(port_id, link.as_mut_ptr());
            let link = link.assume_init();
            if link.link_status() as u32 == ETH_LINK_UP {
                let duplex = if link.link_duplex() as u32 == ETH_LINK_FULL_DUPLEX {
                    "full"
                } else {
                    "half"
                };
                eprintln!("Port {} Link Up - speed {} Mbps - {} duplex", port_id, link.link_speed, duplex);
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

#[no_mangle]
pub extern "C" fn dmtr_socket(qd_out: *mut c_int, domain: c_int, socket_type: c_int, protocol: c_int) -> c_int {
    // Does nothing?
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_listen(fd: c_int, backlog: c_int) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_bind(qd: c_int, saddr: *const sockaddr, size: socklen_t) -> c_int {
    // Sets some global shiz
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_accept(qtok_out: *mut dmtr_qtoken_t, sockqd: c_int) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_connect(qt_out: *mut dmtr_qtoken_t, qd: c_int, saddr: *const sockaddr, size: socklen_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_close(qd: c_int) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_push(qt_out: *mut dmtr_qtoken_t, qd: c_int, sga: *const dmtr_sgarray_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_pop(qt_out: *mut dmtr_qtoken_t, qd: c_int) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_poll(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_drop(qt: dmtr_qtoken_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_wait(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_wait_any(qr_out: *mut dmtr_qresult_t, ready_offset: *mut c_int, qts: *mut dmtr_qtoken_t, num_qts: c_int) -> c_int {
    todo!()
}

// #[no_mangle]
// pub extern "C" fn dmtr_queue(qd_out: *mut c_int) -> c_int {
//     unimplemented!()
// }

// #[no_mangle]
// pub extern "C" fn dmtr_is_qd_valid(flag_out: *mut c_int, qd: c_int) -> c_int {
//     unimplemented!()
// }

// #[no_mangle]
// pub extern "C" fn dmtr_getsockname(qd: c_int, saddr: *mut sockaddr, size: *mut socklen_t) -> c_int {
//     unimplemented!();
// }
