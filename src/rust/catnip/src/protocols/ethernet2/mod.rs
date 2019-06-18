mod frame;
mod mac_address;

pub use frame::{
    EtherType, Ethernet2Frame as Frame, Ethernet2FrameMut as FrameMut,
    Ethernet2Header as Header, ETHERNET2_HEADER_SIZE as HEADER_SIZE,
};
pub use mac_address::MacAddress;

#[cfg(test)]
pub use frame::MIN_PAYLOAD_SIZE;

#[cfg(not(test))]
use frame::MIN_PAYLOAD_SIZE;

use std::cmp::max;

pub fn new_packet(payload_sz: usize) -> Vec<u8> {
    let payload_sz = max(payload_sz, MIN_PAYLOAD_SIZE);
    vec![0u8; payload_sz + HEADER_SIZE]
}
