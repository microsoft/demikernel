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
    cell::RefCell,
    convert::TryFrom,
    mem::swap,
    net::Ipv4Addr,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct ArpState<'a> {
    rt: Rc<RefCell<runtime::State>>,
    cache: ArpCache,
    r#async: Async<'a, (Ipv4Addr, MacAddress)>,
}

impl<'a> ArpState<'a> {
    pub fn new(rt: Rc<RefCell<runtime::State>>, now: Instant) -> ArpState<'a> {
        let cache = {
            let rt = rt.borrow();
            ArpCache::from_options(&rt.options().arp.cache, now)
        };

        ArpState {
            rt,
            cache,
            r#async: Async::new(now),
        }
    }

    pub fn advance_clock(&mut self, now: Instant) {
        self.cache.advance_clock(now)
    }

    pub fn receive(&mut self, bytes: Vec<u8>) -> Result<Vec<Effect>> {
        let (my_ipv4_addr, my_link_addr) = {
            let rt = self.rt.borrow();
            (rt.options().my_ipv4_addr, rt.options().my_link_addr)
        };

        let mut arp = ArpPdu::try_from(bytes.as_slice())?;
        if arp.target_ip_addr != my_ipv4_addr {
            return Err(Fail::Ignored {});
        }

        match arp.op {
            ArpOp::ArpRequest => {
                arp.op = ArpOp::ArpReply;
                arp.target_link_addr = my_link_addr;
                swap(&mut arp.sender_ip_addr, &mut arp.target_ip_addr);
                swap(&mut arp.sender_link_addr, &mut arp.target_link_addr);

                let ether2_header = ethernet2::Header {
                    dest_addr: arp.target_link_addr,
                    src_addr: arp.sender_link_addr,
                    ether_type: ethernet2::EtherType::Arp,
                };

                let mut packet = Vec::with_capacity(
                    ArpPdu::size() + ethernet2::Header::size(),
                );
                ether2_header.write(&mut packet)?;
                arp.write(&mut packet)?;
                Ok(vec![Effect::Transmit(packet)])
            }
            ArpOp::ArpReply => {
                self.cache.insert(arp.target_ip_addr, arp.target_link_addr);
                Ok(vec![])
            }
        }
    }

    fn query(
        &self,
        ipv4_addr: Ipv4Addr,
    ) -> impl Future<'a, (Ipv4Addr, MacAddress)> {
        self.r#async.start_task(|| {
            // rust does not recognize the closure as a generator unless a
            // `yield` statement is present.
            yield None;
            Ok((Ipv4Addr::BROADCAST, MacAddress::broadcast()))
        })
    }
}
