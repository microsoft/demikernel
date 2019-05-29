use super::{
    cache::ArpCache,
    pdu::{ArpOp, ArpPdu},
};
use crate::{
    prelude::*,
    protocols::ethernet2,
    r#async::{Async, Future},
    runtime,
};
use eui48::MacAddress;
use std::{
    cell::RefCell, convert::TryFrom, mem::swap, net::Ipv4Addr, rc::Rc,
    time::Instant,
};

pub struct ArpState<'a> {
    rt: Rc<RefCell<runtime::State>>,
    cache: Rc<RefCell<ArpCache>>,
    r#async: Async<'a, MacAddress>,
}

impl<'a> ArpState<'a> {
    pub fn new(rt: Rc<RefCell<runtime::State>>, now: Instant) -> ArpState<'a> {
        let cache = {
            let rt = rt.borrow();
            ArpCache::from_options(&rt.options().arp.cache, now)
        };

        ArpState {
            rt,
            cache: Rc::new(RefCell::new(cache)),
            r#async: Async::new(now),
        }
    }

    pub fn advance_clock(&mut self, now: Instant) {
        let mut cache = self.cache.borrow_mut();
        cache.advance_clock(now)
    }

    pub fn receive(&mut self, bytes: Vec<u8>) -> Result<()> {
        let (my_ipv4_addr, my_link_addr) = {
            let rt = self.rt.borrow();
            (rt.options().my_ipv4_addr, rt.options().my_link_addr)
        };

        let mut arp = ArpPdu::try_from(bytes.as_slice())?;
        // drop request if it's not intended for this station.
        if arp.target_ip_addr != my_ipv4_addr {
            return Err(Fail::Ignored {});
        }

        match arp.op {
            ArpOp::ArpRequest => {
                arp.op = ArpOp::ArpReply;
                arp.target_link_addr = my_link_addr;
                swap(&mut arp.sender_ip_addr, &mut arp.target_ip_addr);
                swap(&mut arp.sender_link_addr, &mut arp.target_link_addr);

                let packet = arp.to_packet()?;
                let mut rt = self.rt.borrow_mut();
                rt.effects().push_back(Effect::Transmit(packet));
                Ok(())
            }
            ArpOp::ArpReply => {
                let mut cache = self.cache.borrow_mut();
                cache.insert(arp.sender_ip_addr, arp.sender_link_addr);
                Ok(())
            }
        }
    }

    fn query(&self, ipv4_addr: Ipv4Addr) -> Future<'a, MacAddress> {
        {
            let cache = self.cache.borrow();
            if let Some(link_addr) = cache.get_link_addr(ipv4_addr) {
                return Future::Const(Ok(*link_addr));
            }
        }

        let cache = self.cache.clone();
        let rt = self.rt.clone();
        self.r#async.start_task(move || {
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
                rt.effects().push_back(Effect::Transmit(packet));
            }

            // can't make progress until a reply deposits an entry in the
            // cache.
            loop {
                let result = {
                    let cache = cache.borrow();
                    cache.get_link_addr(my_ipv4_addr).copied()
                };

                if let Some(link_addr) = result {
                    return Ok(link_addr);
                } else {
                    // unable to make progress until the appropriate entry is
                    // inserted into the cache.
                    yield None;
                }
            }
        })
    }
}
