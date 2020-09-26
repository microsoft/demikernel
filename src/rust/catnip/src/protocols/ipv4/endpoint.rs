// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::protocols::ip;
use std::net::Ipv4Addr;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ipv4Endpoint {
    address: Ipv4Addr,
    port: ip::Port,
}

impl Ipv4Endpoint {
    pub fn new(address: Ipv4Addr, port: ip::Port) -> Ipv4Endpoint {
        Ipv4Endpoint { address, port }
    }

    pub fn address(&self) -> Ipv4Addr {
        self.address
    }

    pub fn port(&self) -> ip::Port {
        self.port
    }
}
