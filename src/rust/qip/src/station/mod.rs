use crate::{
    prelude::*,
    protocols::{arp, ethernet2},
    runtime,
};
use eui48::MacAddress;
use std::{cell::RefCell, convert::TryFrom, rc::Rc, time::Instant};

pub struct Station<'a> {
    rt: Rc<RefCell<runtime::State>>,
    arp: arp::State<'a>,
}

impl<'a> Station<'a> {
    pub fn from_options(options: Options, now: Instant) -> Station<'a> {
        let rt = Rc::new(RefCell::new(runtime::State::from_options(options)));
        let arp = arp::State::new(rt.clone(), now);
        Station { rt, arp }
    }

    pub fn advance_clock(&mut self, now: Instant) {
        self.arp.advance_clock(now)
    }

    pub fn receive(&mut self, bytes: Vec<u8>) -> Result<()> {
        let frame = ethernet2::Frame::try_from(bytes)?;

        {
            let dest_addr = frame.header().dest_addr;
            let rt = self.rt.borrow();
            if rt.options().my_link_addr != dest_addr
                && MacAddress::broadcast() != dest_addr
            {
                return Err(Fail::Misdelivered {});
            }
        }

        match frame.header().ether_type {
            ethernet2::EtherType::Arp => {
                let payload = frame.payload().to_vec();
                self.arp.receive(payload)
            }
        }
    }
}
