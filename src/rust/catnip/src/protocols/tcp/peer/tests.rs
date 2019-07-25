use super::super::segment::TcpSegmentDecoder;
use crate::{
    prelude::*,
    protocols::{ip, ipv4},
    r#async::Async,
    test,
};
use std::{
    num::Wrapping,
    time::{Duration, Instant},
};

#[test]
fn syn_to_closed_port() {
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let mut alice = test::new_alice(now);
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let mut bob = test::new_bob(now);
    bob.import_arp_cache(hashmap! {
        *test::alice_ipv4_addr() => *test::alice_link_addr(),
    });

    let fut = alice
        .tcp_connect(ipv4::Endpoint::new(*test::bob_ipv4_addr(), bob_port));
    let (tcp_syn, private_port) = {
        let event = alice.poll(now).unwrap().unwrap();
        let bytes = match event {
            Event::Transmit(segment) => segment.to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(segment.header().syn());
        let src_port = segment.header().src_port().unwrap();
        debug!("private_port => {:?}", src_port);
        (bytes, src_port)
    };

    info!("passing TCP SYN to bob...");
    let now = now + Duration::from_millis(1);
    bob.receive(&tcp_syn).unwrap();
    let event = bob.poll(now).unwrap().unwrap();
    let tcp_rst = {
        let bytes = match event {
            Event::Transmit(bytes) => bytes,
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(segment.header().rst());
        assert_eq!(Some(private_port), segment.header().dest_port());
        bytes
    };

    info!("passing TCP RST segment to alice...");
    let now = now + Duration::from_millis(1);
    alice.receive(&tcp_rst).unwrap();
    assert!(fut.poll(now).is_none());
    let now = now + Duration::from_millis(1);
    match fut.poll(now).unwrap() {
        Err(Fail::ConnectionRefused {}) => (),
        _ => panic!("expected `Fail::ConnectionRefused {{}}`"),
    }
}

#[test]
fn handshake() {
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let mut alice = test::new_alice(now);
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let mut bob = test::new_bob(now);
    bob.import_arp_cache(hashmap! {
        *test::alice_ipv4_addr() => *test::alice_link_addr(),
    });

    bob.tcp_listen(bob_port).unwrap();

    let fut = alice
        .tcp_connect(ipv4::Endpoint::new(*test::bob_ipv4_addr(), bob_port));
    let (tcp_syn, private_port, syn_seq_num) = {
        let event = alice.poll(now).unwrap().unwrap();
        let bytes = match event {
            Event::Transmit(segment) => segment.to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(segment.header().syn());
        assert!(!segment.header().ack());
        let src_port = segment.header().src_port().unwrap();
        debug!("private_port => {:?}", src_port);
        let syn_seq_num = segment.header().seq_num();
        (bytes, src_port, syn_seq_num)
    };

    info!("passing TCP SYN to bob...");
    let now = now + Duration::from_millis(1);
    bob.receive(&tcp_syn).unwrap();
    let event = bob.poll(now).unwrap().unwrap();
    let (tcp_syn_ack, syn_ack_seq_num) = {
        let bytes = match event {
            Event::Transmit(bytes) => bytes,
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        let syn_ack_seq_num = segment.header().seq_num();
        assert!(segment.header().syn());
        assert!(segment.header().ack());
        assert_eq!(Some(private_port), segment.header().dest_port());
        assert_eq!(segment.header().ack_num(), syn_seq_num + Wrapping(1));
        (bytes, syn_ack_seq_num)
    };

    info!("passing TCP SYN+ACK segment to alice...");
    let now = now + Duration::from_millis(1);
    alice.receive(&tcp_syn_ack).unwrap();
    let event = alice.poll(now).unwrap().unwrap();
    let tcp_ack = {
        let bytes = match event {
            Event::Transmit(bytes) => bytes,
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(!segment.header().syn());
        assert!(segment.header().ack());
        assert_eq!(Some(private_port), segment.header().src_port());
        assert_eq!(segment.header().seq_num(), syn_seq_num + Wrapping(1));
        assert_eq!(segment.header().ack_num(), syn_ack_seq_num + Wrapping(1));
        bytes
    };

    info!("passing TCP ACK segment to bob...");
    let now = now + Duration::from_millis(1);
    bob.receive(&tcp_ack).unwrap();
    let event = bob.poll(now).unwrap().unwrap();
    match event {
        Event::TcpConnectionEstablished(_) => (),
        e => panic!("got unanticipated event `{:?}`", e),
    }

    assert!(fut.poll(now).unwrap().is_ok());
}

#[test]
fn syn_retry() {
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let mut alice = test::new_alice(now);
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let fut = alice
        .tcp_connect(ipv4::Endpoint::new(*test::bob_ipv4_addr(), bob_port));
    let event = alice.poll(now).unwrap().unwrap();
    let bytes = match event {
        Event::Transmit(segment) => segment.to_vec(),
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
    assert!(segment.header().syn());

    debug!("looking for the second SYN from alice...");
    let now = now + Duration::from_secs(3);
    let event = alice.poll(now).unwrap().unwrap();
    let bytes = match event {
        Event::Transmit(segment) => segment.to_vec(),
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
    assert!(segment.header().syn());

    debug!("looking for the third SYN from alice...");
    let now = now + Duration::from_secs(6);
    let event = alice.poll(now).unwrap().unwrap();
    let bytes = match event {
        Event::Transmit(segment) => segment.to_vec(),
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
    assert!(segment.header().syn());

    debug!("looking for the forth SYN from alice...");
    let now = now + Duration::from_secs(12);
    let event = alice.poll(now).unwrap().unwrap();
    let bytes = match event {
        Event::Transmit(segment) => segment.to_vec(),
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
    assert!(segment.header().syn());

    debug!("looking for the fifth SYN from alice...");
    let now = now + Duration::from_secs(24);
    let event = alice.poll(now).unwrap().unwrap();
    let bytes = match event {
        Event::Transmit(segment) => segment.to_vec(),
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
    assert!(segment.header().syn());

    let now = now + Duration::from_secs(48);
    match fut.poll(now).unwrap() {
        Err(Fail::Timeout {}) => (),
        _ => panic!("expected timeout"),
    }
}
