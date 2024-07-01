// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod ethernet2;

pub use self::ethernet2::{
    frame::{
        Ethernet2Header,
        ETHERNET2_HEADER_SIZE,
        MIN_PAYLOAD_SIZE,
    },
    protocol::EtherType2,
};

use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
    MacAddress,
};
pub struct Layer2Endpoint {
    local_link_addr: MacAddress,
}

impl Layer2Endpoint {
    pub fn new(config: &Config) -> Result<Self, Fail> {
        Ok(Self {
            local_link_addr: config.local_link_addr()?,
        })
    }

    pub fn receive(&mut self, pkt: DemiBuffer) -> Result<(Ethernet2Header, DemiBuffer), Fail> {
        let (header, payload) = Ethernet2Header::parse(pkt)?;
        debug!("Engine received {:?}", header);
        if self.local_link_addr != header.dst_addr()
            && !header.dst_addr().is_broadcast()
            && !header.dst_addr().is_multicast()
        {
            let cause: &str = "invalid link address";
            warn!("dropping packet: {}", cause);
            return Err(Fail::new(libc::EADDRNOTAVAIL, &cause));
        }
        Ok((header, payload))
    }

    pub fn get_link_addr(&self) -> MacAddress {
        self.local_link_addr
    }
}
