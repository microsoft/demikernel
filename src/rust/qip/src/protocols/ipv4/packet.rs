use super::header::{Ipv4HeaderMut, IPV4_HEADER_SIZE};
use crate::{prelude::*, protocols::ethernet2};

pub struct Ipv4PacketMut<'a>(ethernet2::FrameMut<'a>);

impl<'a> Ipv4PacketMut<'a> {
    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Ipv4PacketMut> {
        Ok(Ipv4PacketMut(ethernet2::FrameMut::from_bytes(bytes)?))
    }

    pub fn frame_mut(&mut self) -> &mut ethernet2::FrameMut<'a> {
        &mut self.0
    }

    pub fn header_mut(&mut self) -> Ipv4HeaderMut<&mut [u8]> {
        Ipv4HeaderMut(&mut self.0.payload_mut()[..IPV4_HEADER_SIZE])
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.0.payload_mut()[IPV4_HEADER_SIZE..]
    }

    pub fn validate(&'a mut self) -> Result<()> {
        let payload_len = {
            let mut payload = self.0.payload_mut();
            Ipv4HeaderMut::validate(&mut payload)?;
            payload.len()
        };

        // todo: need to ask about why the lifetime of `frame` lasts longer
        // than the scope its defined in.
        /*let ether_type = {
            let frame = self.frame_mut();
            let header = frame.header_mut();
            header.get_ether_type()?
        };

        assert_eq!(ether_type, ethernet2::EtherType::Ipv4);*/
        let header = self.header_mut();
        let total_len = header.get_total_len() as usize;
        if total_len != payload_len {
            return Err(Fail::Malformed {});
        }

        Ok(())
    }
}
