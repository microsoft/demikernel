// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{prelude::*, protocols::ipv4};
use std::num::NonZeroU16;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TcpConnectionId {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display)]
pub struct TcpConnectionHandle(NonZeroU16);

impl TryFrom<u16> for TcpConnectionHandle {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self> {
        if let Some(n) = NonZeroU16::new(n) {
            Ok(TcpConnectionHandle(n))
        } else {
            Err(Fail::OutOfRange {
                details: "TCP connection handles may not be zero",
            })
        }
    }
}

impl Into<u16> for TcpConnectionHandle {
    fn into(self) -> u16 {
        self.0.get()
    }
}
