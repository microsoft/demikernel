// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(test)]
mod tests;

use super::datagram::{
    Icmpv4Datagram,
    Icmpv4DatagramMut,
    Icmpv4Type,
};
use crate::fail::Fail;
use byteorder::{
    ByteOrder,
    NetworkEndian,
};
use std::convert::TryFrom;

#[derive(Debug, PartialEq, Eq)]
pub enum Icmpv4EchoOp {
    Request,
    Reply,
}

pub struct Icmpv4Echo<'a>(Icmpv4Datagram<'a>);

impl<'a> Icmpv4Echo<'a> {
    pub fn new_vec() -> Vec<u8> {
        Icmpv4Datagram::new_vec(4)
    }

    #[allow(dead_code)]
    pub fn attach(bytes: &'a [u8]) -> Result<Self, Fail> {
        Ok(Icmpv4Echo::try_from(Icmpv4Datagram::attach(bytes)?)?)
    }

    #[allow(dead_code)]
    pub fn icmpv4(&self) -> &Icmpv4Datagram<'a> {
        &self.0
    }

    #[allow(dead_code)]
    pub fn op(&self) -> Icmpv4EchoOp {
        // precondition: we've ensured the call to `r#type()` will succeed in
        // the implementation of `try_from()`.
        #[allow(unreachable_patterns)]
        match self.0.header().r#type().unwrap() {
            Icmpv4Type::EchoRequest => Icmpv4EchoOp::Request,
            Icmpv4Type::EchoReply => Icmpv4EchoOp::Reply,
            _ => panic!("unexpected ICMPv4 type"),
        }
    }

    pub fn id(&self) -> u16 {
        NetworkEndian::read_u16(&self.0.text()[..2])
    }

    pub fn seq_num(&self) -> u16 {
        NetworkEndian::read_u16(&self.0.text()[2..4])
    }
}

impl<'a> TryFrom<Icmpv4Datagram<'a>> for Icmpv4Echo<'a> {
    type Error = Fail;

    fn try_from(datagram: Icmpv4Datagram<'a>) -> Result<Self, Fail> {
        trace!("Icmpv4Datagram::try_from()");
        let r#type = datagram.header().r#type()?;
        assert!(r#type == Icmpv4Type::EchoRequest || r#type == Icmpv4Type::EchoReply);
        Ok(Icmpv4Echo(datagram))
    }
}

pub struct Icmpv4EchoMut<'a>(Icmpv4DatagramMut<'a>);

impl<'a> Icmpv4EchoMut<'a> {
    pub fn attach(bytes: &'a mut [u8]) -> Self {
        Icmpv4EchoMut(Icmpv4DatagramMut::attach(bytes))
    }

    pub fn icmpv4(&mut self) -> &mut Icmpv4DatagramMut<'a> {
        &mut self.0
    }

    pub fn r#type(&mut self, value: Icmpv4EchoOp) {
        match value {
            Icmpv4EchoOp::Request => self.0.header().r#type(Icmpv4Type::EchoRequest),
            Icmpv4EchoOp::Reply => self.0.header().r#type(Icmpv4Type::EchoReply),
        }
    }

    pub fn id(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0.text()[..2], value)
    }

    pub fn seq_num(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0.text()[2..], value)
    }

    #[allow(dead_code)]
    pub fn unmut(&self) -> Icmpv4Echo<'_> {
        Icmpv4Echo(self.0.unmut())
    }

    pub fn seal(self) -> Result<Icmpv4Echo<'a>, Fail> {
        trace!("Icmpv4EchoMut::seal()");
        Ok(Icmpv4Echo::try_from(self.0.seal()?)?)
    }
}

impl<'a> From<Icmpv4DatagramMut<'a>> for Icmpv4EchoMut<'a> {
    fn from(datagram: Icmpv4DatagramMut<'a>) -> Self {
        Icmpv4EchoMut(datagram)
    }
}
