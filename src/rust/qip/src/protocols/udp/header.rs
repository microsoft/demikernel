use crate::prelude::*;
use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use std::{
    convert::TryFrom,
    io::{Read, Write},
};

const UDP_HEADER_SIZE: usize = 8;

pub struct UdpHeader {
    pub src_port: u16,
    pub dest_port: u16,
}

impl UdpHeader {
    pub fn read(reader: &mut Read) -> Result<Self> {
        let src_port = reader.read_u16::<NetworkEndian>()?;
        let dest_port = reader.read_u16::<NetworkEndian>()?;
        Ok(UdpHeader {
            src_port,
            dest_port,
        })
    }

    pub fn write(&self, writer: &mut Write, length: usize) -> Result<()> {
        writer.write_u16::<NetworkEndian>(self.src_port)?;
        writer.write_u16::<NetworkEndian>(self.dest_port)?;
        writer.write_u16::<NetworkEndian>(u16::try_from(length)?)?;
        // todo: UDP checksum is optional, so we're not going to implement it
        // until it becomes a priority.
        writer.write_u16::<NetworkEndian>(0)?;
        Ok(())
    }

    pub fn size() -> usize {
        UDP_HEADER_SIZE
    }
}
