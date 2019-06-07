use super::{
    cache::ArpCache,
    pdu::{ArpOp, ArpPdu},
};
use crate::{prelude::*, protocols::ethernet2::MacAddress, r#async::Future};
use float_duration::FloatDuration;
use std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    convert::TryFrom,
    mem::swap,
    net::Ipv4Addr,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct ArpPeer<'a> {
    rt: Runtime<'a>,
    cache: Rc<RefCell<ArpCache>>,
}

impl<'a> ArpPeer<'a> {
    pub fn new(now: Instant, rt: Runtime<'a>) -> Result<ArpPeer<'a>> {
        let options = rt.options();
        let cache_ttl = options
            .arp
            .cache_ttl
            .unwrap_or_else(|| FloatDuration::seconds(20.0))
            .to_std()?;
        let cache = ArpCache::new(now, Some(cache_ttl));
        Ok(ArpPeer {
            rt,
            cache: Rc::new(RefCell::new(cache)),
        })
    }

    pub fn service(&mut self) {
        let mut cache = self.cache.borrow_mut();
        cache.advance_clock(self.rt.clock());
        cache.try_evict(2);
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<()> {
        let options = self.rt.options();
        // from RFC 826:
        // > ?Do I have the hardware type in ar$hrd?
        // > [optionally check the hardware length ar$hln]
        // > ?Do I speak the protocol in ar$pro?
        // > [optionally check the protocol length ar$pln]
        let mut arp = ArpPdu::try_from(bytes)?;
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
                return Err(Fail::Ignored {});
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
                debug!(
                    "ArpPeer::receive(): request from `{}`",
                    arp.sender_link_addr
                );
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
                let packet = arp.to_packet()?;
                self.rt.emit_effect(Effect::Transmit(packet));
                Ok(())
            }
            ArpOp::ArpReply => {
                debug!(
                    "ArpPeer::receive(): reply from `{}/{}`",
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
        self.rt.start_task(move || {
            let options = rt.options();
            let arp = ArpPdu {
                op: ArpOp::ArpRequest,
                sender_link_addr: options.my_link_addr,
                sender_ip_addr: options.my_ipv4_addr,
                target_link_addr: MacAddress::nil(),
                target_ip_addr: ipv4_addr,
            };

            let packet = arp.to_packet()?;
            let timeout = options
                .arp
                .request_timeout
                .unwrap_or_else(|| FloatDuration::seconds(5.0))
                .to_std()?;
            let mut retries_remaining =
                options.arp.retry_count.unwrap_or(20) + 1;
            while retries_remaining > 0 {
                rt.emit_effect(Effect::Transmit(packet.clone()));
                retries_remaining -= 1;
                // can't make progress until a reply deposits an entry in the
                // cache.
                let t0 = rt.clock();
                let mut dt = Duration::new(0, 0);
                while dt < timeout {
                    debug!(
                        "ArpPeer::query(): looking up `{}` in cache...",
                        options.my_ipv4_addr
                    );
                    let result =
                        cache.borrow().get_link_addr(ipv4_addr).copied();

                    if let Some(link_addr) = result {
                        let x: Rc<Any> = Rc::new(link_addr);
                        return Ok(x);
                    } else {
                        // unable to make progress until the appropriate entry
                        // is inserted into the cache.
                        yield None;
                        dt = rt.clock() - t0;
                        debug!("ArpPeer::query(): dt = {:?}", dt);
                    }
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
}
