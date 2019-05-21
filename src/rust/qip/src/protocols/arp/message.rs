use std::convert::{TryFrom, TryInto};
use crate::prelude::*;
use std::io::{Read, Write, Cursor};
use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use std::net::Ipv4Addr;
use eui48::MacAddress;

const HARD_TYPE_ETHER2: u16 = 1;
const HARD_SIZE_ETHER2: u8 = 6;
const PROT_TYPE_IPV4: u16 = 0x800;
const PROT_SIZE_IPV4: u8 = 4;

#[repr(u16)]
pub enum ArpOp {
    ArpRequest = 1,
    ArpReply = 2,
}

impl TryFrom<u16> for ArpOp {
    type Error = Fail;

    fn try_from(n: u16) -> Result<ArpOp> {
        if n == ArpOp::ArpRequest as u16 {
            return Ok(ArpOp::ArpRequest);
        }

        if n == ArpOp::ArpReply as u16 {
            return Ok(ArpOp::ArpReply);
        }

        Err(Fail::Unsupported {})
    }
}

pub struct ArpMessage {
    pub op: ArpOp,
    pub sender_link_addr: MacAddress,
    pub sender_ip_addr: Ipv4Addr,
    pub target_link_addr: MacAddress,
    pub target_ip_addr: Ipv4Addr,
}

impl ArpMessage {
    pub fn read(reader: &mut Read) -> Result<ArpMessage> {
        let hard_type = reader.read_u16::<NetworkEndian>()?;
        if hard_type != HARD_TYPE_ETHER2 {
            return Err(Fail::Unsupported {});
        }

        let prot_type = reader.read_u16::<NetworkEndian>()?;
        if prot_type != PROT_TYPE_IPV4 {
            return Err(Fail::Unsupported {});
        }

        let mut byte = [0; 1];
        reader.read_exact(&mut byte)?;
        let hard_size = byte[0];
        if hard_size != HARD_SIZE_ETHER2 {
            return Err(Fail::Unsupported {});
        }

        reader.read_exact(&mut byte)?;
        let prot_size = byte[0];
        if prot_size != PROT_SIZE_IPV4 {
            return Err(Fail::Unsupported {});
        }

        let op = reader.read_u16::<NetworkEndian>()?;
        let mut sender_link_addr = [0; 6];
        reader.read_exact(&mut sender_link_addr)?;
        let sender_ip_addr = reader.read_u32::<NetworkEndian>()?;
        let mut target_link_addr = [0; 6];
        reader.read_exact(&mut target_link_addr)?;
        let target_ip_addr = reader.read_u32::<NetworkEndian>()?;

        Ok(ArpMessage {
            op: ArpOp::try_from(op)?,
            sender_link_addr: MacAddress::from_bytes(&sender_link_addr).unwrap(),
            sender_ip_addr: From::from(sender_ip_addr),
            target_link_addr: MacAddress::from_bytes(&target_link_addr).unwrap(),
            target_ip_addr: From::from(target_ip_addr),
        })
    }

    pub fn write(&self, writer: &mut Write) -> Result<()> {
        writer.write_u16::<NetworkEndian>(HARD_TYPE_ETHER2)?;
        writer.write_u16::<NetworkEndian>(PROT_TYPE_IPV4)?;
        let byte = [HARD_SIZE_ETHER2; 1];
        writer.write_all(&byte)?;
        let byte = [PROT_SIZE_IPV4; 1];
        writer.write_all(&byte)?;
        writer.write_u16::<NetworkEndian>(self.op as u16)?;
        let mut sender_link_addr = [0; 6];
        writer.write_all(&sender_link_addr)?;
        writer.write_u32::<NetworkEndian>(self.sender_ip_addr.into())?;
        let mut target_link_addr = [0; 6];
        writer.write_all(&target_link_addr)?;
        writer.write_u32::<NetworkEndian>(self.target_ip_addr.into())?;
        Ok(())
    }
}

impl TryFrom<&[u8]> for ArpMessage {
    type Error = Fail;

    fn try_from(bytes: &[u8]) -> Result<ArpMessage> {
        let mut reader = Cursor::new(bytes);
        ArpMessage::read(&mut reader)
    }
}

impl TryInto<Vec<u8>> for ArpMessage {
    type Error = Fail;

    fn try_into(self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.write(&mut bytes)?;
        Ok(bytes)
    }
}

