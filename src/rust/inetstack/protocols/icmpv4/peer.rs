// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
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
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        scheduler::Yielder,
        timer::{
            SharedTimer,
            UtilityMethods,
        },
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::futures::{
    channel::{
        mpsc,
        oneshot::{
            channel,
            Receiver,
            Sender,
        },
    },
    FutureExt,
    StreamExt,
};
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

//==============================================================================
// ReqQueue
//==============================================================================

/// Queue of Requests
struct ReqQueue(HashMap<(u16, u16), Sender<()>>);

/// Associate Implementation for ReqQueue
impl ReqQueue {
    /// Creates an empty queue of requests.
    pub fn new() -> Self {
        Self { 0: HashMap::new() }
    }

    /// Inserts a new request in the target queue of  requests.
    pub fn insert(&mut self, req: (u16, u16), tx: Sender<()>) -> Option<Sender<()>> {
        self.0.insert(req, tx)
    }

    /// Removes a request from the target queue of requests.
    pub fn remove(&mut self, req: &(u16, u16)) -> Option<Sender<()>> {
        self.0.remove(req)
    }
}

//==============================================================================
// Icmpv4Peer
//==============================================================================

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
pub struct Icmpv4Peer<const N: usize> {
    /// Shared DemiRuntime.
    runtime: SharedDemiRuntime,
    /// Underlying Network Transport
    transport: SharedBox<dyn NetworkRuntime<N>>,
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,

    /// Underlying ARP Peer
    arp: SharedArpPeer<N>,

    /// Transmitter
    tx: mpsc::UnboundedSender<(Ipv4Addr, u16, u16, DemiBuffer)>,

    /// Queue of Requests
    requests: ReqQueue,

    /// Sequence Number
    seq: Wrapping<u16>,

    rng: SmallRng,
}

pub struct SharedIcmpv4Peer<const N: usize>(SharedObject<Icmpv4Peer<N>>);

impl<const N: usize> SharedIcmpv4Peer<N> {
    pub fn new(
        mut runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        arp: SharedArpPeer<N>,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let (tx, rx): (
            mpsc::UnboundedSender<(Ipv4Addr, u16, u16, DemiBuffer)>,
            mpsc::UnboundedReceiver<(Ipv4Addr, u16, u16, DemiBuffer)>,
        ) = mpsc::unbounded();
        runtime.insert_background_coroutine(
            "Inetstack::ICMP::background",
            Box::pin(Self::background(
                transport.clone(),
                local_link_addr,
                local_ipv4_addr,
                arp.clone(),
                rx,
            )),
        )?;
        let requests = ReqQueue::new();
        let rng: SmallRng = SmallRng::from_seed(rng_seed);
        Ok(Self(SharedObject::new(Icmpv4Peer {
            runtime: runtime.clone(),
            transport: transport.clone(),
            local_link_addr,
            local_ipv4_addr,
            arp: arp.clone(),
            tx,
            requests,
            seq: Wrapping(0),
            rng,
        })))
    }

    /// Background task for replying to ICMP messages.
    async fn background(
        mut transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        mut arp: SharedArpPeer<N>,
        mut rx: mpsc::UnboundedReceiver<(Ipv4Addr, u16, u16, DemiBuffer)>,
    ) {
        // Reply requests.
        while let Some((dst_ipv4_addr, id, seq_num, data)) = rx.next().await {
            debug!("initiating ARP query");
            let dst_link_addr: MacAddress = match arp.query(dst_ipv4_addr, &Yielder::new()).await {
                Ok(dst_link_addr) => dst_link_addr,
                Err(e) => {
                    warn!("reply_to_ping({}, {}, {}) failed: {:?}", dst_ipv4_addr, id, seq_num, e);
                    continue;
                },
            };
            debug!("ARP query complete ({} -> {})", dst_ipv4_addr, dst_link_addr);
            debug!("reply ping ({}, {}, {})", dst_ipv4_addr, id, seq_num);
            // Send reply message.
            transport.transmit(Box::new(Icmpv4Message::new(
                Ethernet2Header::new(dst_link_addr, local_link_addr, EtherType2::Ipv4),
                Ipv4Header::new(local_ipv4_addr, dst_ipv4_addr, IpProtocol::ICMPv4),
                Icmpv4Header::new(Icmpv4Type2::EchoReply { id, seq_num }, 0),
                data,
            )));
        }
    }

    /// Parses and handles a ICMP message.
    pub fn receive(&mut self, ipv4_header: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        let (icmpv4_hdr, data) = Icmpv4Header::parse(buf)?;
        debug!("ICMPv4 received {:?}", icmpv4_hdr);
        match icmpv4_hdr.get_protocol() {
            Icmpv4Type2::EchoRequest { id, seq_num } => {
                self.tx
                    .unbounded_send((ipv4_header.get_src_addr(), id, seq_num, data))
                    .unwrap();
            },
            Icmpv4Type2::EchoReply { id, seq_num } => {
                if let Some(tx) = self.requests.remove(&(id, seq_num)) {
                    let _ = tx.send(());
                }
            },
            _ => {
                warn!("Unsupported ICMPv4 message: {:?}", icmpv4_hdr);
            },
        }
        Ok(())
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

    /// Sends a ping to a remote peer.Wrapping
    pub async fn ping(&mut self, dst_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        let timeout: Duration = timeout.unwrap_or_else(|| Duration::from_millis(5000));
        let id: u16 = self.make_id();
        let seq_num: u16 = self.make_seq_num();
        let echo_request: Icmpv4Type2 = Icmpv4Type2::EchoRequest { id, seq_num };

        let t0: Instant = self.runtime.get_now();
        debug!("initiating ARP query");
        let dst_link_addr: MacAddress = self.arp.query(dst_ipv4_addr, &Yielder::new()).await?;
        debug!("ARP query complete ({} -> {})", dst_ipv4_addr, dst_link_addr);

        let data: DemiBuffer = DemiBuffer::new(datagram::ICMPV4_ECHO_REQUEST_MESSAGE_SIZE);

        let msg: Icmpv4Message = Icmpv4Message::new(
            Ethernet2Header::new(dst_link_addr, self.local_link_addr, EtherType2::Ipv4),
            Ipv4Header::new(self.local_ipv4_addr, dst_ipv4_addr, IpProtocol::ICMPv4),
            Icmpv4Header::new(echo_request, 0),
            data,
        );
        self.transport.transmit(Box::new(msg));
        let rx: Receiver<()> = {
            let (tx, rx) = channel();
            assert!(self.requests.insert((id, seq_num), tx).is_none());
            rx
        };
        let yielder: Yielder = Yielder::new();
        let clock_ref: SharedTimer = self.runtime.get_timer();
        let timer = clock_ref.wait(timeout, &yielder);
        match rx.fuse().with_timeout(timer).await? {
            // Request completed successfully.
            Ok(_) => Ok(self.runtime.get_now() - t0),
            // Request expired.
            Err(_) => {
                let message: String = format!("timer expired");
                self.requests.remove(&(id, seq_num));
                error!("ping(): {}", message);
                Err(Fail::new(libc::ETIMEDOUT, &message))
            },
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<const N: usize> Deref for SharedIcmpv4Peer<N> {
    type Target = Icmpv4Peer<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedIcmpv4Peer<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
