use super::connection::TcpConnectionId;
use crate::prelude::*;
use crc::{crc32, Hasher32};
use rand::Rng;
use std::{hash::Hasher, num::Wrapping};

pub struct IsnGenerator {
    nonce: u32,
    counter: Wrapping<u16>,
}

impl IsnGenerator {
    pub fn new<'a>(rt: &Runtime<'a>) -> IsnGenerator {
        IsnGenerator {
            nonce: rt.borrow_rng().gen(),
            counter: Wrapping(0),
        }
    }

    pub fn next(&mut self, cxn_id: &TcpConnectionId) -> Wrapping<u32> {
        let mut hash = crc32::Digest::new(crc32::IEEE);
        hash.write_u32(cxn_id.remote.address().into());
        hash.write_u16(cxn_id.remote.port().into());
        hash.write_u32(cxn_id.local.address().into());
        hash.write_u16(cxn_id.local.port().into());
        hash.write_u32(self.nonce);
        let hash = hash.sum32();
        let isn = Wrapping(hash) + Wrapping(u32::from(self.counter.0));
        self.counter += Wrapping(1);
        isn
    }
}
