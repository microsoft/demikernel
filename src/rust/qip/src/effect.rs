use crate::protocols::ethernet2;

pub enum Effect {
    Transmit(ethernet2::Frame),
}
