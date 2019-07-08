// we don't include the prelude here to avoid circular dependencies
use super::datagram::{
    Icmpv4Datagram, Icmpv4DatagramMut, Icmpv4Header, Icmpv4Type,
};
use crate::{prelude::*, protocols::ipv4};
use byteorder::{ByteOrder, NetworkEndian};
use num_traits::FromPrimitive;
use std::{convert::TryFrom, fmt, io::Write};

#[repr(u8)]
#[derive(FromPrimitive, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icmpv4DestinationUnreachable {
    DestinationNetworkUnreachable = 0,
    DestinationHostUnreachable = 1,
    DestinationProtocolUnreachable = 2,
    DestinationPortUnreachable = 3,
    FragmentationRequired = 4,
    SourceRouteFailed = 5,
    DestinationNetworkUnknown = 6,
    DestinationHostUnknown = 7,
    SourceHostIsolated = 8,
    NetworkAdministrativelyProhibited = 9,
    HostAdministrativelyProhibited = 10,
    NetworkUnreachableForTos = 11,
    HostUnreachableForTos = 12,
    CommunicationAdministrativelyProhibited = 13,
    HostPrecedenceViolation = 14,
    PrecedenceCutoffInEffect = 15,
}

impl Icmpv4DestinationUnreachable {
    fn description(&self) -> &str {
        match self {
            Icmpv4DestinationUnreachable::DestinationNetworkUnreachable => "destination network unreachable",
            Icmpv4DestinationUnreachable::DestinationHostUnreachable => "destination host unreachable",
            Icmpv4DestinationUnreachable::DestinationProtocolUnreachable => "destination protocol unreachable",
            Icmpv4DestinationUnreachable::DestinationPortUnreachable => "destination port unreachable",
            Icmpv4DestinationUnreachable::FragmentationRequired => "fragmentation required and DF flag set",
            Icmpv4DestinationUnreachable::SourceRouteFailed => "source route failed",
            Icmpv4DestinationUnreachable::DestinationNetworkUnknown => "destination network unknown",
            Icmpv4DestinationUnreachable::DestinationHostUnknown => "destination host unknown",
            Icmpv4DestinationUnreachable::SourceHostIsolated => "source host isolated",
            Icmpv4DestinationUnreachable::NetworkAdministrativelyProhibited => "network administratively prohibited",
            Icmpv4DestinationUnreachable::HostAdministrativelyProhibited => "host administratively prohibited",
            Icmpv4DestinationUnreachable::NetworkUnreachableForTos => "network unreachable for ToS",
            Icmpv4DestinationUnreachable::HostUnreachableForTos => "host unreachable for ToS",
            Icmpv4DestinationUnreachable::CommunicationAdministrativelyProhibited => "communication administratively prohibited",
            Icmpv4DestinationUnreachable::HostPrecedenceViolation => "host precedence violation",
            Icmpv4DestinationUnreachable::PrecedenceCutoffInEffect => "precedence cutoff in effect",
        }
    }
}

impl fmt::Display for Icmpv4DestinationUnreachable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl From<u8> for Icmpv4DestinationUnreachable {
    fn from(n: u8) -> Self {
        FromPrimitive::from_u8(n).unwrap()
    }
}

impl Into<u8> for Icmpv4DestinationUnreachable {
    fn into(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icmpv4ErrorId {
    DestinationUnreachable(Icmpv4DestinationUnreachable),
}

impl Icmpv4ErrorId {
    fn description(&self) -> &str {
        match self {
            Icmpv4ErrorId::DestinationUnreachable(code) => code.description(),
        }
    }
}

impl fmt::Display for Icmpv4ErrorId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl Icmpv4ErrorId {
    fn decode(header: &Icmpv4Header<'_>) -> Result<Icmpv4ErrorId> {
        match header.r#type()? {
            Icmpv4Type::DestinationUnreachable => {
                Ok(Icmpv4ErrorId::DestinationUnreachable(
                    Icmpv4DestinationUnreachable::from(header.code()),
                ))
            }
            _ => Err(Fail::Unsupported {
                details: "Icmpv4ErrorId is intended only for ICMPv4 error \
                          packets",
            }),
        }
    }

    fn encode(self) -> (Icmpv4Type, u8) {
        match self {
            Icmpv4ErrorId::DestinationUnreachable(code) => {
                (Icmpv4Type::DestinationUnreachable, code.into())
            }
        }
    }
}

pub struct Icmpv4Error<'a>(Icmpv4Datagram<'a>);

impl<'a> Icmpv4Error<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        Ok(Icmpv4Error::try_from(Icmpv4Datagram::from_bytes(bytes)?)?)
    }

    pub fn icmpv4(&self) -> &Icmpv4Datagram<'a> {
        &self.0
    }

    pub fn id(&self) -> Icmpv4ErrorId {
        // the ID was already validated in `try_from()`.
        Icmpv4ErrorId::decode(&self.0.header()).unwrap()
    }

    pub fn next_hop_mtu(&self) -> u16 {
        NetworkEndian::read_u16(&self.0.text()[2..4])
    }

    pub fn context(&self) -> &[u8] {
        &self.0.text()[4..]
    }
}

impl<'a> TryFrom<Icmpv4Datagram<'a>> for Icmpv4Error<'a> {
    type Error = Fail;

    fn try_from(datagram: Icmpv4Datagram<'a>) -> Result<Self> {
        let header = datagram.header();
        match header.r#type()? {
            Icmpv4Type::DestinationUnreachable => {
                Icmpv4ErrorId::DestinationUnreachable(
                    Icmpv4DestinationUnreachable::from(header.code()),
                )
            }
            _ => unimplemented!(),
        };

        let error = Icmpv4Error(datagram);
        let _ = Icmpv4ErrorId::decode(&error.0.header())?;
        Ok(error)
    }
}

pub struct Icmpv4ErrorMut<'a>(Icmpv4DatagramMut<'a>);

impl<'a> Icmpv4ErrorMut<'a> {
    pub fn new_bytes(ipv4: ipv4::Datagram<'_>) -> Vec<u8> {
        let frame = ipv4.frame();
        // note that the 4 bytes included in the text size is additional
        // data that error datagrams include (e.g. NEXT_HOP_MTU).
        let mut bytes =
            Icmpv4DatagramMut::new_bytes(4 + frame.text().len());
        let mut icmpv4 = Icmpv4ErrorMut::from_bytes(bytes.as_mut()).unwrap();
        let bytes_written = icmpv4.context().write(frame.text()).unwrap();
        // from [Wikipedia](https://en.wikipedia.org/wiki/Internet_Control_Message_Protocol#Control_messages):
        // > ICMP error messages contain a data section that includes a copy
        // > of the entire IPv4 header, plus at least the first eight bytes of
        // > data from the IPv4 packet that caused the error message.
        assert!(bytes_written >= ipv4::HEADER_SIZE + 4 + 8);
        bytes
    }

    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(Icmpv4ErrorMut(Icmpv4DatagramMut::from_bytes(bytes)?))
    }

    pub fn icmpv4(&mut self) -> &mut Icmpv4DatagramMut<'a> {
        &mut self.0
    }

    pub fn id(&mut self, id: Icmpv4ErrorId) {
        let (r#type, code) = id.encode();
        let mut header = self.0.header();
        header.r#type(r#type);
        header.code(code);
    }

    pub fn next_hop_mtu(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0.text()[2..4], value)
    }

    pub fn context(&mut self) -> &mut [u8] {
        &mut self.0.text()[4..]
    }

    pub fn unmut(self) -> Icmpv4Error<'a> {
        Icmpv4Error(self.0.unmut())
    }

    pub fn seal(self) -> Result<Icmpv4Error<'a>> {
        trace!("Icmpv4ErrorMut::seal()");
        let error = Icmpv4Error::try_from(self.0.seal()?)?;
        // todo: we don't yet support setting the next hop MTU. when we do,
        // we'll need to ensure this is 0 when not needed.
        assert_eq!(0, error.next_hop_mtu());
        Ok(error)
    }
}
