use crate::prelude::*;
use std::num::NonZeroU16;

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Display)]
pub struct Port(NonZeroU16);

impl TryFrom<u16> for Port {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self> {
        Ok(Port(NonZeroU16::new(n).ok_or(Fail::OutOfRange {
            details: "port number may not be zero",
        })?))
    }
}

impl Into<u16> for Port {
    fn into(self) -> u16 {
        self.0.get()
    }
}
