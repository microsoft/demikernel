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
    inetstack::protocols::ethernet2::{
        EtherType2,
        Ethernet2Header,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            config::ArpConfig,
            types::MacAddress,
            NetworkRuntime,
        },
        timer::UtilityMethods,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
    scheduler::Yielder,
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
    collections::{
        HashMap,
        LinkedList,
    },
    net::Ipv4Addr,
    ops::{
        Deref,
        DerefMut,
    },
    time::Duration,
};

//==============================================================================
// Constants
//==============================================================================

//==============================================================================
// Structures
//==============================================================================

///
/// Arp Peer
///
pub struct ArpPeer<const N: usize> {
    runtime: SharedDemiRuntime,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,
    cache: ArpCache,
    waiters: HashMap<Ipv4Addr, LinkedList<Sender<MacAddress>>>,
    arp_config: ArpConfig,
}

#[derive(Clone)]
pub struct SharedArpPeer<const N: usize>(SharedObject<ArpPeer<N>>);

//==============================================================================
// Associate Functions
//==============================================================================

impl<const N: usize> ArpPeer<N> {
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        arp_config: ArpConfig,
    ) -> Result<ArpPeer<N>, Fail> {
        let cache: ArpCache = ArpCache::new(
            runtime.get_timer(),
            Some(arp_config.get_cache_ttl()),
            Some(arp_config.get_initial_values()),
            arp_config.get_disable_arp(),
        );

        Ok(Self {
            runtime,
            transport,
            local_link_addr,
            local_ipv4_addr,
            cache,
            waiters: HashMap::default(),
            arp_config,
        })
    }
}

impl<const N: usize> SharedArpPeer<N> {
    /// ARP Cleanup timeout.
    const ARP_CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

    pub fn new(
        mut runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        arp_config: ArpConfig,
    ) -> Result<Self, Fail> {
        let peer: SharedArpPeer<N> = Self(SharedObject::<ArpPeer<N>>::new(ArpPeer::<N>::new(
            runtime.clone(),
            transport,
            local_link_addr,
            local_ipv4_addr,
            arp_config,
        )?));
        let mut peer2: SharedArpPeer<N> = peer.clone();
        // This is a future returned by the async function.
        runtime.insert_background_coroutine(
            "Inetstack::arp::background",
            Box::pin(async move { peer2.background().await }),
        )?;
        Ok(peer.clone())
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
        rx.await.expect("Dropped waiter?")
    }

    /// Background task that cleans up the ARP cache from time to time.
    async fn background(&mut self) {
        let yielder: Yielder = Yielder::new();
        loop {
            match self.runtime.get_timer().wait(Self::ARP_CLEANUP_TIMEOUT, &yielder).await {
                Ok(()) => continue,
                Err(_) => break,
            }
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
            if self.cache.get(header.get_sender_protocol_addr()).is_some() {
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
                self.transport.transmit(Box::new(reply));
                Ok(())
            },
            ArpOperation::Reply => {
                debug!(
                    "reply from `{}/{}`",
                    header.get_sender_protocol_addr(),
                    header.get_sender_hardware_addr()
                );
                self.cache
                    .insert(header.get_sender_protocol_addr(), header.get_sender_hardware_addr());
                Ok(())
            },
        }
    }

    pub fn try_query(&self, ipv4_addr: Ipv4Addr) -> Option<MacAddress> {
        self.cache.get(ipv4_addr).cloned()
    }

    pub async fn query(&mut self, ipv4_addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        if let Some(&link_addr) = self.cache.get(ipv4_addr) {
            return Ok(link_addr);
        }
        let msg = ArpMessage::new(
            Ethernet2Header::new(MacAddress::broadcast(), self.local_link_addr, EtherType2::Arp),
            ArpHeader::new(
                ArpOperation::Request,
                self.local_link_addr,
                self.local_ipv4_addr,
                MacAddress::broadcast(),
                ipv4_addr,
            ),
        );
        let mut peer: SharedArpPeer<N> = self.clone();
        let mut arp_response = Box::pin(peer.do_wait_link_addr(ipv4_addr).fuse());

        // from TCP/IP illustrated, chapter 4:
        // > The frequency of the ARP request is very close to one per
        // > second, the maximum suggested by [RFC1122].
        let result = {
            let yielder: Yielder = Yielder::new();
            for i in 0..self.arp_config.get_retry_count() + 1 {
                self.transport.transmit(Box::new(msg.clone()));
                let timer = self
                    .runtime
                    .get_timer()
                    .wait(self.arp_config.get_request_timeout(), &yielder);

                match arp_response.with_timeout(timer).await {
                    Ok(link_addr) => {
                        debug!("ARP result available ({:?})", link_addr);
                        return Ok(link_addr);
                    },
                    Err(_) => {
                        warn!("ARP request timeout; attempt {}.", i + 1);
                    },
                }
            }
            Err(Fail::new(ETIMEDOUT, "ARP query timeout"))
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

impl<const N: usize> Deref for SharedArpPeer<N> {
    type Target = ArpPeer<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedArpPeer<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
