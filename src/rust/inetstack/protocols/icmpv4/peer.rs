// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    collections::async_queue::AsyncQueue,
    inetstack::protocols::{
        arp::SharedArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        icmpv4::datagram::{
            self,
            Icmpv4Header,
            Icmpv4Message,
            Icmpv4Type2,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
    },
    runtime::{
        conditional_yield_with_timeout,
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        SharedConditionVariable,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::futures::FutureExt;
use ::rand::{
    prelude::SmallRng,
    Rng,
    SeedableRng,
};
use ::std::{
    collections::HashMap,
    net::Ipv4Addr,
    num::Wrapping,
    ops::{
        Deref,
        DerefMut,
    },
    process,
    time::{
        Duration,
        Instant,
    },
};

/// Arbitrary time out for waiting for pings.
const PING_TIMEOUT: Duration = Duration::from_secs(5);

//==============================================================================
// Icmpv4Peer
//==============================================================================

enum InflightRequest {
    Inflight(SharedConditionVariable),
    Complete,
}

///
/// Internet Control Message Protocol (ICMP)
///
/// This is a supporting protocol for the Internet Protocol version 4 (IPv4)
/// suite. It is used by network devices to send error messages and operational
/// information indicating success or failure when communicating with other
/// peers.
///
/// ICMP for IPv4 is defined in RFC 792.
///
pub struct Icmpv4Peer<N: NetworkRuntime> {
    /// Shared DemiRuntime.
    runtime: SharedDemiRuntime,
    /// Underlying Network Transport
    transport: N,
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,

    /// Underlying ARP Peer
    arp: SharedArpPeer<N>,

    /// Incoming packets
    recv_queue: AsyncQueue<(Ipv4Header, DemiBuffer)>,

    /// Sequence Number
    seq: Wrapping<u16>,

    /// Random number generator
    rng: SmallRng,

    /// Inflight ping requests.
    inflight: HashMap<(u16, u16), InflightRequest>,
}

#[derive(Clone)]
pub struct SharedIcmpv4Peer<N: NetworkRuntime>(SharedObject<Icmpv4Peer<N>>);

impl<N: NetworkRuntime> SharedIcmpv4Peer<N> {
    pub fn new(
        mut runtime: SharedDemiRuntime,
        transport: N,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        arp: SharedArpPeer<N>,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let rng: SmallRng = SmallRng::from_seed(rng_seed);
        let peer: SharedIcmpv4Peer<N> = Self(SharedObject::new(Icmpv4Peer {
            runtime: runtime.clone(),
            transport: transport.clone(),
            local_link_addr,
            local_ipv4_addr,
            arp: arp.clone(),
            recv_queue: AsyncQueue::<(Ipv4Header, DemiBuffer)>::default(),
            seq: Wrapping(0),
            rng,
            inflight: HashMap::<(u16, u16), InflightRequest>::new(),
        }));
        runtime.insert_background_coroutine("Inetstack::ICMP::background", Box::pin(peer.clone().poll().fuse()))?;
        Ok(peer)
    }

    /// Background task for replying to ICMP messages.
    async fn poll(mut self) {
        loop {
            let (ipv4_hdr, buf): (Ipv4Header, DemiBuffer) = match self.recv_queue.pop(Some(PING_TIMEOUT)).await {
                Ok(result) => result,
                Err(_) => break,
            };
            let (icmpv4_hdr, data): (Icmpv4Header, DemiBuffer) = match Icmpv4Header::parse(buf) {
                Ok(result) => result,
                Err(e) => {
                    let cause = "Cannot parse ICMP header";
                    warn!("{}: {:?}", cause, e);
                    continue;
                },
            };
            debug!("ICMPv4 received {:?}", icmpv4_hdr);
            let (id, seq_num, dst_ipv4_addr) = match icmpv4_hdr.get_protocol() {
                Icmpv4Type2::EchoRequest { id, seq_num } => (id, seq_num, ipv4_hdr.get_src_addr()),
                Icmpv4Type2::EchoReply { id, seq_num } => {
                    match self.inflight.get_mut(&(id, seq_num)) {
                        Some(InflightRequest::Inflight(condition_variable)) => condition_variable.signal(),
                        _ => continue,
                    }
                    self.inflight.insert((id, seq_num), InflightRequest::Complete);
                    continue;
                },
                _ => {
                    warn!("Unsupported ICMPv4 message: {:?}", icmpv4_hdr);
                    continue;
                },
            };
            debug!("initiating ARP query");
            let dst_link_addr: MacAddress = match self.arp.query(dst_ipv4_addr).await {
                Ok(dst_link_addr) => dst_link_addr,
                Err(e) => {
                    warn!("reply_to_ping({}, {}, {}) failed: {:?}", dst_ipv4_addr, id, seq_num, e);
                    continue;
                },
            };
            debug!("ARP query complete ({} -> {})", dst_ipv4_addr, dst_link_addr);
            debug!("reply ping ({}, {}, {})", dst_ipv4_addr, id, seq_num);
            // Send reply message.
            let local_link_addr: MacAddress = self.local_link_addr;
            let local_ipv4_addr: Ipv4Addr = self.local_ipv4_addr;
            self.transport.transmit(Box::new(Icmpv4Message::new(
                Ethernet2Header::new(dst_link_addr, local_link_addr, EtherType2::Ipv4),
                Ipv4Header::new(local_ipv4_addr, dst_ipv4_addr, IpProtocol::ICMPv4),
                Icmpv4Header::new(Icmpv4Type2::EchoReply { id, seq_num }, 0),
                data,
            )));
        }
    }

    /// Parses and handles a ICMP message.
    pub fn receive(&mut self, ipv4_hdr: Ipv4Header, buf: DemiBuffer) {
        self.recv_queue.push((ipv4_hdr, buf));
    }

    /// Computes the identifier for an ICMP message.
    fn make_id(&mut self) -> u16 {
        let mut state: u32 = 0xFFFF;
        let addr_octets: [u8; 4] = self.local_ipv4_addr.octets();
        state += u16::from_be_bytes([addr_octets[0], addr_octets[1]]) as u32;
        state += u16::from_be_bytes([addr_octets[2], addr_octets[3]]) as u32;

        let mut pid_buf: [u8; 4] = [0u8; 4];
        pid_buf[0..4].copy_from_slice(&process::id().to_be_bytes());
        state += u16::from_be_bytes([pid_buf[0], pid_buf[1]]) as u32;
        state += u16::from_be_bytes([pid_buf[2], pid_buf[3]]) as u32;

        let nonce: [u8; 2] = self.rng.gen();
        state += u16::from_be_bytes([nonce[0], nonce[1]]) as u32;

        while state > 0xFFFF {
            state -= 0xFFFF;
        }
        !state as u16
    }

    /// Computes sequence number for an ICMP message.
    fn make_seq_num(&mut self) -> u16 {
        let Wrapping(seq_num) = self.seq;
        self.seq += Wrapping(1);
        seq_num
    }

    /// Sends a ping to a remote peer.
    pub async fn ping(&mut self, dst_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        let id: u16 = self.make_id();
        let seq_num: u16 = self.make_seq_num();
        let echo_request: Icmpv4Type2 = Icmpv4Type2::EchoRequest { id, seq_num };

        let t0: Instant = self.runtime.get_now();
        debug!("initiating ARP query");
        let dst_link_addr: MacAddress = self.arp.query(dst_ipv4_addr).await?;
        debug!("ARP query complete ({} -> {})", dst_ipv4_addr, dst_link_addr);

        let data: DemiBuffer = DemiBuffer::new(datagram::ICMPV4_ECHO_REQUEST_MESSAGE_SIZE);

        let msg: Icmpv4Message = Icmpv4Message::new(
            Ethernet2Header::new(dst_link_addr, self.local_link_addr, EtherType2::Ipv4),
            Ipv4Header::new(self.local_ipv4_addr, dst_ipv4_addr, IpProtocol::ICMPv4),
            Icmpv4Header::new(echo_request, 0),
            data,
        );
        self.transport.transmit(Box::new(msg));
        let condition_variable: SharedConditionVariable = SharedConditionVariable::default();
        self.inflight
            .insert((id, seq_num), InflightRequest::Inflight(condition_variable));
        match conditional_yield_with_timeout(
            // Yield into the scheduler until the request completes.
            async {
                while let Some(request) = self.inflight.get(&(id, seq_num)) {
                    match request {
                        InflightRequest::Inflight(condition_variable) => condition_variable.clone().wait().await,
                        InflightRequest::Complete => return,
                    }
                }
            },
            timeout.unwrap_or(PING_TIMEOUT),
        )
        .await
        {
            Ok(_) => {
                self.inflight.remove(&(id, seq_num));
                Ok(self.runtime.get_now() - t0)
            },
            Err(_) => {
                let message: String = format!("timer expired");
                self.inflight.remove(&(id, seq_num));
                error!("ping(): {}", message);
                Err(Fail::new(libc::ETIMEDOUT, &message))
            },
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<N: NetworkRuntime> Deref for SharedIcmpv4Peer<N> {
    type Target = Icmpv4Peer<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<N: NetworkRuntime> DerefMut for SharedIcmpv4Peer<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
