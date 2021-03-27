use anyhow::Error;
use crate::runtime::DPDKRuntime;
use std::mem;
use std::ptr;
use std::slice;
use spdk_rs::*;
use libc::c_void;
use std::ffi::{CStr, CString};

pub struct SPDKConfig {
    initialized: bool,

    namespace_id: u32,
    
    namespace: *mut spdk_nvme_ns,
    qpair: *mut spdk_nvme_qpair,

    namespace_size: usize,
    sector_size: usize,

    partial_block_usage: usize,
    partial_block: Box<[u8]>,

    log_offset: usize,
}

impl SPDKConfig {
    pub fn push(&mut self, buf: &[u8]) {
        // Copied from spdk_queue.cc
        let partial_block_size = (self.partial_block_usage + buf.len()) % self.sector_size;
        let mut num_blocks = (buf.len() + self.partial_block_usage - partial_block_size) / self.sector_size;
        if partial_block_size > 0 {
            num_blocks += 1;
        }

        let alloc_size = num_blocks * self.sector_size;
        let out_ptr = unsafe { 
            spdk_malloc(
                alloc_size as u64, 
                0x200, 
                ptr::null_mut(),
                0,
                SPDK_MALLOC_DMA,
            )
        };
        assert!(!out_ptr.is_null());
        let out_buf = unsafe { slice::from_raw_parts_mut(out_ptr as *mut u8, alloc_size) };

        let mut pos = 0;
        if self.partial_block_usage > 0 {
            out_buf[..self.partial_block_usage].copy_from_slice(&self.partial_block[..self.partial_block_usage]);
            pos += self.partial_block_usage;
        }
        out_buf[pos..(pos + buf.len())].copy_from_slice(buf);
        
        if self.log_offset * self.sector_size + buf.len() > self.namespace_size {
            panic!("Out of space");
        }
        
        self.partial_block_usage = partial_block_size;
        // if self.partial_block_usage > 0 {
        //     self.partial_block[..partial_block_size].copy_from_slice(&buf[((num_blocks - 1) * self.sector_size)..]);
        // }
        drop(out_buf);

        let ret = unsafe { 
            spdk_nvme_ns_cmd_write(
                self.namespace,
                self.qpair,
                out_ptr,
                self.log_offset as u64,
                num_blocks as u32,
                None,
                ptr::null_mut(),
                0,
            )
        };
        assert_eq!(ret, 0);

        self.log_offset += num_blocks;
        if self.partial_block_usage > 0 {
            self.log_offset -= 1;
        }

        loop {
            let ret = unsafe { spdk_nvme_qpair_process_completions(self.qpair, 1) };
            if ret != 0 {
                break;
            }
        }

        unsafe { spdk_free(out_ptr) };
    }
}

extern "C" fn probe_cb(cb_ctx: *mut c_void, trid: *const spdk_nvme_transport_id, opts: *mut spdk_nvme_ctrlr_opts) -> bool {
    println!("Probing NVMe device: {}", unsafe { CStr::from_ptr(&(*trid).traddr[0] as *const _).to_string_lossy() });
    true
}

extern "C" fn attach_cb(cb_ctx: *mut c_void, trid: *const spdk_nvme_transport_id, ctrlr: *mut spdk_nvme_ctrlr, opts: *const spdk_nvme_ctrlr_opts) {
    println!("Attaching NVMe device: {}", unsafe { CStr::from_ptr(&(*trid).traddr[0] as *const _).to_string_lossy() });
    unsafe {
         let config = &mut *(cb_ctx as *mut SPDKConfig);
        assert!(!config.initialized);

        let namespace = spdk_nvme_ctrlr_get_ns(ctrlr, config.namespace_id);
        assert!(!namespace.is_null());
        config.namespace = namespace;

        assert!(spdk_nvme_ns_is_active(config.namespace));

        let mut qpopts = mem::zeroed();
        let qpopts_sz = mem::size_of::<spdk_nvme_io_qpair_opts>() as u64;
        spdk_nvme_ctrlr_get_default_io_qpair_opts(ctrlr, &mut qpopts as *mut _, qpopts_sz);
        // TODO: Add queue configuration here.

        let qpair = spdk_nvme_ctrlr_alloc_io_qpair(ctrlr, &qpopts as *const _, qpopts_sz);
        assert!(!qpair.is_null());
        config.qpair = qpair;

        let namespace_size = spdk_nvme_ns_get_size(config.namespace);
        assert!(namespace_size > 0);
        config.namespace_size = namespace_size as usize;

        let sector_size = spdk_nvme_ns_get_sector_size(config.namespace);
        assert!(sector_size > 0);
        config.sector_size = sector_size as usize;

        config.partial_block = Box::new_zeroed_slice(config.sector_size).assume_init();

        println!("Finished initialization!");
        println!("Namespace size: {}", namespace_size);
        println!("Sector size:    {}", sector_size);

        config.initialized = true;
    }
}

pub fn initialize_spdk(dev_address: &str, namespace_id: u32) -> Result<SPDKConfig, Error> {
    unsafe {
        spdk_rs::load_pcie_driver();
        assert!(spdk_env_dpdk_external_init(), "DPDK not initialized");
        let legacy_mem = false;
        assert_eq!(spdk_env_dpdk_post_init(legacy_mem), 0);

        let trinfo = CString::new(format!("trtype=PCIe traddr={}", dev_address))?;
        let mut trid: spdk_nvme_transport_id = mem::zeroed();
        trid.trtype = SPDK_NVME_TRANSPORT_PCIE;
        
        let ret = spdk_nvme_transport_id_parse(&mut trid as *mut _, trinfo.as_ptr());
        assert_eq!(ret, 0);
        assert_eq!(trid.trtype, SPDK_NVME_TRANSPORT_PCIE);

        let mut pci_addr = mem::zeroed();
        let ret = spdk_pci_addr_parse(&mut pci_addr as *mut _, &trid.traddr[0]);
        assert_eq!(ret, 0);

        spdk_pci_addr_fmt(&mut trid.traddr[0] as *mut _, mem::size_of_val(&trid.traddr) as u64, &pci_addr as *const _);

        let mut config = SPDKConfig {
            initialized: false,
            namespace_id,
            namespace: ptr::null_mut(),
            qpair: ptr::null_mut(),
            namespace_size: 0,
            sector_size: 0,
            partial_block_usage: 0,
            partial_block: Box::new([]),
            log_offset: 0,
        };

        assert_eq!(spdk_vmd_init(), 0);
        let ret = spdk_nvme_probe(
            &trid as *const _,
            &mut config as *mut _ as *mut _,
            Some(probe_cb),
            Some(attach_cb),
            None,
        );
        assert_eq!(ret, 0);
        assert!(config.initialized);

        Ok(config)
    }
}

#[test]
#[ignore]
fn test_spdk() {
    use std::net::Ipv4Addr;
    use std::collections::HashMap;

    spdk_rs::load_pcie_driver();

    let eal_init_args = vec![
        CString::new("-c").unwrap(),
        CString::new("0xff").unwrap(),
        CString::new("-n").unwrap(),
        CString::new("4").unwrap(),
        CString::new("-w").unwrap(),
        CString::new("37:00.0").unwrap(),
        CString::new("--proc-type=auto").unwrap(),
    ];

    let runtime = crate::dpdk::initialize_dpdk(
        "198.19.200.3".parse().unwrap(),
        &eal_init_args[..],
        HashMap::new(),
        true,
        false,
        9216,
        9000,
        false,
        false,
    ).unwrap();
    let mut config = initialize_spdk("86:00.0", 1).unwrap();
    config.push(&[0, 1, 2, 3, 4, 5, 6, 7]);

}
