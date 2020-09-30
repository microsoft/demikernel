// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod frame;
mod mac_address;

pub use frame::{
    EtherType,
    Ethernet2Frame as Frame,
    Ethernet2FrameMut as FrameMut,
    Ethernet2Header as Header,
    ETHERNET2_HEADER_SIZE as HEADER_SIZE,
};
pub use mac_address::MacAddress;

#[cfg(test)]
pub use frame::MIN_PAYLOAD_SIZE;
