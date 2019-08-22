use super::{
    cache::ArpCache,
    pdu::{ArpOp, ArpPdu},
};
use crate::{
    prelude::*,
    protocols::ethernet2::{self, MacAddress},
};
use std::{
    cell::RefCell, collections::HashMap, convert::TryFrom, mem::swap,
    net::Ipv4Addr, rc::Rc, time::Instant,
};

#[derive(Clone)]
pub struct ArpPeer<'a> {
    rt: Runtime<'a>,
    // todo: this should be shared state.
    cache: Rc<RefCell<ArpCache>>,
}

impl<'a> ArpPeer<'a> {
    pub fn new(now: Instant, rt: Runtime<'a>) -> Result<ArpPeer<'a>> {
        let options = rt.options();
        let cache = ArpCache::new(now, Some(options.arp.cache_ttl));
        Ok(ArpPeer {
            rt,
            cache: Rc::new(RefCell::new(cache)),
        })
    }

    pub fn receive(&mut self, frame: ethernet2::Frame<'_>) -> Result<()> {
        trace!("ArpPeer::receive(...)");
        let options = self.rt.options();
        // from RFC 826:
        // > ?Do I have the hardware type in ar$hrd?
        // > [optionally check the hardware length ar$hln]
        // > ?Do I speak the protocol in ar$pro?
        // > [optionally check the protocol length ar$pln]
        let mut arp = ArpPdu::try_from(frame.text())?;
        // from RFC 826:
        // > Merge_flag := false
        // > If the pair <protocol type, sender protocol address> is
        // > already in my translation table, update the sender
        // > hardware address field of the entry with the new
        // > information in the packet and set Merge_flag to true.
        let merge_flag = {
            let mut cache = self.cache.borrow_mut();
            if cache.get_link_addr(arp.sender_ip_addr).is_some() {
                cache.insert(arp.sender_ip_addr, arp.sender_link_addr);
                true
            } else {
                false
            }
        };

        // from RFC 826: ?Am I the target protocol address?
        if arp.target_ip_addr != options.my_ipv4_addr {
            if merge_flag {
                // we did do something.
                return Ok(());
            } else {
                // we didn't do anything.
                return Err(Fail::Ignored {
                    details: "unrecognized IP address",
                });
            }
        }

        // from RFC 826:
        // > If Merge_flag is false, add the triplet <protocol type,
        // > sender protocol address, sender hardware address> to
        // > the translation table.
        if !merge_flag {
            self.cache
                .borrow_mut()
                .insert(arp.sender_ip_addr, arp.sender_link_addr);
        }

        match arp.op {
            ArpOp::ArpRequest => {
                debug!("request from `{}`", arp.sender_link_addr);
                // from RFC 826:
                // > Swap hardware and protocol fields, putting the local
                // > hardware and protocol addresses in the sender fields.
                arp.target_link_addr = options.my_link_addr;
                swap(&mut arp.sender_ip_addr, &mut arp.target_ip_addr);
                swap(&mut arp.sender_link_addr, &mut arp.target_link_addr);
                // > Set the ar$op field to ares_op$REPLY
                arp.op = ArpOp::ArpReply;
                // > Send the packet to the (new) target hardware address on
                // > the same hardware on which the request was received.
                let bytes = Rc::new(RefCell::new(arp.to_datagram()?));
                self.rt.emit_event(Event::Transmit(bytes));
                Ok(())
            }
            ArpOp::ArpReply => {
                debug!(
                    "reply from `{}/{}`",
                    arp.sender_ip_addr, arp.sender_link_addr
                );
                self.cache
                    .borrow_mut()
                    .insert(arp.sender_ip_addr, arp.sender_link_addr);
                Ok(())
            }
        }
    }

    pub fn query(&self, ipv4_addr: Ipv4Addr) -> Future<'a, MacAddress> {
        {
            if let Some(link_addr) =
                self.cache.borrow().get_link_addr(ipv4_addr)
            {
                return Future::r#const(*link_addr);
            }
        }

        let rt = self.rt.clone();
        let cache = self.cache.clone();
        self.rt.start_coroutine(move || {
            let options = rt.options();
            let arp = ArpPdu {
                op: ArpOp::ArpRequest,
                sender_link_addr: options.my_link_addr,
                sender_ip_addr: options.my_ipv4_addr,
                target_link_addr: MacAddress::nil(),
                target_ip_addr: ipv4_addr,
            };

            let bytes = Rc::new(RefCell::new(arp.to_datagram()?));
            // from TCP/IP illustrated, chapter 4:
            // > The frequency of the ARP request is very close to one per
            // > second, the maximum suggested by [RFC1122].
            let mut retries_remaining = options.arp.retry_count + 1;
            while retries_remaining > 0 {
                rt.emit_event(Event::Transmit(bytes.clone()));
                retries_remaining -= 1;
                if yield_until!(
                    cache.borrow().get_link_addr(ipv4_addr).is_some(),
                    rt.now(),
                    options.arp.request_timeout
                ) {
                    let link_addr = cache
                        .borrow()
                        .get_link_addr(ipv4_addr)
                        .copied()
                        .unwrap();
                    debug!("ARP result available ({})", link_addr);
                    return CoroutineOk(link_addr);
                }

                warn!(
                    "ARP request timeout; {} retries remain.",
                    retries_remaining
                );
            }

            Err(Fail::Timeout {})
        })
    }

    pub fn export_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.cache.borrow().export()
    }

    pub fn import_cache(&self, cache: HashMap<Ipv4Addr, MacAddress>) {
        self.cache.borrow_mut().import(cache);
    }
}

impl<'a> Async<()> for ArpPeer<'a> {
    fn poll(&self, now: Instant) -> Option<Result<()>> {
        let mut cache = self.cache.borrow_mut();
        cache.advance_clock(now);
        cache.try_evict(2);
        Some(Ok(()))
    }
}
