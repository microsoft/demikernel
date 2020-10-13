// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::protocols::ip;
use std::net::Ipv4Addr;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ipv4Endpoint {
    pub addr: Ipv4Addr,
    pub port: ip::Port,
}

impl Ipv4Endpoint {
    pub fn new(addr: Ipv4Addr, port: ip::Port) -> Ipv4Endpoint {
        Ipv4Endpoint { addr, port }
    }

    pub fn address(&self) -> Ipv4Addr {
        self.addr
    }

    pub fn port(&self) -> ip::Port {
        self.port
    }
}
