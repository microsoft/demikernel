mod cache;
mod options;
mod pdu;

#[cfg(test)]
mod tests;

use crate::{prelude::*, protocols::ethernet2::MacAddress, r#async::Future};
use cache::ArpCache;
use float_duration::FloatDuration;
use pdu::{ArpOp, ArpPdu};
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

pub use cache::ArpCacheOptions;
pub use options::ArpOptions;

struct ArpState {
    cache: ArpCache,
}

pub struct Arp<'a> {
    rt: Runtime<'a>,
    state: Rc<RefCell<ArpState>>,
}

impl<'a> Arp<'a> {
    pub fn new(now: Instant, rt: Runtime<'a>) -> Arp<'a> {
        let state = ArpState {
            cache: ArpCache::from_options(now, &rt.options().arp.cache),
        };

        Arp {
            rt,
            state: Rc::new(RefCell::new(state)),
        }
    }

    pub fn service(&mut self) {
        let mut state = self.state.borrow_mut();
        state.cache.advance_clock(self.rt.clock());
        state.cache.try_evict(2);
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
            let mut state = self.state.borrow_mut();
            if state.cache.get_link_addr(arp.sender_ip_addr).is_some() {
                state.cache.insert(arp.sender_ip_addr, arp.sender_link_addr);
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
            self.state
                .borrow_mut()
                .cache
                .insert(arp.sender_ip_addr, arp.sender_link_addr);
        }

        match arp.op {
            ArpOp::ArpRequest => {
                eprintln!("# received arp request");
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
                eprintln!(
                    "# received arp reply: `{}` -> `{}`",
                    arp.sender_ip_addr, arp.sender_link_addr
                );
                let mut state = self.state.borrow_mut();
                state.cache.insert(arp.sender_ip_addr, arp.sender_link_addr);
                Ok(())
            }
        }
    }

    pub fn query(&self, ipv4_addr: Ipv4Addr) -> Future<'a, MacAddress> {
        {
            let state = self.state.borrow();
            if let Some(link_addr) = state.cache.get_link_addr(ipv4_addr) {
                return Future::r#const(*link_addr);
            }
        }

        let mut rt = self.rt.clone();
        let state = self.state.clone();
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
                    eprintln!(
                        "# looking up `{}` in ARP cache...",
                        options.my_ipv4_addr
                    );
                    let result =
                        state.borrow().cache.get_link_addr(ipv4_addr).copied();

                    if let Some(link_addr) = result {
                        let x: Rc<Any> = Rc::new(link_addr);
                        return Ok(x);
                    } else {
                        // unable to make progress until the appropriate entry
                        // is inserted into the cache.
                        yield None;
                        dt = rt.clock() - t0;
                        eprintln!("# dt = {:?}", dt);
                    }
                }

                eprintln!(
                    "ARP request timeout; {} retries remain.",
                    retries_remaining
                );
            }

            Err(Fail::Timeout {})
        })
    }

    pub fn export_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.state.borrow().cache.export()
    }
}
