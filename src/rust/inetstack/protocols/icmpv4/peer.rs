// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::{
        futures::UtilityMethods,
        protocols::{
            arp::ArpPeer,
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
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        queue::BackgroundTask,
        timer::{
            TimerRc,
            WaitFuture,
        },
    },
    scheduler::{
        Scheduler,
        SchedulerHandle,
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
    cell::RefCell,
    collections::HashMap,
    future::Future,
    net::Ipv4Addr,
    num::Wrapping,
    process,
    rc::Rc,
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
pub struct Icmpv4Peer {
    /// Underlying Runtime
    rt: Rc<dyn NetworkRuntime>,

    clock: TimerRc,

    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,

    /// Underlying ARP Peer
    arp: ArpPeer,

    /// Transmitter
    tx: mpsc::UnboundedSender<(Ipv4Addr, u16, u16, DemiBuffer)>,

    /// Queue of Requests
    requests: Rc<RefCell<ReqQueue>>,

    /// Sequence Number
    seq: Wrapping<u16>,

    rng: Rc<RefCell<SmallRng>>,

    /// The background co-routine relies to incoming PING requests.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: SchedulerHandle,
}

impl Icmpv4Peer {
    /// Creates a new peer for handling ICMP.
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        arp: ArpPeer,
        rng_seed: [u8; 32],
    ) -> Result<Icmpv4Peer, Fail> {
        let (tx, rx) = mpsc::unbounded();
        let requests = ReqQueue::new();
        let rng: Rc<RefCell<SmallRng>> = Rc::new(RefCell::new(SmallRng::from_seed(rng_seed)));
        let task: BackgroundTask = BackgroundTask::new(
            String::from("Inetstack::ICMP::background"),
            Box::pin(Self::background(
                rt.clone(),
                local_link_addr,
                local_ipv4_addr,
                arp.clone(),
                rx,
            )),
        );
        let handle: SchedulerHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => {
                let message: String = format!("failed to schedule background co-routine for ICMPv4 module");
                error!("{}", message);
                return Err(Fail::new(libc::EAGAIN, &message));
            },
        };
        Ok(Icmpv4Peer {
            rt,
            clock,
            local_link_addr,
            local_ipv4_addr,
            arp,
            tx,
            requests: Rc::new(RefCell::new(requests)),
            seq: Wrapping(0),
            rng,
            background: handle,
        })
    }

    /// Background task for replying to ICMP messages.
    async fn background(
        rt: Rc<dyn NetworkRuntime>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        arp: ArpPeer,
        mut rx: mpsc::UnboundedReceiver<(Ipv4Addr, u16, u16, DemiBuffer)>,
    ) {
        // Reply requests.
        while let Some((dst_ipv4_addr, id, seq_num, data)) = rx.next().await {
            debug!("initiating ARP query");
            let dst_link_addr: MacAddress = match arp.query(dst_ipv4_addr).await {
                Ok(dst_link_addr) => dst_link_addr,
                Err(e) => {
                    warn!("reply_to_ping({}, {}, {}) failed: {:?}", dst_ipv4_addr, id, seq_num, e);
                    continue;
                },
            };
            debug!("ARP query complete ({} -> {})", dst_ipv4_addr, dst_link_addr);
            debug!("reply ping ({}, {}, {})", dst_ipv4_addr, id, seq_num);
            // Send reply message.
            rt.transmit(Box::new(Icmpv4Message::new(
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
                if let Some(tx) = self.requests.borrow_mut().remove(&(id, seq_num)) {
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
    fn make_id(&self) -> u16 {
        let mut state: u32 = 0xFFFF;
        let addr_octets: [u8; 4] = self.local_ipv4_addr.octets();
        state += u16::from_be_bytes([addr_octets[0], addr_octets[1]]) as u32;
        state += u16::from_be_bytes([addr_octets[2], addr_octets[3]]) as u32;

        let mut pid_buf: [u8; 4] = [0u8; 4];
        pid_buf[0..4].copy_from_slice(&process::id().to_be_bytes());
        state += u16::from_be_bytes([pid_buf[0], pid_buf[1]]) as u32;
        state += u16::from_be_bytes([pid_buf[2], pid_buf[3]]) as u32;

        let nonce: [u8; 2] = self.rng.borrow_mut().gen();
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
    pub fn ping(
        &mut self,
        dst_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Duration, Fail>> {
        let timeout: Duration = timeout.unwrap_or_else(|| Duration::from_millis(5000));
        let id: u16 = self.make_id();
        let seq_num: u16 = self.make_seq_num();
        let echo_request: Icmpv4Type2 = Icmpv4Type2::EchoRequest { id, seq_num };
        let arp: ArpPeer = self.arp.clone();
        let rt: Rc<dyn NetworkRuntime> = self.rt.clone();
        let clock: TimerRc = self.clock.clone();
        let requests: Rc<RefCell<ReqQueue>> = self.requests.clone();
        let local_link_addr: MacAddress = self.local_link_addr.clone();
        let local_ipv4_addr: Ipv4Addr = self.local_ipv4_addr.clone();
        async move {
            let t0: Instant = clock.now();
            debug!("initiating ARP query");
            let dst_link_addr: MacAddress = arp.query(dst_ipv4_addr).await?;
            debug!("ARP query complete ({} -> {})", dst_ipv4_addr, dst_link_addr);

            let data: DemiBuffer = DemiBuffer::new(datagram::ICMPV4_ECHO_REQUEST_MESSAGE_SIZE);

            let msg: Icmpv4Message = Icmpv4Message::new(
                Ethernet2Header::new(dst_link_addr, local_link_addr, EtherType2::Ipv4),
                Ipv4Header::new(local_ipv4_addr, dst_ipv4_addr, IpProtocol::ICMPv4),
                Icmpv4Header::new(echo_request, 0),
                data,
            );
            rt.transmit(Box::new(msg));
            let rx: Receiver<()> = {
                let (tx, rx) = channel();
                assert!(requests.borrow_mut().insert((id, seq_num), tx).is_none());
                rx
            };
            let timer: WaitFuture<TimerRc> = clock.wait(clock.clone(), timeout);
            match rx.fuse().with_timeout(timer).await? {
                // Request completed successfully.
                Ok(_) => Ok(clock.now() - t0),
                // Request expired.
                Err(_) => {
                    let message: String = format!("timer expired");
                    requests.borrow_mut().remove(&(id, seq_num));
                    error!("ping(): {}", message);
                    Err(Fail::new(libc::ETIMEDOUT, &message))
                },
            }
        }
    }
}
