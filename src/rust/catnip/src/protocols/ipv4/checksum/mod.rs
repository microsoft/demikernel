// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(test)]
mod tests;

use byteorder::{ByteOrder, NetworkEndian};
use either::Either;
use std::io::{Result as IoResult, Write};

// note: we're following the model of std::hash::Ipv4Checksum, even though
// it only supports 64-bit hash widths.

#[derive(Default)]
pub struct Accum {
    sum: u32,
    odd_byte: Option<u8>,
}

pub struct Ipv4Checksum(Either<Accum, u16>);

impl Ipv4Checksum {
    pub fn new() -> Ipv4Checksum {
        Ipv4Checksum(Either::Left(Accum::default()))
    }

    pub fn finish(&mut self) -> u16 {
        if let Either::Right(result) = self.0 {
            return result;
        }

        let mut accum = self.0.as_mut().left().unwrap();
        if let Some(odd_byte) = accum.odd_byte {
            let pair = [odd_byte, 0];
            let n = u32::from(NetworkEndian::read_u16(&pair));
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

impl Default for Ipv4Checksum {
    fn default() -> Self {
        Ipv4Checksum::new()
    }
}

impl Write for Ipv4Checksum {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        let mut bytes = buf;
        // `write()` shouldn't be called after `finish()` has been called.
        let mut accum = self.0.as_mut().left().unwrap();

        if bytes.is_empty() {
            return Ok(0);
        }

        if let Some(odd_byte) = accum.odd_byte {
            let pair = [odd_byte, bytes[0]];
            let n = u32::from(NetworkEndian::read_u16(&pair));
            accum.sum += n;
            bytes = &bytes[1..];
            accum.odd_byte = None;
        }

        while bytes.len() >= 2 {
            let n = u32::from(NetworkEndian::read_u16(bytes));
            accum.sum += n;
            bytes = &bytes[2..];
        }

        if !bytes.is_empty() {
            accum.odd_byte = Some(bytes[0])
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}
