use crate::{
    prelude::*,
    protocols::{
        arp,
        ethernet2::{self, MacAddress},
    },
    runtime,
};
use r#async::Future;
use std::{
    cell::RefCell, collections::HashMap, convert::TryFrom, net::Ipv4Addr,
    rc::Rc, time::Instant,
};

pub struct Station<'a> {
    rt: Rc<RefCell<runtime::State<'a>>>,
    arp: arp::State<'a>,
}

impl<'a> Station<'a> {
    pub fn from_options(now: Instant, options: Options) -> Station<'a> {
        let rt =
            Rc::new(RefCell::new(runtime::State::from_options(now, options)));
        let arp = arp::State::new(now, rt.clone());
        Station { rt, arp }
    }

    pub fn receive(&mut self, bytes: Vec<u8>) -> Result<()> {
        let frame = ethernet2::Frame::try_from(bytes)?;

        {
            let dest_addr = frame.header().dest_addr;
            let rt = self.rt.borrow();
            if rt.options().my_link_addr != dest_addr
                && !dest_addr.is_broadcast()
            {
                return Err(Fail::Misdelivered {});
            }
        }

        match frame.header().ether_type {
            ethernet2::EtherType::Arp => self.arp.receive(frame.payload()),
        }
    }

    pub fn poll(&mut self, now: Instant) -> Option<Effect> {
        self.arp.service(now);
        let mut rt = self.rt.borrow_mut();
        rt.poll(now)
    }

    pub fn arp_query(&self, ipv4_addr: Ipv4Addr) -> Future<'a, MacAddress> {
        self.arp.query(ipv4_addr)
    }

    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.arp.export_cache()
    }
}
