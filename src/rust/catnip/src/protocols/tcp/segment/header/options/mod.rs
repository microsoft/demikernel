#![allow(dead_code)]

#[cfg(test)]
mod tests;

mod parsers;

use crate::prelude::*;
use byteorder::{NetworkEndian, WriteBytesExt};
use num_traits::FromPrimitive;
use std::{collections::HashMap, convert::TryFrom};

// from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch13.html):
// > if no MSS option is provided, a default value of 536 bytes is used.
const MIN_MSS: usize = 536;
const MAX_MSS: usize = u16::max_value() as usize;

#[repr(u8)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug, Hash)]
pub enum TcpOptionKind {
    Eol = 0,
    Nop = 1,
    Mss = 2,
}

impl TcpOptionKind {
    pub fn encoded_length(&self) -> usize {
        match self {
            TcpOptionKind::Eol => 1,
            TcpOptionKind::Nop => 1,
            TcpOptionKind::Mss => 4,
        }
    }
}

impl TryFrom<u8> for TcpOptionKind {
    type Error = Fail;

    fn try_from(n: u8) -> Result<Self> {
        match FromPrimitive::from_u8(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported {
                details: "unrecognized TCP option KIND",
            }),
        }
    }
}

impl Into<u8> for TcpOptionKind {
    fn into(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TcpOption {
    Nop,
    Mss(u16),
    Other { kind: u8, len: u8 },
}

impl TcpOption {
    pub fn kind(&self) -> Result<TcpOptionKind> {
        match self {
            TcpOption::Nop => Ok(TcpOptionKind::Nop),
            TcpOption::Mss(_) => Ok(TcpOptionKind::Mss),
            TcpOption::Other { kind, .. } => {
                if &0u8 == kind {
                    Ok(TcpOptionKind::Eol)
                } else {
                    Err(Fail::Unsupported {
                        details: "unrecognized TCP option KIND",
                    })
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct TcpOptions(HashMap<TcpOptionKind, TcpOption>);

impl TcpOptions {
    pub fn new() -> TcpOptions {
        TcpOptions(HashMap::new())
    }

    pub fn parse(bytes: &[u8]) -> Result<TcpOptions> {
        match parsers::start(bytes) {
            Ok((_, list)) => {
                let mut options = TcpOptions::new();
                for o in list {
                    match o.kind()? {
                        TcpOptionKind::Eol => (),
                        TcpOptionKind::Nop => (),
                        TcpOptionKind::Mss => options.insert(o)?,
                    }
                }

                Ok(options)
            }
            Err(e) => {
                debug!("failed to parse TCP options: {:?}", e);
                Err(Fail::Malformed {
                    details: "failed to parse TCP options",
                })
            }
        }
    }

    fn insert(&mut self, option: TcpOption) -> Result<()> {
        let kind = option.kind()?;
        match kind {
            TcpOptionKind::Mss => {
                if self.0.insert(kind, option).is_some() {
                    return Err(Fail::Malformed {
                        details: "duplicate TCP option",
                    });
                }

                Ok(())
            }
            _ => panic!(
                "unexpected attempt to insert TCP option `{:?}` into \
                 `TcpOptions` struct",
                kind
            ),
        }
    }

    fn padding(length: usize) -> usize {
        // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch13.html):
        // > the TCP header’s length is always required to be a multiple of 32
        // > bits because the TCP Header Length field uses that unit.
        let n = length % 4;
        let padding = if n == 0 { 0 } else { 4 - n };
        assert!(padding < 4);
        assert_eq!(0, (padding + length) % 4);
        padding
    }

    pub fn encoded_length(&self) -> usize {
        let mut length = TcpOptionKind::Eol.encoded_length();
        for kind in self.0.keys() {
            length += kind.encoded_length();
        }

        // the TCP header’s length is always required to be a multiple of 32
        // bits because the TCP Header Length field uses that unit.
        let padding = TcpOptions::padding(length);
        length + padding
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get_mss(&self) -> usize {
        self.0
            .get(&TcpOptionKind::Mss)
            .map(|o| match o {
                TcpOption::Mss(n) => (*n).into(),
                _ => unreachable!(),
            })
            .unwrap_or(MIN_MSS)
    }

    pub fn set_mss(&mut self, mss: usize) {
        assert!(mss >= MIN_MSS);
        assert!(mss <= MAX_MSS);
        let mss = u16::try_from(mss).unwrap();
        self.0.insert(TcpOptionKind::Mss, TcpOption::Mss(mss));
    }

    pub fn encode(&self, mut bytes: &mut [u8]) {
        let length = self.encoded_length();
        assert!(bytes.len() >= length);

        let mut bytes_written = 0;
        for (k, v) in &self.0 {
            bytes.write_u8(k.clone().into()).unwrap();
            bytes
                .write_u8(u8::try_from(k.encoded_length()).unwrap())
                .unwrap();

            match v {
                TcpOption::Mss(n) => {
                    bytes.write_u16::<NetworkEndian>(*n).unwrap()
                }
                _ => panic!(""),
            };

            bytes_written += k.encoded_length();
        }

        // calculate the number of NOPs we'll need for padding. note that the
        // EOL option must be considered.
        let padding = length - bytes_written - 1;
        for _ in 0..padding {
            bytes.write_u8(0u8).unwrap();
        }

        bytes.write_u8(TcpOptionKind::Eol.into()).unwrap();
    }

    pub fn header_length(&self) -> usize {
        self.encoded_length() + super::MIN_TCP_HEADER_SIZE
    }
}
