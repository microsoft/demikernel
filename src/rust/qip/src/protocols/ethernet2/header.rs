use crate::{prelude::*, protocols::ethernet2::MacAddress};
use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use num_traits::FromPrimitive;
use std::io::{Read, Write};

#[repr(u16)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug)]
pub enum EtherType {
    Arp = 0x806,
    Ipv4 = 0x800,
}

pub struct Ethernet2Header {
    pub dest_addr: MacAddress,
    pub src_addr: MacAddress,
    pub ether_type: EtherType,
}

impl Ethernet2Header {
    pub fn read(reader: &mut Read) -> Result<Ethernet2Header> {
        let mut dest_addr = [0; 6];
        reader.read_exact(&mut dest_addr)?;
        let mut src_addr = [0; 6];
        reader.read_exact(&mut src_addr)?;
        let ether_type = reader.read_u16::<NetworkEndian>()?;
        let ether_type = match FromPrimitive::from_u16(ether_type) {
            Some(x) => x,
            None => return Err(Fail::Unsupported {}),
        };

        Ok(Ethernet2Header {
            dest_addr: MacAddress::new(dest_addr),
            src_addr: MacAddress::new(src_addr),
            ether_type,
        })
    }

    pub fn write(&self, writer: &mut Write) -> Result<()> {
        writer.write_all(&self.dest_addr.to_array())?;
        writer.write_all(&self.src_addr.to_array())?;
        writer.write_u16::<NetworkEndian>(self.ether_type.clone() as u16)?;
        Ok(())
    }

    pub fn size() -> usize {
        14
    }
}
