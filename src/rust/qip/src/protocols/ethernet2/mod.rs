mod frame;
mod header;
mod mac_address;

pub use frame::{Ethernet2Frame as Frame, Ethernet2FrameMut as FrameMut};
pub use header::{
    EtherType, Ethernet2Header as Header, Ethernet2HeaderMut as HeaderMut,
    ETHERNET2_HEADER_SIZE as HEADER_SIZE,
};
pub use mac_address::MacAddress;
use std::cmp::max;

pub use frame::MIN_PAYLOAD_SIZE;

pub fn alloc(payload_sz: usize) -> Vec<u8> {
    let payload_sz = max(payload_sz, MIN_PAYLOAD_SIZE);
    let packet_len = payload_sz + HEADER_SIZE;
    vec![0u8; packet_len]
}
