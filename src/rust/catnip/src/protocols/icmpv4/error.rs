// we don't include the prelude here to avoid circular dependencies
use super::datagram::{Icmpv4Datagram, Icmpv4Type};
use crate::{fail::Fail, protocols::ipv4, result::Result};
use num_traits::FromPrimitive;
use std::{cmp::min, convert::TryFrom, error::Error, fmt, io::Write};

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

impl From<u8> for Icmpv4DestinationUnreachable {
    fn from(n: u8) -> Self {
        FromPrimitive::from_u8(n).unwrap()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icmpv4ErrorType {
    DestinationUnreachable(Icmpv4DestinationUnreachable),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Icmpv4Error {
    r#type: Icmpv4ErrorType,
    context: Vec<u8>,
}

impl Icmpv4Error {
    pub fn new(
        r#type: Icmpv4ErrorType,
        context: ipv4::Datagram<'_>,
    ) -> Icmpv4Error {
        let mut bytes = Vec::new();
        // i don't see a reason why writing to `bytes` should fail.
        bytes.write_all(context.header().as_bytes()).unwrap();
        // from [Wikipedia]():
        // > ICMP error messages contain a data section that includes a copy
        // > of the entire IPv4 header, plus at least the first eight bytes of
        // > data from the IPv4 packet that caused the error message.
        let sample_len = min(8, context.payload().len());
        bytes.write_all(&context.payload()[..sample_len]).unwrap();
        Icmpv4Error {
            r#type,
            context: bytes,
        }
    }

    pub fn r#type(&self) -> Icmpv4ErrorType {
        self.r#type
    }

    pub fn context(&self) -> &[u8] {
        &self.context
    }
}

impl Error for Icmpv4Error {
    fn description(&self) -> &str {
        match &self.r#type {
            Icmpv4ErrorType::DestinationUnreachable(code) => {
                match code {
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
            },
        }
    }
}

impl fmt::Display for Icmpv4Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl<'a> TryFrom<Icmpv4Datagram<'a>> for Icmpv4Error {
    type Error = Fail;

    fn try_from(datagram: Icmpv4Datagram<'a>) -> Result<Self> {
        let header = datagram.header();
        let r#type = match header.r#type()? {
            Icmpv4Type::DestinationUnreachable => {
                Icmpv4ErrorType::DestinationUnreachable(
                    Icmpv4DestinationUnreachable::from(header.code()),
                )
            }
            _ => return Err(Fail::Unimplemented {}),
        };

        Ok(Icmpv4Error {
            r#type,
            context: datagram.payload().to_vec(),
        })
    }
}
