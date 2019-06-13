use crate::{
    prelude::*,
    protocols::{
        arp,
        ethernet2::{self, MacAddress},
        ipv4,
    },
};
use r#async::Future;
use std::{
    collections::HashMap, convert::TryFrom, net::Ipv4Addr, rc::Rc,
    time::Instant,
};

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

    pub fn receive(&mut self, bytes: Vec<u8>) -> Result<()> {
        trace!("entering `Station::receive()`");
        let frame = ethernet2::Frame::try_from(bytes)?;
        let header = frame.read_header()?;
        if self.rt.options().my_link_addr != header.dest_addr
            && !header.dest_addr.is_broadcast()
        {
            return Err(Fail::Misdelivered {});
        }

        #[allow(unreachable_patterns)]
        match header.ether_type {
            ethernet2::EtherType::Arp => self.arp.receive(frame),
            ethernet2::EtherType::Ipv4 => self.ipv4.receive(frame),
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

    pub fn udp_cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: u16,
        src_port: u16,
        payload: Vec<u8>,
    ) -> Future<'a, ()> {
        self.ipv4.udp_cast(dest_ipv4_addr, dest_port, src_port, payload)
    }

    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.arp.export_cache()
    }
}
