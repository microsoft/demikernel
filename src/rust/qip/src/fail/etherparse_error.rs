use etherparse::{ReadError, WriteError};
use std::error::Error;

#[derive(Debug, Display)]
#[display(fmt = "{0}", "self.description()")]
pub enum EtherParseError {
    ReadError(ReadError),
    WriteError(WriteError),
}

impl Error for EtherParseError {
    fn description(&self) -> &str {
        match self {
            EtherParseError::ReadError(ref e) => {
                match e {
                    ReadError::IoError(ref f) => f.description(),
                    ReadError::UnexpectedEndOfSlice(_) => "Unexpected end of a slice was reached even though more data was expected to be present (argument is expected minimum size)",
                    ReadError::VlanDoubleTaggingUnexpectedOuterTpid(_) => "A double vlan tag was expected but the tpid of the outer vlan does not contain the expected id of 0x8100",
                    ReadError::IpUnsupportedVersion(_) => "The ip header version is not supported (only 4 & 6 are supported); the value is the version that was received.",
                    ReadError::Ipv4UnexpectedVersion(_) => "The ip header version field is not equal 4; the value is the version that was received.",
                    ReadError::Ipv4HeaderLengthBad(_) => "The ipv4 header length is smaller then the header itself (5)",
                    ReadError::Ipv4TotalLengthTooSmall(_) => "the total length field is too small to contain the header itself",
                    ReadError::Ipv6UnexpectedVersion(_) => "The ip header version field is not equal 6. The value is the version that was received.",
                    ReadError::Ipv6TooManyHeaderExtensions => "more then 7 header extensions are present (according to RFC82000 this should never happen).",
                    ReadError::TcpDataOffsetTooSmall(_) => "the data_offset field in a TCP header is smaller then the minimum size of the tcp header itself.",
                }
            },
            EtherParseError::WriteError(ref e) => {
                match e {
                    WriteError::IoError(ref f) => f.description(),
                    WriteError::ValueError(_) => "There is an error in the data that was given to build a packet",
                    WriteError::SliceTooSmall(_) => "a given slice is not big enough to serialize packet data"
                }
            },
        }
    }

    fn cause(&self) -> Option<&Error> {
        match self {
            EtherParseError::ReadError(ref e) => match e {
                ReadError::IoError(ref f) => Some(f),
                _ => None,
            },
        }
    }
}
