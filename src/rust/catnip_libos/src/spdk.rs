use anyhow::Error;
use crate::runtime::DPDKRuntime;
use std::mem;
use spdk_rs::{
    spdk_env_dpdk_init,
    spdk_env_dpdk_external_init,
    spdk_env_opts,
};

pub fn initialize_spdk(dpdk_rt: &DPDKRuntime) -> Result<(), Error> {
    let mut opts: spdk_env_opts = unsafe { mem::zeroed() };
    unsafe {
        assert_eq!(spdk_env_dpdk_external_init(), 1);
        assert_eq!(spdk_env_dpdk_init(0), 0);
    }

    todo!();
}
