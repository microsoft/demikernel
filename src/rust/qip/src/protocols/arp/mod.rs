mod cache;
mod options;
mod pdu;

#[cfg(test)]
mod tests;

use crate::{
    prelude::*, protocols::ethernet2::MacAddress, r#async::Future, runtime,
};
use cache::ArpCache;
use pdu::{ArpOp, ArpPdu};
use std::{
    any::Any, cell::RefCell, collections::HashMap, convert::TryFrom,
    mem::swap, net::Ipv4Addr, rc::Rc, time::Instant,
};

pub use cache::ArpCacheOptions;
pub use options::ArpOptions;

struct ArpState {
    cache: ArpCache,
}

pub struct Arp<'a> {
    rt: Rc<RefCell<runtime::State<'a>>>,
    state: Rc<RefCell<ArpState>>,
}

impl<'a> Arp<'a> {
    pub fn new(now: Instant, rt: Rc<RefCell<runtime::State<'a>>>) -> Arp<'a> {
        let state = ArpState {
            cache: ArpCache::from_options(now, &rt.borrow().options().arp.cache),
        };

        Arp {
            rt,
            state: Rc::new(RefCell::new(state)),
        }
    }

    pub fn service(&mut self, now: Instant) {
        let mut state = self.state.borrow_mut();
        state.cache.advance_clock(now);
        state.cache.try_evict(2);
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<()> {
        let options = self.rt.borrow().options();
        let mut arp = ArpPdu::try_from(bytes)?;
        // drop request if it's not intended for this station.
        if arp.target_ip_addr != options.my_ipv4_addr {
            return Err(Fail::Ignored {});
        }

        match arp.op {
            ArpOp::ArpRequest => {
                eprintln!("# received arp request");
                arp.op = ArpOp::ArpReply;
                arp.target_link_addr = options.my_link_addr;
                swap(&mut arp.sender_ip_addr, &mut arp.target_ip_addr);
                swap(&mut arp.sender_link_addr, &mut arp.target_link_addr);

                let packet = arp.to_packet()?;
                self.rt.borrow_mut().emit_effect(Effect::Transmit(packet));
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

        let rt = self.rt.clone();
        let state = self.state.clone();
        self.rt.borrow_mut().start_task(move || {
            let options = rt.borrow().options();
            let arp = ArpPdu {
                op: ArpOp::ArpRequest,
                sender_link_addr: options.my_link_addr,
                sender_ip_addr: options.my_ipv4_addr,
                target_link_addr: MacAddress::nil(),
                target_ip_addr: ipv4_addr,
            };

            let packet = arp.to_packet()?;
            rt.borrow_mut().emit_effect(Effect::Transmit(packet));

            // can't make progress until a reply deposits an entry in the
            // cache.
            loop {
                eprintln!("# looking up `{}` in ARP cache...", options.my_ipv4_addr);
                let result = state.borrow().cache.get_link_addr(ipv4_addr).copied();

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
        self.state.borrow().cache.export()
    }
}
