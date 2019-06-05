use crate::protocols::ethernet2;
use std::rc::Rc;

pub enum Effect {
    Transmit(Rc<ethernet2::Frame>),
}
