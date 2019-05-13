mod options;

#[cfg(test)]
mod tests;

use eui48::MacAddress;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

pub use options::ArpCacheOptions;

#[derive(Debug, Clone)]
struct Record {
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    expires: Instant,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Expiry {
    ipv4_addr: Ipv4Addr,
    when: Instant,
}

impl Ord for Expiry {
    fn cmp(&self, other: &Expiry) -> Ordering {
        // `BinaryHeap` is a max-heap, so we need to reverse the order of comparisons in order to get `peek()` and `pop()` to return the smallest time.
        match self.when.cmp(&other.when) {
            Ordering::Equal => Ordering::Equal,
            Ordering::Less => Ordering::Greater,
            Ordering::Greater => Ordering::Less,
        }
    }
}

impl PartialOrd for Expiry {
    fn partial_cmp(&self, other: &Expiry) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct ArpCache {
    records: HashMap<Ipv4Addr, Record>,
    rmap: HashMap<MacAddress, Ipv4Addr>,
    expiries: BinaryHeap<Expiry>,
    ttl: Duration,
    now: Instant,
}

impl ArpCache {
    pub fn new(ttl: Duration, now: Instant) -> ArpCache {
        assert!(ttl > Duration::new(0, 0));
        ArpCache {
            records: HashMap::new(),
            rmap: HashMap::new(),
            expiries: BinaryHeap::new(),
            ttl,
            now,
        }
    }

    pub fn insert(
        &mut self,
        ipv4_addr: Ipv4Addr,
        link_addr: MacAddress
    ) {
        let expires = self.now + self.ttl;
        let record = Record {
            ipv4_addr,
            link_addr,
            expires,
        };

        if let Some(_) = self.records.insert(ipv4_addr, record) {
            panic!(
                "redundant attempt to insert station (`{}`) into ARP cache",
                ipv4_addr
            );
        }

        // the following operations are expected to succeed now that we have succeeded to insert into `self.records`.
        assert!(self.rmap.insert(link_addr, ipv4_addr).is_none());

        let expiry = Expiry {
            ipv4_addr,
            when: expires,
        };

        self.expiries.push(expiry);
    }

    pub fn remove(&mut self, ipv4_addr: &Ipv4Addr) {
        if let Some(record) = self.records.remove(ipv4_addr) {
            assert!(self.rmap.remove(&record.link_addr).is_some());
        } else {
            panic!(
                "attempt to remove unrecognized station (`{}`) from ARP cache",
                ipv4_addr
            );
        }
    }

    pub fn get_link_addr(
        &self,
        ipv4_addr: &Ipv4Addr
    ) -> Option<&MacAddress> {
        if let Some(record) = self.records.get(ipv4_addr) {
            if self.now < record.expires {
                return Some(&record.link_addr);
            }
        }

        None
    }

    pub fn get_ipv4_addr(
        &self,
        link_addr: &MacAddress
    ) -> Option<&Ipv4Addr> {
        if let Some(ipv4_addr) = self.rmap.get(link_addr) {
            let record = self.records.get(ipv4_addr).unwrap();
            if self.now < record.expires {
                return Some(ipv4_addr);
            }
        }

        None
    }

    pub fn touch(&mut self, now: Instant) {
        self.now = now;

        loop {
            if let Some(when) = self.try_evict() {
                if when != now {
                    continue;
                }
            }

            break;
        }
    }

    fn try_evict(&mut self) -> Option<Instant> {
        let expiry = match self.expiries.peek() {
            Some(e) => (*e).clone(),
            None => return None,
        };

        if self.now < expiry.when {
            return None;
        }

        assert!(self.expiries.pop().is_some());
        let record = match self.records.get(&expiry.ipv4_addr) {
            Some(r) => (*r).clone(),
            None => return Some(expiry.when),
        };

        self.remove(&record.ipv4_addr);
        Some(expiry.when)
    }
}
