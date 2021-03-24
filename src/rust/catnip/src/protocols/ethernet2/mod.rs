// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod frame;
mod mac_address;

pub use mac_address::MacAddress;

pub use frame::{EtherType2, Ethernet2Header};

#[cfg(test)]
pub use frame::MIN_PAYLOAD_SIZE;
