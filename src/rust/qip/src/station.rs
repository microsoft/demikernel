use crate::{
    prelude::*,
    protocols::{
        arp,
        ethernet2::{self, MacAddress},
        ipv4,
    },
};
use r#async::Future;
use std::{collections::HashMap, net::Ipv4Addr, rc::Rc, time::Instant};

pub struct Station<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
    ipv4: ipv4::Peer<'a>,
}

impl<'a> Station<'a> {
    pub fn from_options(
        now: Instant,
        options: Options,
    ) -> Result<Station<'a>> {
        let rt = Runtime::from_options(now, options);
        let arp = arp::Peer::new(now, rt.clone())?;
        let ipv4 = ipv4::Peer::new(rt.clone(), arp.clone());
        Ok(Station { rt, arp, ipv4 })
    }

    pub fn options(&self) -> Rc<Options> {
        self.rt.options()
    }

    pub fn receive(&mut self, bytes: &mut [u8]) -> Result<()> {
        let frame = ethernet2::Frame::from_bytes(bytes)?;
        let dest_addr = frame.header().dest_addr();
        if self.rt.options().my_link_addr != dest_addr
            && !dest_addr.is_broadcast()
        {
            return Err(Fail::Misdelivered {});
        }

        #[allow(unreachable_patterns)]
        match frame.header().ether_type()? {
            ethernet2::EtherType::Arp => self.arp.receive(bytes),
            ethernet2::EtherType::Ipv4 => self.ipv4.receive(bytes),
            _ => Err(Fail::Unsupported {}),
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
