//======================================================================================================================
// Constants
//======================================================================================================================
use std::time::Duration;
use ::std::net::Ipv4Addr;
use crate::runtime::network::types::MacAddress;

// TEMP
pub const SELF_UDP_PORT: u16 = 10000; // it will be set properly when the socket is binded

pub const TARGET_MAC: MacAddress = MacAddress::new([0x98, 0x5d, 0x82, 0x9c, 0xef, 0xb5]);
pub const TARGET_IP: Ipv4Addr = Ipv4Addr::new(198, 19, 201, 37);
pub const TARGET_PORT: u16 = 10000;

pub const ORIGIN_MAC: MacAddress = MacAddress::new([0x98, 0x5d, 0x82, 0x9c, 0xef, 0xb5]);
pub const ORIGIN_IP: Ipv4Addr = Ipv4Addr::new(198, 19, 201, 36);
pub const ORIGIN_PORT: u16 = 10000;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_micros(1000);

pub const HEARTBEAT_MAGIC: u32 = 0xCAFECAFE;