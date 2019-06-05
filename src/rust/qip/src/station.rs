use crate::{
    prelude::*,
    protocols::{
        arp::Arp,
        ethernet2::{self, MacAddress},
    },
};
use r#async::Future;
use std::{collections::HashMap, net::Ipv4Addr, rc::Rc, time::Instant};

pub struct Station<'a> {
    rt: Runtime<'a>,
    arp: Arp<'a>,
}

impl<'a> Station<'a> {
    pub fn from_options(now: Instant, options: Options) -> Station<'a> {
        let rt = Runtime::from_options(now, options);
        let arp = Arp::new(now, rt.clone());
        Station { rt, arp }
    }

    pub fn options(&self) -> Rc<Options> {
        self.rt.options()
    }

    pub fn receive(&mut self, frame: Rc<ethernet2::Frame>) -> Result<()> {
        let dest_addr = frame.header().dest_addr;
        if self.rt.options().my_link_addr != dest_addr
            && !dest_addr.is_broadcast()
        {
            return Err(Fail::Misdelivered {});
        }

        match frame.header().ether_type {
            ethernet2::EtherType::Arp => self.arp.receive(frame.payload()),
        }
    }

    pub fn poll(&mut self, now: Instant) -> Option<Effect> {
        self.arp.service();
        self.rt.poll(now)
    }

    pub fn arp_query(&self, ipv4_addr: Ipv4Addr) -> Future<'a, MacAddress> {
        self.arp.query(ipv4_addr)
    }

    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.arp.export_cache()
    }
}
