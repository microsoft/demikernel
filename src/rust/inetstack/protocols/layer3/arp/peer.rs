// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    collections::async_queue::AsyncQueue,
    demikernel::config::Config,
    expect_ok,
    inetstack::{
        config::ArpConfig,
        protocols::{
            layer2::SharedLayer2Endpoint,
            layer3::arp::{
                cache::ArpCache,
                header::{ArpHeader, ArpOperation},
            },
        },
        types::MacAddress,
    },
    runtime::{conditional_yield_with_timeout, fail::Fail, memory::DemiBuffer, SharedDemiRuntime, SharedObject},
};
use ::futures::{
    channel::oneshot::{channel, Receiver, Sender},
    FutureExt,
};
use ::libc::ETIMEDOUT;
use ::std::{
    collections::{HashMap, LinkedList},
    net::Ipv4Addr,
    ops::{Deref, DerefMut},
    time::Duration,
};

//======================================================================================================================
// Constants
//======================================================================================================================

//======================================================================================================================
// Structures
//======================================================================================================================

///
/// Arp Peer
///
pub struct ArpPeer {
    layer2_endpoint: SharedLayer2Endpoint,
    local_ipv4_addr: Ipv4Addr,
    cache: ArpCache,
    waiters: HashMap<Ipv4Addr, LinkedList<Sender<MacAddress>>>,
    arp_config: ArpConfig,
    recv_queue: AsyncQueue<DemiBuffer>,
}

#[derive(Clone)]
pub struct SharedArpPeer(SharedObject<ArpPeer>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl SharedArpPeer {
    /// ARP Cleanup timeout.
    const ARP_CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

    pub fn new(
        config: &Config,
        mut runtime: SharedDemiRuntime,
        layer2_endpoint: SharedLayer2Endpoint,
    ) -> Result<Self, Fail> {
        let arp_config: ArpConfig = ArpConfig::new(config)?;
        let cache: ArpCache = ArpCache::new(
            runtime.get_now(),
            Some(arp_config.get_cache_ttl()),
            Some(arp_config.get_initial_values()),
            arp_config.is_enabled(),
        );

        let peer: SharedArpPeer = Self(SharedObject::new(ArpPeer {
            layer2_endpoint,
            local_ipv4_addr: config.local_ipv4_addr()?,
            cache,
            waiters: HashMap::default(),
            arp_config,
            recv_queue: AsyncQueue::<DemiBuffer>::default(),
        }));
        // This is a future returned by the async function.
        runtime.insert_coroutine(
            "bgc::inetstack::arp::background",
            None,
            Box::pin(peer.clone().poll().fuse()),
        )?;
        Ok(peer.clone())
    }

    /// Insert a packet for processing.
    pub fn receive(&mut self, buf: DemiBuffer) {
        self.recv_queue.push(buf)
    }

    /// Drops a waiter for a target IP address.
    fn do_drop(&mut self, ipv4_addr: Ipv4Addr) {
        self.waiters.remove(&ipv4_addr);
    }

    fn do_insert(&mut self, ipv4_addr: Ipv4Addr, link_addr: MacAddress) -> Option<MacAddress> {
        if let Some(wait_queue) = self.waiters.remove(&ipv4_addr) {
            for sender in wait_queue {
                let _ = sender.send(link_addr);
            }
        }
        self.cache.insert(ipv4_addr, link_addr)
    }

    async fn do_wait_link_addr(&mut self, ipv4_addr: Ipv4Addr) -> MacAddress {
        let (tx, rx): (Sender<MacAddress>, Receiver<MacAddress>) = channel();
        if let Some(&link_addr) = self.cache.get(ipv4_addr) {
            let _ = tx.send(link_addr);
        } else {
            if let Some(wait_queue) = self.waiters.get_mut(&ipv4_addr) {
                warn!("Duplicate waiter for IP address: {}", ipv4_addr);
                wait_queue.push_back(tx);
            } else {
                let mut wait_queue: LinkedList<Sender<MacAddress>> = LinkedList::new();
                wait_queue.push_back(tx);
                self.waiters.insert(ipv4_addr, wait_queue);
            }
        }
        expect_ok!(rx.await, "Dropped waiter?")
    }

    async fn poll(mut self) {
        loop {
            let buf: DemiBuffer = match self.recv_queue.pop(Some(Self::ARP_CLEANUP_TIMEOUT)).await {
                Ok(buf) => buf,
                Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT || errno == libc::EAGAIN => continue,
                Err(_) => break,
            };
            // from RFC 826:
            // > ?Do I have the hardware type in ar$hrd?
            // > [optionally check the hardware length ar$hln]
            // > ?Do I speak the protocol in ar$pro?
            // > [optionally check the protocol length ar$pln]
            let header: ArpHeader = match ArpHeader::parse_and_consume(buf) {
                Ok(header) => header,
                Err(e) => {
                    let cause: String = format!("could not parse ARP header:");
                    warn!("arp_cache::poll(): {} {:?}", &cause, e);
                    continue;
                },
            };
            debug!("Received {:?}", header);

            // from RFC 826:
            // > Merge_flag := false
            // > If the pair <protocol type, sender protocol address> is
            // > already in my translation table, update the sender
            // > hardware address field of the entry with the new
            // > information in the packet and set Merge_flag to true.
            let merge_flag: bool = {
                if self.cache.get(header.get_sender_protocol_addr()).is_some() {
                    trace!(
                        "poll(): updating the arp cache (link_addr={:?}, ipv4_addr={:?})",
                        header.get_sender_hardware_addr(),
                        header.get_sender_protocol_addr()
                    );
                    self.do_insert(header.get_sender_protocol_addr(), header.get_sender_hardware_addr());
                    true
                } else {
                    trace!(
                        "poll(): arp cache miss (link_addr={:?}, ipv4_addr={:?})",
                        header.get_sender_hardware_addr(),
                        header.get_sender_protocol_addr()
                    );
                    false
                }
            };
            // from RFC 826: ?Am I the target protocol address?
            if header.get_destination_protocol_addr() != self.local_ipv4_addr {
                if !merge_flag {
                    // we didn't do something.
                    let cause: String = format!("unrecognized IP address");
                    warn!("arp_cache::poll(): {}", &cause);
                }
                trace!(
                    "poll(): dropping arp packet (link_addr={:?}, ipv4_addr={:?})",
                    header.get_sender_hardware_addr(),
                    header.get_sender_protocol_addr()
                );
                continue;
            }
            // from RFC 826:
            // > If Merge_flag is false, add the triplet <protocol type,
            // > sender protocol address, sender hardware address> to
            // > the translation table.
            if !merge_flag {
                trace!(
                    "poll(): adding entry to the arp cache (link_addr={:?}, ipv4_addr={:?})",
                    header.get_sender_hardware_addr(),
                    header.get_sender_protocol_addr()
                );
                self.do_insert(header.get_sender_protocol_addr(), header.get_sender_hardware_addr());
            }

            match header.get_operation() {
                ArpOperation::Request => {
                    // from RFC 826:
                    // > Swap hardware and protocol fields, putting the local
                    // > hardware and protocol addresses in the sender fields.
                    let reply_hdr: ArpHeader = ArpHeader::new(
                        ArpOperation::Reply,
                        self.layer2_endpoint.get_local_link_addr(),
                        self.local_ipv4_addr,
                        header.get_sender_hardware_addr(),
                        header.get_sender_protocol_addr(),
                    );
                    debug!("Responding {:?}", reply_hdr);

                    if let Err(e) = self
                        .layer2_endpoint
                        .transmit_arp_packet(header.get_sender_hardware_addr(), reply_hdr.create_and_serialize())
                    {
                        // Ignore for now because the other end will retry.
                        // TODO: Implement a retry mechanism so we do not have to wait for the other end to time out.
                        // FIXME: https://github.com/microsoft/demikernel/issues/1365
                        warn!("Could not transmit message: {:?}", e);
                    }
                },
                ArpOperation::Reply => {
                    debug!(
                        "reply from `{}/{}`",
                        header.get_sender_protocol_addr(),
                        header.get_sender_hardware_addr()
                    );
                    self.cache
                        .insert(header.get_sender_protocol_addr(), header.get_sender_hardware_addr());
                },
            }
        }
    }

    pub fn try_query(&self, ipv4_addr: Ipv4Addr) -> Option<MacAddress> {
        self.cache.get(ipv4_addr).cloned()
    }

    pub async fn query(&mut self, ipv4_addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        if let Some(&link_addr) = self.cache.get(ipv4_addr) {
            return Ok(link_addr);
        }
        let header: ArpHeader = ArpHeader::new(
            ArpOperation::Request,
            self.layer2_endpoint.get_local_link_addr(),
            self.local_ipv4_addr,
            MacAddress::broadcast(),
            ipv4_addr,
        );
        let mut peer: SharedArpPeer = self.clone();
        // from TCP/IP illustrated, chapter 4:
        // > The frequency of the ARP request is very close to one per
        // > second, the maximum suggested by [RFC1122].
        let result = {
            for i in 0..self.arp_config.get_retry_count() + 1 {
                if let Err(e) = self
                    .layer2_endpoint
                    .transmit_arp_packet(MacAddress::broadcast(), header.create_and_serialize())
                {
                    warn!("Could not send packet: {:?}", e);
                    continue;
                }
                let arp_response = peer.do_wait_link_addr(ipv4_addr);

                match conditional_yield_with_timeout(arp_response, self.arp_config.get_request_timeout()).await {
                    Ok(link_addr) => {
                        debug!("ARP result available ({:?})", link_addr);
                        return Ok(link_addr);
                    },
                    Err(_) => {
                        warn!("ARP request timeout; attempt {}.", i + 1);
                    },
                }
            }
            let cause: String = format!("query(): query timeout (ipv4_addr={:?})", ipv4_addr);
            error!("{}", &cause);
            Err(Fail::new(ETIMEDOUT, &cause))
        };

        self.do_drop(ipv4_addr);

        result
    }

    #[cfg(test)]
    pub fn export_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.cache.export()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedArpPeer {
    type Target = ArpPeer;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedArpPeer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
