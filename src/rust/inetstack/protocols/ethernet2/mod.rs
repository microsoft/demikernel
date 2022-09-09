// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod frame;
mod protocol;

pub use self::{
    frame::{
        Ethernet2Header,
        ETHERNET2_HEADER_SIZE,
        MIN_PAYLOAD_SIZE,
    },
    protocol::EtherType2,
};
