use super::{
    cache::ArpCache,
    pdu::{ArpOp, ArpPdu},
};
use crate::{
    prelude::*, protocols::ethernet2::MacAddress, r#async::Future, runtime,
};
use std::{
    any::Any, cell::RefCell, collections::HashMap, convert::TryFrom,
    mem::swap, net::Ipv4Addr, rc::Rc, time::Instant,
};

pub struct ArpState<'a> {
    rt: Rc<RefCell<runtime::State<'a>>>,
    cache: Rc<RefCell<ArpCache>>,
}

impl<'a> ArpState<'a> {
    pub fn new(
        now: Instant,
        rt: Rc<RefCell<runtime::State<'a>>>,
    ) -> ArpState<'a> {
        let cache = {
            let rt = rt.borrow();
            ArpCache::from_options(now, &rt.options().arp.cache)
        };

        ArpState {
            rt,
            cache: Rc::new(RefCell::new(cache)),
        }
    }

    fn advance_clock(&mut self, now: Instant) {
        let mut cache = self.cache.borrow_mut();
        cache.advance_clock(now)
    }

    pub fn service(&mut self, now: Instant) {
        self.advance_clock(now);
        let mut cache = self.cache.borrow_mut();
        cache.try_evict(2);
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<()> {
        let (my_ipv4_addr, my_link_addr) = {
            let rt = self.rt.borrow();
            (rt.options().my_ipv4_addr, rt.options().my_link_addr)
        };

        let mut arp = ArpPdu::try_from(bytes)?;
        // drop request if it's not intended for this station.
        if arp.target_ip_addr != my_ipv4_addr {
            return Err(Fail::Ignored {});
        }

        match arp.op {
            ArpOp::ArpRequest => {
                eprintln!("# received arp request");
                arp.op = ArpOp::ArpReply;
                arp.target_link_addr = my_link_addr;
                swap(&mut arp.sender_ip_addr, &mut arp.target_ip_addr);
                swap(&mut arp.sender_link_addr, &mut arp.target_link_addr);

                let packet = arp.to_packet()?;
                let mut rt = self.rt.borrow_mut();
                rt.emit_effect(Effect::Transmit(packet));
                Ok(())
            }
            ArpOp::ArpReply => {
                eprintln!(
                    "# received arp reply: `{}` -> `{}`",
                    arp.sender_ip_addr, arp.sender_link_addr
                );
                let mut cache = self.cache.borrow_mut();
                cache.insert(arp.sender_ip_addr, arp.sender_link_addr);
                Ok(())
            }
        }
    }

    pub fn query(&self, ipv4_addr: Ipv4Addr) -> Future<'a, MacAddress> {
        {
            let cache = self.cache.borrow();
            if let Some(link_addr) = cache.get_link_addr(ipv4_addr) {
                return Future::r#const(*link_addr);
            }
        }

        let rt1 = self.rt.borrow_mut();
        let cache = self.cache.clone();
        let rt = self.rt.clone();
        rt1.start_task(move || {
            let (my_ipv4_addr, my_link_addr) = {
                let rt = rt.borrow();
                (rt.options().my_ipv4_addr, rt.options().my_link_addr)
            };

            {
                let arp = ArpPdu {
                    op: ArpOp::ArpRequest,
                    sender_link_addr: my_link_addr,
                    sender_ip_addr: my_ipv4_addr,
                    target_link_addr: MacAddress::nil(),
                    target_ip_addr: ipv4_addr,
                };

                let packet = arp.to_packet()?;
                let mut rt = rt.borrow_mut();
                rt.emit_effect(Effect::Transmit(packet));
            }

            // can't make progress until a reply deposits an entry in the
            // cache.
            loop {
                eprintln!("# looking up `{}` in ARP cache...", my_ipv4_addr);
                let result = {
                    let cache = cache.borrow();
                    cache.get_link_addr(ipv4_addr).copied()
                };

                if let Some(link_addr) = result {
                    let x: Rc<Any> = Rc::new(link_addr);
                    return Ok(x);
                } else {
                    // unable to make progress until the appropriate entry is
                    // inserted into the cache.
                    yield None;
                }
            }
        })
    }

    pub fn export_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        let cache = self.cache.borrow();
        cache.export()
    }
}
