// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    cache::ArpCache,
    packet::{
        ArpHeader,
        ArpMessage,
        ArpOperation,
    },
};
use crate::{
    inetstack::{
        futures::UtilityMethods,
        protocols::ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            config::ArpConfig,
            types::MacAddress,
            NetworkRuntime,
        },
        queue::BackgroundTask,
        timer::TimerRc,
    },
    scheduler::{
        Scheduler,
        TaskHandle,
    },
};
use ::futures::{
    channel::oneshot::{
        channel,
        Receiver,
        Sender,
    },
    FutureExt,
};
use ::libc::{
    EBADMSG,
    ETIMEDOUT,
};
use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    collections::{
        HashMap,
        LinkedList,
    },
    future::Future,
    net::Ipv4Addr,
    rc::Rc,
    time::Duration,
};

//==============================================================================
// Structures
//==============================================================================

///
/// Arp Peer
///
#[derive(Clone)]
pub struct ArpPeer<const N: usize> {
    rt: Rc<dyn NetworkRuntime<N>>,
    clock: TimerRc,
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,
    cache: Rc<RefCell<ArpCache>>,
    waiters: Rc<RefCell<HashMap<Ipv4Addr, LinkedList<Sender<MacAddress>>>>>,
    arp_config: ArpConfig,

    /// The background co-routine cleans up the ARP cache from time to time.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: Rc<TaskHandle>,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl<const N: usize> ArpPeer<N> {
    pub fn new(
        rt: Rc<dyn NetworkRuntime<N>>,
        scheduler: Scheduler,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        arp_config: ArpConfig,
    ) -> Result<ArpPeer<N>, Fail> {
        let cache: Rc<RefCell<ArpCache>> = Rc::new(RefCell::new(ArpCache::new(
            clock.clone(),
            Some(arp_config.get_cache_ttl()),
            Some(arp_config.get_initial_values()),
            arp_config.get_disable_arp(),
        )));

        // This is a future returned by the async function.
        let task: BackgroundTask = BackgroundTask::new(
            String::from("Inetstack::arp::background"),
            Box::pin(Self::background(clock.clone(), cache.clone())),
        );
        let handle: TaskHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => {
                return Err(Fail::new(
                    libc::EAGAIN,
                    "failed to schedule background co-routine for ARP module",
                ))
            },
        };
        let peer: ArpPeer<N> = ArpPeer {
            rt,
            clock,
            local_link_addr,
            local_ipv4_addr,
            cache,
            waiters: Rc::new(RefCell::new(HashMap::default())),
            arp_config,
            background: Rc::new(handle),
        };

        Ok(peer)
    }

    /// Drops a waiter for a target IP address.
    fn do_drop(&mut self, ipv4_addr: Ipv4Addr) {
        self.waiters.borrow_mut().remove(&ipv4_addr);
    }

    fn do_insert(&mut self, ipv4_addr: Ipv4Addr, link_addr: MacAddress) -> Option<MacAddress> {
        if let Some(wait_queue) = self.waiters.borrow_mut().remove(&ipv4_addr) {
            for sender in wait_queue {
                let _ = sender.send(link_addr);
            }
        }
        self.cache.borrow_mut().insert(ipv4_addr, link_addr)
    }

    fn do_wait_link_addr(&mut self, ipv4_addr: Ipv4Addr) -> impl Future<Output = MacAddress> {
        let (tx, rx): (Sender<MacAddress>, Receiver<MacAddress>) = channel();
        if let Some(&link_addr) = self.cache.borrow().get(ipv4_addr) {
            let _ = tx.send(link_addr);
        } else {
            let mut waiters: RefMut<HashMap<Ipv4Addr, LinkedList<Sender<MacAddress>>>> = self.waiters.borrow_mut();
            if let Some(wait_queue) = waiters.get_mut(&ipv4_addr) {
                warn!("Duplicate waiter for IP address: {}", ipv4_addr);
                wait_queue.push_back(tx);
            } else {
                let mut wait_queue: LinkedList<Sender<MacAddress>> = LinkedList::new();
                wait_queue.push_back(tx);
                waiters.insert(ipv4_addr, wait_queue);
            }
        }
        rx.map(|r| r.expect("Dropped waiter?"))
    }

    /// Background task that cleans up the ARP cache from time to time.
    async fn background(clock: TimerRc, cache: Rc<RefCell<ArpCache>>) {
        loop {
            let current_time = clock.now();
            {
                let mut cache = cache.borrow_mut();
                cache.advance_clock(current_time);
                // TODO: re-enable eviction once TCP/IP stack is fully functional.
                // cache.clear();
            }
            clock.wait(clock.clone(), Duration::from_secs(1)).await;
        }
    }

    pub fn receive(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        // from RFC 826:
        // > ?Do I have the hardware type in ar$hrd?
        // > [optionally check the hardware length ar$hln]
        // > ?Do I speak the protocol in ar$pro?
        // > [optionally check the protocol length ar$pln]
        let header = ArpHeader::parse(buf)?;
        debug!("Received {:?}", header);

        // from RFC 826:
        // > Merge_flag := false
        // > If the pair <protocol type, sender protocol address> is
        // > already in my translation table, update the sender
        // > hardware address field of the entry with the new
        // > information in the packet and set Merge_flag to true.
        let merge_flag = {
            if self.cache.borrow().get(header.get_sender_protocol_addr()).is_some() {
                self.do_insert(header.get_sender_protocol_addr(), header.get_sender_hardware_addr());
                true
            } else {
                false
            }
        };
        // from RFC 826: ?Am I the target protocol address?
        if header.get_destination_protocol_addr() != self.local_ipv4_addr {
            if merge_flag {
                // we did do something.
                return Ok(());
            } else {
                // we didn't do anything.
                return Err(Fail::new(EBADMSG, "unrecognized IP address"));
            }
        }
        // from RFC 826:
        // > If Merge_flag is false, add the triplet <protocol type,
        // > sender protocol address, sender hardware address> to
        // > the translation table.
        if !merge_flag {
            self.do_insert(header.get_sender_protocol_addr(), header.get_sender_hardware_addr());
        }

        match header.get_operation() {
            ArpOperation::Request => {
                // from RFC 826:
                // > Swap hardware and protocol fields, putting the local
                // > hardware and protocol addresses in the sender fields.
                let reply = ArpMessage::new(
                    Ethernet2Header::new(header.get_sender_hardware_addr(), self.local_link_addr, EtherType2::Arp),
                    ArpHeader::new(
                        ArpOperation::Reply,
                        self.local_link_addr,
                        self.local_ipv4_addr,
                        header.get_sender_hardware_addr(),
                        header.get_sender_protocol_addr(),
                    ),
                );
                debug!("Responding {:?}", reply);
                self.rt.transmit(Box::new(reply));
                Ok(())
            },
            ArpOperation::Reply => {
                debug!(
                    "reply from `{}/{}`",
                    header.get_sender_protocol_addr(),
                    header.get_sender_hardware_addr()
                );
                self.cache
                    .borrow_mut()
                    .insert(header.get_sender_protocol_addr(), header.get_sender_hardware_addr());
                Ok(())
            },
        }
    }

    pub fn try_query(&self, ipv4_addr: Ipv4Addr) -> Option<MacAddress> {
        self.cache.borrow().get(ipv4_addr).cloned()
    }

    pub fn query(&self, ipv4_addr: Ipv4Addr) -> impl Future<Output = Result<MacAddress, Fail>> {
        let rt = self.rt.clone();
        let mut arp = self.clone();
        let cache = self.cache.clone();
        let arp_options = self.arp_config.clone();
        let clock: TimerRc = self.clock.clone();
        let local_link_addr: MacAddress = self.local_link_addr.clone();
        let local_ipv4_addr: Ipv4Addr = self.local_ipv4_addr.clone();
        async move {
            if let Some(&link_addr) = cache.borrow().get(ipv4_addr) {
                return Ok(link_addr);
            }
            let msg = ArpMessage::new(
                Ethernet2Header::new(MacAddress::broadcast(), local_link_addr, EtherType2::Arp),
                ArpHeader::new(
                    ArpOperation::Request,
                    local_link_addr,
                    local_ipv4_addr,
                    MacAddress::broadcast(),
                    ipv4_addr,
                ),
            );
            let mut arp_response = arp.do_wait_link_addr(ipv4_addr).fuse();

            // from TCP/IP illustrated, chapter 4:
            // > The frequency of the ARP request is very close to one per
            // > second, the maximum suggested by [RFC1122].
            let result = {
                for i in 0..arp_options.get_retry_count() + 1 {
                    rt.transmit(Box::new(msg.clone()));
                    let timer = clock.wait(clock.clone(), arp_options.get_request_timeout());

                    match arp_response.with_timeout(timer).await {
                        Ok(link_addr) => {
                            debug!("ARP result available ({})", link_addr);
                            return Ok(link_addr);
                        },
                        Err(_) => {
                            warn!("ARP request timeout; attempt {}.", i + 1);
                        },
                    }
                }
                Err(Fail::new(ETIMEDOUT, "ARP query timeout"))
            };

            arp.do_drop(ipv4_addr);

            result
        }
    }

    #[cfg(test)]
    pub fn export_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.cache.borrow().export()
    }
}
