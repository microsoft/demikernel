use crate::{prelude::*, protocols::ethernet2::MacAddress};
use byteorder::{ByteOrder, NetworkEndian};
use num_traits::FromPrimitive;
use std::convert::TryFrom;

pub const ETHERNET2_HEADER_SIZE: usize = 14;

#[repr(u16)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug)]
pub enum EtherType {
    Arp = 0x806,
    Ipv4 = 0x800,
}

impl TryFrom<u16> for EtherType {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self> {
        match FromPrimitive::from_u16(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported {}),
        }
    }
}

impl Into<u16> for EtherType {
    fn into(self) -> u16 {
        self as u16
    }
}

pub struct Ethernet2Header<'a, T>(&'a T);

impl<'a, T> Ethernet2Header<'a, T>
where
    T: AsRef<[u8]>,
{
    pub fn new(bytes: &'a T) -> Ethernet2Header<'a, T> {
        Ethernet2Header(bytes)
    }

    pub fn dest_addr(&self) -> MacAddress {
        MacAddress::from_bytes(&self.0.as_ref()[0..6])
    }

    pub fn src_addr(&self) -> MacAddress {
        MacAddress::from_bytes(&self.0.as_ref()[6..12])
    }

    pub fn ether_type(&self) -> Result<EtherType> {
        let n = NetworkEndian::read_u16(&self.0.as_ref()[12..14]);
        Ok(EtherType::try_from(n)?)
    }
}

pub struct Ethernet2HeaderMut<'a, T>(&'a mut T);

impl<'a, T> Ethernet2HeaderMut<'a, T>
where
    T: AsRef<[u8]> + AsMut<[u8]>,
{
    pub fn new(bytes: &'a mut T) -> Ethernet2HeaderMut<'a, T> {
        Ethernet2HeaderMut(bytes)
    }

    pub fn get_dest_addr(&self) -> MacAddress {
        MacAddress::from_bytes(&self.0.as_ref()[0..6])
    }

    pub fn get_src_addr(&self) -> MacAddress {
        MacAddress::from_bytes(&self.0.as_ref()[6..12])
    }

    pub fn get_ether_type(&self) -> Result<EtherType> {
        let n = NetworkEndian::read_u16(&self.0.as_ref()[12..14]);
        Ok(EtherType::try_from(n)?)
    }

    pub fn set_dest_addr(&mut self, addr: MacAddress) {
        self.0.as_mut()[0..6].copy_from_slice(addr.as_bytes());
    }

    pub fn set_src_addr(&mut self, addr: MacAddress) {
        self.0.as_mut()[6..12].copy_from_slice(addr.as_bytes());
    }

    pub fn set_ether_type(&mut self, ether_type: EtherType) {
        NetworkEndian::write_u16(
            &mut self.0.as_mut()[12..14],
            ether_type.into(),
        );
    }
}
