// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::protocols::{
    ipv4,
    tcp::SeqNumber,
};
use crc::{
    crc32,
    Hasher32,
};
use std::{
    hash::Hasher,
    num::Wrapping,
};

pub struct IsnGenerator {
    nonce: u32,
    counter: Wrapping<u16>,
}

impl IsnGenerator {
    pub fn new(nonce: u32) -> Self {
        Self {
            nonce,
            counter: Wrapping(0),
        }
    }

    pub fn generate(&mut self, local: &ipv4::Endpoint, remote: &ipv4::Endpoint) -> SeqNumber {
        let mut hash = crc32::Digest::new(crc32::IEEE);
        hash.write_u32(remote.address().into());
        hash.write_u16(remote.port().into());
        hash.write_u32(local.address().into());
        hash.write_u16(local.port().into());
        hash.write_u32(self.nonce);
        let hash = hash.sum32();
        let isn = Wrapping(hash) + Wrapping(u32::from(self.counter.0));
        self.counter += Wrapping(1);
        isn
    }
}
