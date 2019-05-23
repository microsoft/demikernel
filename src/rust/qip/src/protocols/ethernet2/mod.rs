mod frame;
mod header;

pub use frame::Ethernet2Frame as Frame;
pub use header::{EtherType, Ethernet2Header as Header};
