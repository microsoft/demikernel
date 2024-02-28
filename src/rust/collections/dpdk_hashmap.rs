//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    QDesc,
    pal::arch,
    runtime::{
        fail::Fail,
        libdpdk::{
            rte_hash,
            rte_hash_crc,
            rte_hash_create,
            rte_hash_add_key_data,
            rte_hash_lookup_data,
            rte_hash_parameters,
            rte_socket_id,
            rte_zmalloc,
        },
        network::NetworkRuntime,
    },
    inetstack::protocols::tcp::established::ctrlblk::SharedControlBlock,
};

use std::os::raw::c_void;

//==============================================================================
// Constants
//==============================================================================

const BASE_QD: u32 = 500;
const MAX_ENTRIES: u32 = 128;

//======================================================================================================================
// Structures
//======================================================================================================================
pub struct DPDKHashMap {
    hash: *mut rte_hash,
    keys: *mut QDesc,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================
impl DPDKHashMap {
    pub fn new() -> Self {
        let hash: *mut rte_hash = unsafe {
            fn cast_c(f: unsafe fn(*const c_void, u32, u32) -> u32) -> unsafe extern "C" fn(*const c_void, u32, u32) -> u32 {
                unsafe { std::mem::transmute(f) }
            }

            let params = rte_hash_parameters {
                name: "DPDK HashMap for QDesc".as_ptr() as *const i8,
                entries: MAX_ENTRIES,
                reserved: 0,
                key_len: std::mem::size_of::<QDesc>() as u32,
                hash_func: Some(cast_c(rte_hash_crc)),
                hash_func_init_val: 0,
                socket_id: rte_socket_id() as i32,
                extra_flag: 0,
            };

            let ptr = rte_hash_create(&params);

            if ptr.is_null() {
                panic!("Could not allocate");
            }

            ptr
        };

        let keys: *mut QDesc = unsafe {
            let ptr: *mut QDesc = rte_zmalloc("DPDK HashMap keys".as_ptr() as *const i8, std::mem::size_of::<QDesc>() * MAX_ENTRIES as usize, arch::CPU_DATA_CACHE_LINE_SIZE.try_into().unwrap()) as *mut QDesc;

            for i in 0..MAX_ENTRIES as usize {
                *(ptr.offset(i as isize)) = QDesc::from(i as u32);
            }

            ptr
        };

        Self { hash, keys }
    }

    pub fn insert<T: NetworkRuntime>(&self, key: QDesc, value: *mut SharedControlBlock<T>) -> Result<(), Fail> {
        let idx: isize = (u32::try_from(key).unwrap() - BASE_QD) as isize;
        let key_ptr: *const c_void = unsafe { self.keys.offset(idx) as *const c_void };
        let _ret: i32 = unsafe { rte_hash_add_key_data(self.hash, key_ptr, value as *mut c_void) };

        Ok(())
    }

    pub fn get_mut<T: NetworkRuntime>(&self, key: QDesc) -> Option<*mut SharedControlBlock<T>> {
        let idx: isize = (u32::try_from(key).unwrap() - BASE_QD) as isize;
        let key_ptr: *const c_void = unsafe { self.keys.offset(idx) as *const c_void };
        let value: *mut c_void = std::ptr::null_mut();
        let value_ptr: *mut *mut c_void = &value as *const _ as *mut *mut c_void;
        let _ret: i32 = unsafe { rte_hash_lookup_data(self.hash, key_ptr, value_ptr) };

        Some(unsafe { *value_ptr as *mut SharedControlBlock<T> })
    }
}
