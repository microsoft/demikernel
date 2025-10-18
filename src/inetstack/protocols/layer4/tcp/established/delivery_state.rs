use crate::inetstack::protocols::layer4::tcp::established::{receiver::Receiver, sender::Sender};

pub struct DeliveryState {
    pub sender: Sender,
    pub receiver: Receiver,
}

impl DeliveryState {
    pub fn new(sender: Sender, receiver: Receiver) -> Self {
        Self { sender, receiver }
    }
}
