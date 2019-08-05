use super::super::{
    connection::TcpConnectionHandle,
    segment::{TcpSegment, TcpSegmentDecoder},
};
use crate::{
    prelude::*,
    protocols::{ip, ipv4},
    r#async::Retry,
    test,
};
use std::{
    num::Wrapping,
    time::{Duration, Instant},
};

struct EstablishedConnection<'a> {
    alice: Engine<'a>,
    alice_cxn_handle: TcpConnectionHandle,
    bob: Engine<'a>,
    bob_cxn_handle: TcpConnectionHandle,
    now: Instant,
}

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

fn establish_connection() -> EstablishedConnection<'static> {
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
        assert_eq!(segment.header().window_size(), 0xffff);
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
        assert_eq!(segment.header().window_size(), 0xffff);
        bytes
    };

    info!("passing TCP ACK segment to bob...");
    let now = now + Duration::from_millis(1);
    bob.receive(&tcp_ack).unwrap();
    let event = bob.poll(now).unwrap().unwrap();
    let bob_cxn_handle = match event {
        Event::TcpConnectionEstablished(h) => h,
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let alice_cxn_handle = fut.poll(now).unwrap().unwrap();

    EstablishedConnection {
        alice,
        alice_cxn_handle,
        bob,
        bob_cxn_handle,
        now,
    }
}

#[test]
fn unfragmented_data_transfer() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    let data_in = IoVec::from(vec![0xab; 10]);
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_millis(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let bytes = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.as_slice(), &data_in[0]);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    debug!("passing data segment to Bob...");
    cxn.now += Duration::from_millis(1);
    cxn.bob.receive(bytes.as_slice()).unwrap();
    match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert!(data_in.structural_eq(data_out));

    cxn.now += cxn.bob.options().tcp.trailing_ack_delay();
    assert!(cxn.bob.poll(cxn.now).is_none());

    cxn.now += Duration::from_micros(1);
    let pure_ack = match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert!(segment.ack);
            assert_eq!(0, segment.payload.len());
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };
}

#[test]
fn syn_retry() {
    let bob_port = ip::Port::try_from(12345).unwrap();

    let mut now = Instant::now();
    let mut alice = test::new_alice(now);
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let options = alice.options();
    let retries = options.tcp.handshake_retries();
    let timeout = options.tcp.handshake_timeout();
    let mut retry = Retry::exponential(timeout, 2, retries);

    let fut = alice
        .tcp_connect(ipv4::Endpoint::new(*test::bob_ipv4_addr(), bob_port));
    let event = alice.poll(now).unwrap().unwrap();
    match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert!(segment.syn);
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    for i in 0..(retries - 1) {
        let timeout = retry.next().unwrap();
        now += timeout;
        assert!(fut.poll(now).is_none());
        info!("syn_retry(): retry #{}", i + 1);
        now += Duration::from_micros(1);
        let event = alice.poll(now).unwrap().unwrap();
        match event {
            Event::Transmit(bytes) => {
                let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
                assert!(segment.syn);
            }
            e => panic!("got unanticipated event `{:?}`", e),
        }
    }

    now += retry.next().unwrap();
    assert!(retry.next().is_none());
    assert!(fut.poll(now).is_none());
    now += Duration::from_micros(1);
    match fut.poll(now).unwrap() {
        Err(Fail::Timeout {}) => (),
        _ => panic!("expected timeout"),
    }
}
