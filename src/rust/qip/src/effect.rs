use std::rc::Rc;

pub enum Effect {
    Transmit(Rc<Vec<u8>>),
}
