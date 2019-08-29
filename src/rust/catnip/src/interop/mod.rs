use crate::Options;
use libc;
use std::{net::Ipv4Addr, sync::Mutex};

lazy_static! {
    static ref OPTIONS: Mutex<Options> = Mutex::new(Options::default());
}

#[no_mangle]
pub extern "C" fn nip_set_my_ipv4_addr(ipv4_addr: u32) -> libc::c_int {
    let ipv4_addr = Ipv4Addr::from(ipv4_addr);
    if ipv4_addr.is_unspecified() || ipv4_addr.is_broadcast() {
        return libc::EINVAL;
    }

    let mut options = OPTIONS.lock().unwrap();
    options.my_ipv4_addr = ipv4_addr;
    0
}
