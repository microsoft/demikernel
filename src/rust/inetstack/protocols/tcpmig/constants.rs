//======================================================================================================================
// Constants
//======================================================================================================================
use std::time::Duration;
use ::std::net::Ipv4Addr;
use crate::runtime::network::types::MacAddress;

// TEMP
pub const SELF_UDP_PORT: u16 = 10000; // it will be set properly when the socket is binded

pub const TARGET_MAC: MacAddress = MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xc5, 0xad]);
pub const TARGET_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 1, 9);
pub const TARGET_PORT: u16 = 10009;

pub const ORIGIN_MAC: MacAddress = MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xe8, 0x05]);
pub const ORIGIN_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 1, 8);
pub const ORIGIN_PORT: u16 = 10008;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_micros(1000);

pub const HEARTBEAT_MAGIC: u32 = 0xCAFECAFE;