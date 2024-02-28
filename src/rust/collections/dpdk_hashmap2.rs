//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::{
        fail::Fail,
        libdpdk::{
            rte_hash,
            rte_hash_crc,
            rte_hash_create,
            rte_hash_add_key_with_hash_data,
            rte_hash_lookup_with_hash_data,
            rte_hash_parameters,
            rte_socket_id,
        },
        network::{
            NetworkRuntime,
            socket::SocketId,
        },
    },
    inetstack::protocols::tcp::socket::SharedTcpSocket,
};

use std::{
    hash::{
        Hash, 
        Hasher
    },
    os::raw::c_void,
};

//==============================================================================
// Constants
//==============================================================================

const MAX_ENTRIES: u32 = 128;

//======================================================================================================================
// Structures
//======================================================================================================================
pub struct DPDKHashMap2 {
    hash: *mut rte_hash,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================
impl DPDKHashMap2 {
    pub fn new() -> Self {
        let hash: *mut rte_hash = unsafe {
            fn cast_c(f: unsafe fn(*const c_void, u32, u32) -> u32) -> unsafe extern "C" fn(*const c_void, u32, u32) -> u32 {
                unsafe { std::mem::transmute(f) }
            }

            let params = rte_hash_parameters {
                name: "DPDK HashMap for addresses".as_ptr() as *const i8,
                entries: MAX_ENTRIES,
                reserved: 0,
                key_len: std::mem::size_of::<SocketId>() as u32,
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

        Self { hash }
    }

    pub fn insert<N: NetworkRuntime>(&self, key2: SocketId, value: *mut SharedTcpSocket<N>) -> Result<(), Fail> {
        let hash_value: u32 = {
            let mut state: std::hash::DefaultHasher = std::hash::DefaultHasher::default();
            key2.hash(&mut state);
            state.finish() as u32
        };

        let ret: i32 = unsafe { rte_hash_add_key_with_hash_data(self.hash, &key2 as *const _ as *mut c_void, hash_value, value as *mut c_void) };

        if ret == 0 {
            Ok(())
        } else {
            panic!("Could not allocate")
        }
    }

    pub fn get_mut<N: NetworkRuntime>(&self, key2: SocketId) -> Option<*mut SharedTcpSocket<N>> {
        let hash_value: u32 = {
            let mut state: std::hash::DefaultHasher = std::hash::DefaultHasher::default();
            key2.hash(&mut state);
            state.finish() as u32
        };

        let value: *mut c_void = std::ptr::null_mut();
        let value_ptr: *mut *mut c_void = &value as *const _ as *mut *mut c_void;
        let ret: i32 = unsafe { rte_hash_lookup_with_hash_data(self.hash, &key2 as *const _ as *mut c_void, hash_value, value_ptr) };
        log::warn!("key = {:?} -- hash_value = {:?} -- ret = {:?}", key2, hash_value, ret);

        if ret < 0 {
            None 
        } else {
            Some(unsafe { *value_ptr as *mut SharedTcpSocket<N> })
        }
    }
}
