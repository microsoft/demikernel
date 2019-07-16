use crate::{
    prelude::*,
    protocols::ethernet2::{self, MacAddress},
};
use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use num_traits::FromPrimitive;
use std::{
    convert::TryFrom,
    io::{Cursor, Read, Write},
    net::Ipv4Addr,
};

const HARD_TYPE_ETHER2: u16 = 1;
const HARD_SIZE_ETHER2: u8 = 6;
const PROT_TYPE_IPV4: u16 = 0x800;
const PROT_SIZE_IPV4: u8 = 4;

#[repr(u16)]
#[derive(FromPrimitive, Clone)]
pub enum ArpOp {
    ArpRequest = 1,
    ArpReply = 2,
}

impl TryFrom<u16> for ArpOp {
    type Error = Fail;

    fn try_from(n: u16) -> Result<ArpOp> {
        match FromPrimitive::from_u16(n) {
            Some(op) => Ok(op),
            None => Err(Fail::Unsupported {
                details: "ARP opcode must be REQUEST or REPLY",
            }),
        }
    }
}

pub struct ArpPdu {
    pub op: ArpOp,
    pub sender_link_addr: MacAddress,
    pub sender_ip_addr: Ipv4Addr,
    pub target_link_addr: MacAddress,
    pub target_ip_addr: Ipv4Addr,
}

impl ArpPdu {
    pub fn read(reader: &mut dyn Read) -> Result<ArpPdu> {
        trace!("ArpPdu::read(...)");
        let hard_type = reader.read_u16::<NetworkEndian>()?;
        if hard_type != HARD_TYPE_ETHER2 {
            return Err(Fail::Unsupported {
                details: "this ARP implementation only supports Ethernet II",
            });
        }

        let prot_type = reader.read_u16::<NetworkEndian>()?;
        if prot_type != PROT_TYPE_IPV4 {
            return Err(Fail::Unsupported {
                details: "this ARP implementation only supports IPv4",
            });
        }

        let mut byte = [0; 1];
        reader.read_exact(&mut byte)?;
        let hard_size = byte[0];
        if hard_size != HARD_SIZE_ETHER2 {
            return Err(Fail::Malformed {
                details: "ARP field HLEN contains an unexpected value",
            });
        }

        reader.read_exact(&mut byte)?;
        let prot_size = byte[0];
        if prot_size != PROT_SIZE_IPV4 {
            return Err(Fail::Unsupported {
                details: "ARP field PLEN contains an unexpected value",
            });
        }

        let op = reader.read_u16::<NetworkEndian>()?;
        let mut sender_link_addr = [0; 6];
        reader.read_exact(&mut sender_link_addr)?;
        let sender_ip_addr = reader.read_u32::<NetworkEndian>()?;
        let mut target_link_addr = [0; 6];
        reader.read_exact(&mut target_link_addr)?;
        let target_ip_addr = reader.read_u32::<NetworkEndian>()?;

        Ok(ArpPdu {
            op: ArpOp::try_from(op)?,
            sender_link_addr: MacAddress::new(sender_link_addr),
            sender_ip_addr: From::from(sender_ip_addr),
            target_link_addr: MacAddress::new(target_link_addr),
            target_ip_addr: From::from(target_ip_addr),
        })
    }

    pub fn write(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_u16::<NetworkEndian>(HARD_TYPE_ETHER2)?;
        writer.write_u16::<NetworkEndian>(PROT_TYPE_IPV4)?;
        let byte = [HARD_SIZE_ETHER2; 1];
        writer.write_all(&byte)?;
        let byte = [PROT_SIZE_IPV4; 1];
        writer.write_all(&byte)?;
        writer.write_u16::<NetworkEndian>(self.op.clone() as u16)?;
        writer.write_all(self.sender_link_addr.as_bytes())?;
        writer.write_u32::<NetworkEndian>(self.sender_ip_addr.into())?;
        writer.write_all(self.target_link_addr.as_bytes())?;
        writer.write_u32::<NetworkEndian>(self.target_ip_addr.into())?;
        Ok(())
    }

    pub fn size() -> usize {
        28
    }

    pub fn to_datagram(&self) -> Result<Vec<u8>> {
        trace!("ArpPdu::to_datagram()");
        let dest_addr = match self.op {
            ArpOp::ArpRequest => {
                if MacAddress::nil() != self.target_link_addr {
                    panic!(
                        "the target link address of an ARP request must be \
                         `MacAddress::nil()`"
                    );
                }

                MacAddress::broadcast()
            }
            ArpOp::ArpReply => {
                if MacAddress::nil() == self.target_link_addr {
                    panic!(
                        "the target link address of an ARP reply must not be \
                         `MacAddress::nil()`"
                    );
                }

                if MacAddress::broadcast() == self.target_link_addr {
                    panic!(
                        "the target link address of an ARP reply must not be \
                         `MacAddress::broadcast()`"
                    );
                }

                self.target_link_addr
            }
        };

        let mut bytes = ethernet2::Frame::new(ArpPdu::size());
        let mut frame = ethernet2::FrameMut::attach(&mut bytes);
        self.write(&mut frame.text())?;
        let mut header = frame.header();
        header.dest_addr(dest_addr);
        header.src_addr(self.sender_link_addr);
        header.ether_type(ethernet2::EtherType::Arp);
        debug!("ArpPdu::to_datagram() -> `{:?}`", bytes);
        Ok(bytes)
    }
}

impl TryFrom<&[u8]> for ArpPdu {
    type Error = Fail;

    fn try_from(bytes: &[u8]) -> Result<ArpPdu> {
        trace!("ArpPdu::try_from({:?})", bytes);
        let mut reader = Cursor::new(bytes);
        ArpPdu::read(&mut reader)
    }
}
