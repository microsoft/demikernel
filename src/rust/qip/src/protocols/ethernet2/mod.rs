mod frame;
mod header;
mod mac_address;

pub use frame::Ethernet2Frame as Frame;
pub use header::{EtherType, Ethernet2Header as Header};
pub use mac_address::MacAddress;

pub const MIN_PAYLOAD_SIZE: usize = 48;
