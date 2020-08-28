// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(clippy::cognitive_complexity)]

use crate::retry::Retry;
use super::super::{
    connection::TcpConnectionHandle,
    segment::{TcpSegment, TcpSegmentDecoder},
};
use std::future::Future;
use futures::FutureExt;
use futures::task::{Context, noop_waker_ref};
use std::task::Poll;
use crate::{
    prelude::*,
    protocols::{ip, ipv4},
    test,
};
use fxhash::FxHashMap;
use std::{
    iter,
    num::Wrapping,
    time::{Duration, Instant},
};

struct EstablishedConnection {
    alice: Engine,
    alice_cxn_handle: TcpConnectionHandle,
    bob: Engine,
    bob_cxn_handle: TcpConnectionHandle,
    now: Instant,
}

#[test]
fn syn_to_closed_port() {
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let mut alice = test::new_alice(now);
    alice.import_arp_cache(
        iter::once((*test::bob_ipv4_addr(), *test::bob_link_addr()))
            .collect::<FxHashMap<_, _>>(),
    );

    let mut bob = test::new_bob(now);
    bob.import_arp_cache(
        iter::once((*test::alice_ipv4_addr(), *test::alice_link_addr()))
            .collect::<FxHashMap<_, _>>(),
    );

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.tcp_connect(ipv4::Endpoint::new(*test::bob_ipv4_addr(), bob_port)).boxed_local();
    let (tcp_syn, private_port) = {
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        let bytes = match &*event {
            Event::Transmit(segment) => segment.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(segment.header().syn());
        let src_port = segment.header().src_port().unwrap();
        debug!("private_port => {:?}", src_port);
        (bytes, src_port)
    };

    info!("passing TCP SYN to bob...");
    let now = now + Duration::from_micros(1);
    bob.receive(&tcp_syn).unwrap();
    bob.advance_clock(now);
    let event = bob.pop_event().unwrap();
    let tcp_rst = {
        let bytes = match &*event {
            Event::Transmit(bytes) => bytes,
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let bytes = bytes.borrow().to_vec();
        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(segment.header().rst());
        assert_eq!(Some(private_port), segment.header().dest_port());
        bytes
    };

    info!("passing TCP RST segment to alice...");
    let now = now + Duration::from_micros(1);
    alice.receive(&tcp_rst).unwrap();
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());
    let now = now + Duration::from_micros(1);
    alice.advance_clock(now);
    match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Err(Fail::ConnectionRefused {})) => (),
        _ => panic!("expected `Fail::ConnectionRefused {{}}`"),
    }
}

fn establish_connection() -> EstablishedConnection {
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let mut alice = test::new_alice(now);
    alice.import_arp_cache(
        iter::once((*test::bob_ipv4_addr(), *test::bob_link_addr()))
            .collect::<FxHashMap<_, _>>(),
    );

    let mut bob = test::new_bob(now);
    bob.import_arp_cache(
        iter::once((*test::alice_ipv4_addr(), *test::alice_link_addr()))
            .collect::<FxHashMap<_, _>>(),
    );

    bob.tcp_listen(bob_port).unwrap();

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice
        .tcp_connect(ipv4::Endpoint::new(*test::bob_ipv4_addr(), bob_port)).boxed_local();
    let (tcp_syn, private_port, alice_isn) = {
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        let bytes = match &*event {
            Event::Transmit(segment) => segment.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(segment.header().syn());
        assert!(!segment.header().ack());
        let src_port = segment.header().src_port().unwrap();
        debug!("private_port => {:?}", src_port);
        let alice_isn = segment.header().seq_num();
        (bytes, src_port, alice_isn)
    };

    info!("passing TCP SYN to bob...");
    let now = now + Duration::from_micros(1);
    bob.receive(&tcp_syn).unwrap();
    bob.advance_clock(now);
    let event = bob.pop_event().unwrap();
    let (tcp_syn_ack, bob_isn) = {
        let bytes = match &*event {
            Event::Transmit(bytes) => bytes,
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let bytes = bytes.borrow().to_vec();
        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        let bob_isn = segment.header().seq_num();
        assert!(segment.header().syn());
        assert!(segment.header().ack());
        assert_eq!(Some(private_port), segment.header().dest_port());
        assert_eq!(segment.header().ack_num(), alice_isn + Wrapping(1));
        assert_eq!(
            usize::from(segment.header().window_size()),
            alice.options().tcp.receive_window_size
        );
        (bytes, bob_isn)
    };

    info!("passing TCP SYN+ACK segment to alice...");
    let now = now + Duration::from_micros(1);
    alice.receive(&tcp_syn_ack).unwrap();
    alice.advance_clock(now);
    let event = alice.pop_event().unwrap();
    let tcp_ack = {
        let bytes = match &*event {
            Event::Transmit(bytes) => bytes,
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let bytes = bytes.borrow().to_vec();
        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(!segment.header().syn());
        assert!(segment.header().ack());
        assert_eq!(Some(private_port), segment.header().src_port());
        assert_eq!(segment.header().seq_num(), alice_isn + Wrapping(1));
        assert_eq!(segment.header().ack_num(), bob_isn + Wrapping(1));
        assert_eq!(
            usize::from(segment.header().window_size()),
            alice.options().tcp.receive_window_size
        );
        bytes
    };

    info!("passing TCP ACK segment to bob...");
    let now = now + Duration::from_micros(1);
    bob.receive(&tcp_ack).unwrap();
    bob.advance_clock(now);
    let event = bob.pop_event().unwrap();
    let bob_cxn_handle = match &*event {
        Event::IncomingTcpConnection(h) => *h,
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let alice_cxn_handle = match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Ok(h)) => h,
        x => panic!("Unexpected result: {:?}", x),
    };
    info!(
        "connection established; alice isn = {:?}, bob isn = {:?}",
        alice_isn, bob_isn,
    );
    EstablishedConnection {
        alice,
        alice_cxn_handle,
        bob,
        bob_cxn_handle,
        now,
    }
}

#[test]
fn unfragmented_data_exchange() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in = vec![0xab; 10];
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let (bytes, seq_num) = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(*segment.payload, data_in);
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, *handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    info!("Reading from Bob's TCP receive window...");
    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, *data_out);
    assert!(cxn.bob.tcp_read(cxn.bob_cxn_handle).is_err());

    info!("Bob writes data to the TCP connection...");
    cxn.bob
        .tcp_write(cxn.bob_cxn_handle, data_in.clone())
        .unwrap();
    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    let event = cxn.bob.pop_event().unwrap();
    let (bytes, seq_num) = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(*segment.payload, data_in);
            assert!(segment.ack);
            assert_eq!(
                seq_num + Wrapping(u32::try_from(data_in.len()).unwrap()),
                segment.ack_num
            );
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing data segment to Alice...");
    cxn.now += Duration::from_micros(1);
    cxn.alice.receive(bytes.as_slice()).unwrap();
    cxn.alice.advance_clock(cxn.now);
    match &*cxn.alice.pop_event().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.alice_cxn_handle, *handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    info!("waiting for trailing ACK timeout to pass...");
    cxn.now += cxn.alice.options().tcp.trailing_ack_delay;
    cxn.alice.advance_clock(cxn.now);
    assert!(cxn.alice.pop_event().is_none());

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let pure_ack = match &*cxn.alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.payload.len());
            assert!(segment.ack);
            assert_eq!(
                seq_num + Wrapping(u32::try_from(data_in.len()).unwrap()),
                segment.ack_num
            );
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing pure ACK segment to Bob...");
    cxn.bob.receive(pure_ack.as_slice()).unwrap();

    info!("Reading from Alice's TCP buffer...");
    let data_out = cxn.alice.tcp_read(cxn.alice_cxn_handle).unwrap();
    assert_eq!(data_in, *data_out);
    assert!(cxn.alice.tcp_read(cxn.bob_cxn_handle).is_err());

    // alice is going to kick out a single window advertisement after reading.
    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    match &*cxn.alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };
    assert!(cxn.alice.pop_event().is_none());
}

#[test]
fn packetization() {
    let mut cxn = establish_connection();

    // transmitting 2k bytes of data should produce an equivalent `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in =
        vec![0xab; cxn.alice.tcp_mss(cxn.alice_cxn_handle).unwrap() + 1];
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let (bytes0, seq_num) = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let bytes1 = match &*event {
        Event::Transmit(bytes) => bytes.borrow().to_vec(),
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing data segments to Bob...");
    cxn.now += Duration::from_micros(1);
    // ACK timeout starts from here.
    cxn.bob.receive(bytes0.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, *handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes1.as_slice()).unwrap();
    // Event::TcpBytesAvailable won't be emitted unless the read buffer starts
    // out empty.
    cxn.bob.advance_clock(cxn.now);
    assert!(cxn.bob.pop_event().is_none());

    info!("waiting for trailing ACK timeout to pass...");
    cxn.now +=
        cxn.bob.options().tcp.trailing_ack_delay - Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    assert!(cxn.bob.pop_event().is_none());

    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    let pure_ack = match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.payload.len());
            assert!(segment.ack);
            assert_eq!(
                seq_num + Wrapping(u32::try_from(data_in.len()).unwrap()),
                segment.ack_num
            );
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing pure ACK segment to Alice...");
    cxn.alice.receive(pure_ack.as_slice()).unwrap();

    info!("Reading from Bob's TCP receive window...");
    let mut data_out: Vec<u8> = Vec::new();
    data_out.extend(&*cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap());
    data_out.extend(&*cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap());
    assert_eq!(data_in, data_out);
    assert!(cxn.bob.tcp_read(cxn.bob_cxn_handle).is_err());

    // bob is going to kick out a single window advertisement after reading.
    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };
    assert!(cxn.bob.pop_event().is_none());
}

#[test]
fn multiple_writes() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in = vec![0xab; 10];
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let (bytes, seq_num) = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(*segment.payload, data_in);
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    // ACK timeout starts from here.
    cxn.bob.receive(bytes.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, *handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    info!("Alice writes more data to the TCP connection...");
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let bytes = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(*segment.payload, data_in);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing second data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes.as_slice()).unwrap();
    // Event::TcpBytesAvailable won't be emitted unless the read buffer starts
    // out empty.
    cxn.bob.advance_clock(cxn.now);
    assert!(cxn.bob.pop_event().is_none());

    info!("waiting for trailing ACK timeout to pass...");
    cxn.now +=
        cxn.bob.options().tcp.trailing_ack_delay - Duration::from_micros(2);
    cxn.bob.advance_clock(cxn.now);
    assert!(cxn.bob.pop_event().is_none());

    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    let pure_ack = match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.payload.len());
            assert!(segment.ack);
            assert_eq!(
                seq_num + Wrapping(u32::try_from(data_in.len()).unwrap() * 2),
                segment.ack_num
            );
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing pure ACK segment to Alice...");
    cxn.alice.receive(pure_ack.as_slice()).unwrap();

    info!("Reading from Bob's TCP receive window...");
    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, *data_out);
    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, *data_out);
    assert!(cxn.bob.tcp_read(cxn.bob_cxn_handle).is_err());

    // bob is going to kick out a single window advertisement after reading.
    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };
    assert!(cxn.bob.pop_event().is_none());
}

#[test]
fn syn_retry() {
    let bob_port = ip::Port::try_from(12345).unwrap();

    let mut now = Instant::now();
    let mut alice = test::new_alice(now);
    alice.import_arp_cache(
        iter::once((*test::bob_ipv4_addr(), *test::bob_link_addr()))
            .collect::<FxHashMap<_, _>>(),
    );

    let options = alice.options();
    let retries = options.tcp.handshake_retries;
    let mut retry = Retry::new(options.tcp.handshake_timeout, retries);

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice
        .tcp_connect(ipv4::Endpoint::new(*test::bob_ipv4_addr(), bob_port))
        .boxed_local();
    alice.advance_clock(now);
    let event = alice.pop_event().unwrap();
    match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert!(segment.syn);
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    for i in 0..(retries - 1) {
        let timeout = retry.fail().unwrap();
        now += timeout;
        assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());
        info!("syn_retry(): retry #{}", i + 1);
        now += Duration::from_micros(1);
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        match &*event {
            Event::Transmit(bytes) => {
                let bytes = bytes.borrow().to_vec();
                let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
                assert!(segment.syn);
            }
            e => panic!("got unanticipated event `{:?}`", e),
        }
    }

    now += retry.fail().unwrap();
    assert!(retry.fail().is_none());
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());
    now += Duration::from_micros(1);
    alice.advance_clock(now);
    match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Err(Fail::Timeout {})) => (),
        _ => panic!("expected timeout"),
    }
}

#[test]
fn retransmission_fail() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in = vec![0xab; 10];
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let dropped_segment = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(*segment.payload, data_in);
            segment
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let rto = cxn.alice.tcp_rto(cxn.alice_cxn_handle).unwrap();
    let retries = cxn.alice.options().tcp.retries;
    let mut retry = Retry::new(rto, retries);
    for i in 0..(retries - 1) {
        let timeout = retry.fail().unwrap();
        cxn.now += timeout;
        cxn.alice.advance_clock(cxn.now);
        assert!(cxn.alice.pop_event().is_none());
        info!("retransmission(): retry #{}", i + 1);
        cxn.now += Duration::from_micros(1);
        cxn.alice.advance_clock(cxn.now);
        let event = cxn.alice.pop_event().unwrap();
        match &*event {
            Event::Transmit(bytes) => {
                let bytes = bytes.borrow().to_vec();
                assert_eq!(
                    dropped_segment,
                    TcpSegment::decode(bytes.as_slice()).unwrap()
                );
            }
            e => panic!("got unanticipated event `{:?}`", e),
        }
    }

    cxn.now += retry.fail().unwrap();
    assert!(retry.fail().is_none());
    cxn.alice.advance_clock(cxn.now);
    assert!(cxn.alice.pop_event().is_none());
    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    match &*cxn.alice.pop_event().unwrap() {
        Event::TcpConnectionClosed { handle, error } => {
            assert_eq!(*handle, cxn.alice_cxn_handle);
            match error {
                Some(Fail::Timeout {}) => (),
                _ => panic!("expected a timeout error"),
            }
        }
        _ => panic!("unexpected event"),
    }
}

#[test]
fn retransmission_ok() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in = vec![0xab; 10];
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    // retransmission timer starts here.
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let first_dropped_segment = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(*segment.payload, data_in);
            segment
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let second_dropped_segment = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(*segment.payload, data_in);
            segment
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("dropping data segments and attempting retransmission...");
    let rto = cxn.alice.tcp_rto(cxn.alice_cxn_handle).unwrap();
    let mut retry = Retry::new(rto, cxn.alice.options().tcp.retries);
    let timeout = retry.fail().unwrap();
    cxn.now += timeout - Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    assert!(cxn.alice.pop_event().is_none());
    cxn.now += Duration::from_micros(1);
    let bytes0 = {
        cxn.alice.advance_clock(cxn.now);
        match &*cxn.alice.pop_event().unwrap() {
            Event::Transmit(bytes) => {
                let bytes = bytes.borrow().to_vec();
                let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
                assert_eq!(first_dropped_segment, segment);
                bytes
            }
            e => panic!("got unanticipated event `{:?}`", e),
        }
    };

    let bytes1 = {
        cxn.alice.advance_clock(cxn.now);
        match &*cxn.alice.pop_event().unwrap() {
            Event::Transmit(bytes) => {
                let bytes = bytes.borrow().to_vec();
                let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
                assert_eq!(second_dropped_segment, segment);
                bytes
            }
            e => panic!("got unanticipated event `{:?}`", e),
        }
    };

    info!("passing data segments to Bob...");
    cxn.now += Duration::from_micros(1);
    // ACK timeout starts from here.
    cxn.bob.receive(bytes0.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, *handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    cxn.bob.receive(bytes1.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    assert!(cxn.bob.pop_event().is_none());

    info!("Alice writes more data to the TCP connection...");
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let event = cxn.alice.pop_event().unwrap();
    let bytes = match &*event {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(*segment.payload, data_in);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing third data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes.as_slice()).unwrap();
    // Event::TcpBytesAvailable won't be emitted unless the read buffer starts
    // out empty.
    cxn.bob.advance_clock(cxn.now);
    assert!(cxn.bob.pop_event().is_none());

    info!("waiting for trailing ACK timeout to pass...");
    cxn.now +=
        cxn.bob.options().tcp.trailing_ack_delay - Duration::from_micros(2);
    cxn.bob.advance_clock(cxn.now);
    assert!(cxn.bob.pop_event().is_none());

    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    let pure_ack = match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.payload.len());
            assert!(segment.ack);
            assert_eq!(
                second_dropped_segment.seq_num
                    + Wrapping(u32::try_from(data_in.len()).unwrap() * 2),
                segment.ack_num
            );
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing pure ACK segment to Alice...");
    cxn.alice.receive(pure_ack.as_slice()).unwrap();

    info!("Reading from Bob's TCP buffer...");
    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, *data_out);
    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, *data_out);
    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, *data_out);
    assert!(cxn.bob.tcp_read(cxn.bob_cxn_handle).is_err());

    // bob is going to kick out a single window advertisement after reading.
    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };
    assert!(cxn.bob.pop_event().is_none());
}

#[test]
fn flow_control() {
    let mut cxn = establish_connection();
    info!(
        "flow_control(): recieve_window_size = {}",
        cxn.bob.options().tcp.receive_window_size
    );

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("flow_control(): Alice writes data to the TCP connection...");
    let data_in = vec![0xab; cxn.bob.options().tcp.receive_window_size];
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let bytes0 = match &*cxn.alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let _ = TcpSegment::decode(bytes.as_slice()).unwrap();
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let bytes1 = match &*cxn.alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_ne!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("flow_control(): passing data segments to Bob...");
    cxn.now += Duration::from_micros(1);
    // ACK timeout starts from here.
    cxn.bob.receive(bytes0.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, *handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes1.as_slice()).unwrap();
    // Event::TcpBytesAvailable won't be emitted unless the read buffer starts
    // out empty.
    cxn.bob.advance_clock(cxn.now);
    let zero_window_advertisement = match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.window_size);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("flow_control(): passing zero window advertisement to Alice...");
    cxn.now += Duration::from_micros(1);
    cxn.alice
        .receive(zero_window_advertisement.as_slice())
        .unwrap();
    cxn.alice.advance_clock(cxn.now);
    assert!(cxn.alice.pop_event().is_none());

    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();
    cxn.alice.advance_clock(cxn.now);
    assert!(cxn.alice.pop_event().is_none());

    info!(
        "flow_control(): waiting for Alice to start sending window probes..."
    );
    let rto = cxn.alice.tcp_rto(cxn.alice_cxn_handle).unwrap();
    let retries = cxn.alice.options().tcp.retries;
    let mut retry = Retry::new(rto, retries);
    for i in 0..(retries - 1) {
        let timeout = retry.fail().unwrap();
        cxn.now += timeout;
        cxn.alice.advance_clock(cxn.now);
        assert!(cxn.alice.pop_event().is_none());
        info!("flow_control(): try #{}", i + 1);
        cxn.now += Duration::from_micros(1);
        cxn.alice.advance_clock(cxn.now);
        let window_probe = match &*cxn.alice.pop_event().unwrap() {
            Event::Transmit(bytes) => {
                let bytes = bytes.borrow().to_vec();
                let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
                assert_eq!(segment.payload.len(), 1);
                bytes
            }
            e => panic!("got unanticipated event `{:?}`", e),
        };

        debug!("flow_control(): passing window probe to Bob...");
        cxn.bob.receive(window_probe.as_slice()).unwrap();
        let zero_window_advertisement = {
            cxn.bob.advance_clock(cxn.now);
            match &*cxn.bob.pop_event().unwrap() {
                Event::Transmit(bytes) => {
                    let bytes = bytes.borrow().to_vec();
                    let segment =
                        TcpSegment::decode(bytes.as_slice()).unwrap();
                    assert_eq!(0, segment.window_size);
                    bytes
                }
                e => panic!("got unanticipated event `{:?}`", e),
            }
        };

        info!("flow_control(): passing zero window advertisement to Alice...");
        cxn.now += Duration::from_micros(1);
        cxn.alice
            .receive(zero_window_advertisement.as_slice())
            .unwrap();
        cxn.alice.advance_clock(cxn.now);
        assert!(cxn.alice.pop_event().is_none());

        // this is acceptable, since we increment the time by `timeout` when
        // we start a new loop iteration.
        cxn.now -= Duration::from_micros(1);
    }

    info!("flow_control(): reading available bytes from Bob's TCP window...");
    let mut data_out: Vec<u8> = Vec::new();
    data_out.extend(&*cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap());
    data_out.extend(&*cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap());
    assert_eq!(data_in, data_out);
    assert!(cxn.bob.tcp_read(cxn.bob_cxn_handle).is_err());

    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    let window_advertisement = match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_ne!(0, segment.window_size);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("flow_control(): passing window advertisement to Alice...");
    cxn.now += Duration::from_micros(1);
    cxn.alice.receive(window_advertisement.as_slice()).unwrap();
    cxn.alice.advance_clock(cxn.now);
    let bytes0 = match &*cxn.alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.len(), 1);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let bytes1 = match &*cxn.alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_ne!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.now += Duration::from_micros(1);
    cxn.alice.advance_clock(cxn.now);
    let bytes2 = match &*cxn.alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_ne!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("flow_control(): passing data segments to Bob...");
    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes0.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, *handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes1.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    assert!(cxn.bob.pop_event().is_none());

    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes2.as_slice()).unwrap();
    cxn.bob.advance_clock(cxn.now);
    let _ = match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.window_size);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("flow_control(): reading available bytes from Bob's TCP window...");
    let mut data_out: Vec<u8> = Vec::new();
    // there's three segments because one is a window probe.
    data_out.extend(&*cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap());
    data_out.extend(&*cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap());
    data_out.extend(&*cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap());
    assert_eq!(data_in, data_out);
    assert!(cxn.bob.tcp_read(cxn.bob_cxn_handle).is_err());

    // bob is going to kick out a single window advertisement after reading.
    cxn.now += Duration::from_micros(1);
    cxn.bob.advance_clock(cxn.now);
    match &*cxn.bob.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };
    assert!(cxn.bob.pop_event().is_none());
}
