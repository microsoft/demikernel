#[cfg(test)]
mod tests;

use byteorder::{ByteOrder, NetworkEndian};
use either::Either;

// note: we're following the model of std::hash::Hasher, even though
// it only supports 64-bit hash widths.

#[derive(Default)]
pub struct Accum {
    sum: u32,
    odd_byte: Option<u8>,
}

pub struct Hasher(Either<Accum, u16>);

impl Hasher {
    pub fn new() -> Hasher {
        Hasher(Either::Left(Accum::default()))
    }

    pub fn write(&mut self, mut bytes: &[u8]) {
        let mut accum = self.0.as_mut().left().unwrap();

        if bytes.is_empty() {
            return;
        }

        if let Some(odd_byte) = accum.odd_byte {
            let pair = [odd_byte, bytes[0]];
            let n = u32::from(NetworkEndian::read_u16(&pair));
            eprintln!("{:x}", n);
            accum.sum += n;
            bytes = &bytes[1..];
            accum.odd_byte = None;
        }

        while bytes.len() >= 2 {
            let n = u32::from(NetworkEndian::read_u16(bytes));
            eprintln!("{:x}", n);
            accum.sum += n;
            bytes = &bytes[2..];
        }

        if !bytes.is_empty() {
            accum.odd_byte = Some(bytes[0])
        }
    }

    pub fn finish(&mut self) -> u16 {
        if let Either::Right(result) = self.0 {
            return result;
        }

        let mut accum = self.0.as_mut().left().unwrap();
        if let Some(odd_byte) = accum.odd_byte {
            let pair = [odd_byte, 0];
            let n = u32::from(NetworkEndian::read_u16(&pair));
            eprintln!("{:x}", n);
            accum.sum += n;
        }

        loop {
            accum.sum = (accum.sum & 0xffff) + (accum.sum >> 16);
            if accum.sum >> 16 == 0 {
                break;
            }
        }

        let result = ((!accum.sum) & 0xffff) as u16;
        self.0 = Either::Right(result);
        result
    }
}
