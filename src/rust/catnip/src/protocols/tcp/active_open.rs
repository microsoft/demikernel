use super::{
    constants::FALLBACK_MSS,
    established::state::{
        receiver::Receiver,
        sender::Sender,
        ControlBlock,
    },
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ethernet2::frame::{
            EtherType2,
            Ethernet2Header,
        },
        ipv4,
        ipv4::datagram::{
            Ipv4Header,
            Ipv4Protocol2,
        },
        tcp::{
            segment::{
                TcpHeader,
                TcpOptions2,
                TcpSegment,
            },
            SeqNumber,
        },
    },
    runtime::Runtime,
    runtime::RuntimeBuf,
    scheduler::SchedulerHandle,
};
use std::{
    cell::RefCell,
    convert::TryInto,
    future::Future,
    num::Wrapping,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::Duration,
};

struct ConnectResult<RT: Runtime> {
    waker: Option<Waker>,
    result: Option<Result<ControlBlock<RT>, Fail>>,
}

pub struct ActiveOpenSocket<RT: Runtime> {
    local_isn: SeqNumber,

    local: ipv4::Endpoint,
    remote: ipv4::Endpoint,

    rt: RT,
    arp: arp::Peer<RT>,

    #[allow(unused)]
    handle: SchedulerHandle,
    result: Rc<RefCell<ConnectResult<RT>>>,
}

impl<RT: Runtime> ActiveOpenSocket<RT> {
    pub fn new(
        local_isn: SeqNumber,
        local: ipv4::Endpoint,
        remote: ipv4::Endpoint,
        rt: RT,
        arp: arp::Peer<RT>,
    ) -> Self {
        let result = ConnectResult {
            waker: None,
            result: None,
        };
        let result = Rc::new(RefCell::new(result));

        let future = Self::background(
            local_isn,
            local.clone(),
            remote.clone(),
            rt.clone(),
            arp.clone(),
            result.clone(),
        );
        let handle = rt.spawn(future);

        // TODO: Add fast path here when remote is already in the ARP cache (and subtract one retry).
        Self {
            local_isn,
            local,
            remote,
            rt,
            arp,

            handle,
            result,
        }
    }

    pub fn poll_result(&mut self, context: &mut Context) -> Poll<Result<ControlBlock<RT>, Fail>> {
        let mut r = self.result.borrow_mut();
        match r.result.take() {
            None => {
                r.waker.replace(context.waker().clone());
                Poll::Pending
            },
            Some(r) => Poll::Ready(r),
        }
    }

    fn set_result(&mut self, result: Result<ControlBlock<RT>, Fail>) {
        let mut r = self.result.borrow_mut();
        r.waker.take().map(|w| w.wake());
        r.result.replace(result);
    }

    pub fn receive(&mut self, header: &TcpHeader) {
        if header.rst {
            self.set_result(Err(Fail::ConnectionRefused {}));
            return;
        }
        let expected_seq = self.local_isn + Wrapping(1);

        // Bail if we didn't receive a SYN+ACK packet with the right sequence number.
        if !(header.ack && header.syn && header.ack_num == expected_seq) {
            return;
        }

        // Acknowledge the SYN+ACK segment.
        debug!("Received SYN+ACK: {:?}", header);
        let remote_link_addr = match self.arp.try_query(self.remote.address()) {
            Some(r) => r,
            None => panic!("TODO: Clean up ARP query control flow"),
        };
        let remote_seq_num = header.seq_num + Wrapping(1);
        let mut tcp_hdr = TcpHeader::new(self.local.port, self.remote.port);
        tcp_hdr.ack = true;
        tcp_hdr.ack_num = remote_seq_num;
        debug!("Sending ACK: {:?}", tcp_hdr);

        let tcp_options = self.rt.tcp_options();
        let segment = TcpSegment {
            ethernet2_hdr: Ethernet2Header {
                dst_addr: remote_link_addr,
                src_addr: self.rt.local_link_addr(),
                ether_type: EtherType2::Ipv4,
            },
            ipv4_hdr: Ipv4Header::new(self.local.addr, self.remote.addr, Ipv4Protocol2::Tcp),
            tcp_hdr,
            data: RT::Buf::empty(),
            tx_checksum_offload: tcp_options.tx_checksum_offload,
        };
        self.rt.transmit(segment);

        let mut remote_window_scale = None;
        let mut mss = FALLBACK_MSS;
        for option in header.iter_options() {
            match option {
                TcpOptions2::WindowScale(w) => {
                    info!("Received window scale: {}", w);
                    remote_window_scale = Some(*w);
                },
                TcpOptions2::MaximumSegmentSize(m) => {
                    info!("Received advertised MSS: {}", m);
                    mss = *m as usize;
                },
                _ => continue,
            }
        }

        let (local_window_scale, remote_window_scale) = match remote_window_scale {
            Some(w) => (tcp_options.window_scale as u32, w),
            None => (0, 0),
        };

        // TODO(RFC1323): Clamp the scale to 14 instead of panicking.
        assert!(local_window_scale <= 14 && remote_window_scale <= 14);

        let rx_window_size: u32 = (tcp_options.receive_window_size as u32)
            .checked_shl(local_window_scale as u32)
            .expect("TODO: Window size overflow")
            .try_into()
            .expect("TODO: Window size overflow");

        let tx_window_size: u32 = (header.window_size as u32)
            .checked_shl(remote_window_scale as u32)
            .expect("TODO: Window size overflow")
            .try_into()
            .expect("TODO: Window size overflow");

        info!(
            "Window sizes: local {}, remote {}",
            rx_window_size, tx_window_size
        );
        info!(
            "Window scale: local {}, remote {}",
            local_window_scale, remote_window_scale
        );

        let sender = Sender::new(expected_seq, tx_window_size, remote_window_scale, mss);
        let receiver = Receiver::new(remote_seq_num, rx_window_size, local_window_scale);
        let cb = ControlBlock {
            local: self.local.clone(),
            remote: self.remote.clone(),
            rt: self.rt.clone(),
            arp: self.arp.clone(),
            sender,
            receiver,
        };
        self.set_result(Ok(cb));
    }

    fn background(
        local_isn: SeqNumber,
        local: ipv4::Endpoint,
        remote: ipv4::Endpoint,
        rt: RT,
        arp: arp::Peer<RT>,
        result: Rc<RefCell<ConnectResult<RT>>>,
    ) -> impl Future<Output = ()> {
        let tcp_options = rt.tcp_options();
        let handshake_retries = 3usize;
        let handshake_timeout = Duration::from_secs(5);

        async move {
            for _ in 0..handshake_retries {
                let remote_link_addr = match arp.query(remote.address()).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("ARP query failed: {:?}", e);
                        continue;
                    },
                };

                let mut tcp_hdr = TcpHeader::new(local.port, remote.port);
                tcp_hdr.syn = true;
                tcp_hdr.seq_num = local_isn;
                tcp_hdr.window_size = tcp_options.receive_window_size;

                let mss = tcp_options.advertised_mss as u16;
                tcp_hdr.push_option(TcpOptions2::MaximumSegmentSize(mss));
                info!("Advertising MSS: {}", mss);

                tcp_hdr.push_option(TcpOptions2::WindowScale(tcp_options.window_scale));
                info!("Advertising window scale: {}", tcp_options.window_scale);

                debug!("Sending SYN {:?}", tcp_hdr);
                let segment = TcpSegment {
                    ethernet2_hdr: Ethernet2Header {
                        dst_addr: remote_link_addr,
                        src_addr: rt.local_link_addr(),
                        ether_type: EtherType2::Ipv4,
                    },
                    ipv4_hdr: Ipv4Header::new(local.addr, remote.addr, Ipv4Protocol2::Tcp),
                    tcp_hdr,
                    data: RT::Buf::empty(),
                    tx_checksum_offload: tcp_options.tx_checksum_offload,
                };
                rt.transmit(segment);
                rt.wait(handshake_timeout).await;
            }
            let mut r = result.borrow_mut();
            r.waker.take().map(|w| w.wake());
            r.result.replace(Err(Fail::Timeout {}));
        }
    }
}
