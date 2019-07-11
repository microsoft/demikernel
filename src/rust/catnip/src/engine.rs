use crate::{
    prelude::*,
    protocols::{
        arp,
        ethernet2::{self, MacAddress},
        ip, ipv4,
    },
    r#async::{self, Async},
};
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct Engine<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
    ipv4: ipv4::Peer<'a>,
}

impl<'a> Engine<'a> {
    pub fn from_options(now: Instant, options: Options) -> Result<Engine<'a>> {
        let rt = Runtime::from_options(now, options);
        let arp = arp::Peer::new(now, rt.clone())?;
        let ipv4 = ipv4::Peer::new(rt.clone(), arp.clone());
        Ok(Engine { rt, arp, ipv4 })
    }

    pub fn options(&self) -> Rc<Options> {
        self.rt.options()
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<()> {
        trace!("Engine::receive({:?})", bytes);
        let frame = ethernet2::Frame::attach(&bytes)?;
        let header = frame.header();
        if self.rt.options().my_link_addr != header.dest_addr()
            && !header.dest_addr().is_broadcast()
        {
            return Err(Fail::Misdelivered {});
        }

        #[allow(unreachable_patterns)]
        match header.ether_type()? {
            ethernet2::EtherType::Arp => self.arp.receive(frame),
            ethernet2::EtherType::Ipv4 => self.ipv4.receive(frame),
            _ => Err(Fail::Unimplemented {}),
        }
    }

    pub fn arp_query(
        &self,
        ipv4_addr: Ipv4Addr,
    ) -> r#async::Future<'a, MacAddress> {
        self.arp.query(ipv4_addr)
    }

    pub fn udp_cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: ip::Port,
        src_port: ip::Port,
        text: Vec<u8>,
    ) -> r#async::Future<'a, ()> {
        self.ipv4
            .udp_cast(dest_ipv4_addr, dest_port, src_port, text)
    }

    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.arp.export_cache()
    }

    pub fn import_arp_cache(&self, cache: HashMap<Ipv4Addr, MacAddress>) {
        self.arp.import_cache(cache)
    }

    pub fn ping(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> r#async::Future<'a, Duration> {
        self.ipv4.ping(dest_ipv4_addr, timeout)
    }

    pub fn is_udp_port_open(&self, port: ip::Port) -> bool {
        self.ipv4.is_udp_port_open(port)
    }

    pub fn open_udp_port(&mut self, port: ip::Port) {
        self.ipv4.open_udp_port(port);
    }

    pub fn close_udp_port(&mut self, port: ip::Port) {
        self.ipv4.close_udp_port(port);
    }
}

impl<'a> Async<Effect> for Engine<'a> {
    fn poll(&self, now: Instant) -> Option<Result<Effect>> {
        try_poll!(&self.arp, now);
        try_poll!(&self.ipv4, now);
        self.rt.poll(now)
    }
}
