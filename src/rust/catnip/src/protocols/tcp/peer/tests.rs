#![allow(clippy::cognitive_complexity)]

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
    let now = now + Duration::from_micros(1);
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
    let now = now + Duration::from_micros(1);
    alice.receive(&tcp_rst).unwrap();
    assert!(fut.poll(now).is_none());
    let now = now + Duration::from_micros(1);
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
    let (tcp_syn, private_port, alice_isn) = {
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
        let alice_isn = segment.header().seq_num();
        (bytes, src_port, alice_isn)
    };

    info!("passing TCP SYN to bob...");
    let now = now + Duration::from_micros(1);
    bob.receive(&tcp_syn).unwrap();
    let event = bob.poll(now).unwrap().unwrap();
    let (tcp_syn_ack, bob_isn) = {
        let bytes = match event {
            Event::Transmit(bytes) => bytes,
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        let bob_isn = segment.header().seq_num();
        assert!(segment.header().syn());
        assert!(segment.header().ack());
        assert_eq!(Some(private_port), segment.header().dest_port());
        assert_eq!(segment.header().ack_num(), alice_isn + Wrapping(1));
        assert_eq!(
            usize::from(segment.header().window_size()),
            alice.options().tcp.receive_window_size()
        );
        (bytes, bob_isn)
    };

    info!("passing TCP SYN+ACK segment to alice...");
    let now = now + Duration::from_micros(1);
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
        assert_eq!(segment.header().seq_num(), alice_isn + Wrapping(1));
        assert_eq!(segment.header().ack_num(), bob_isn + Wrapping(1));
        assert_eq!(
            usize::from(segment.header().window_size()),
            alice.options().tcp.receive_window_size()
        );
        bytes
    };

    info!("passing TCP ACK segment to bob...");
    let now = now + Duration::from_micros(1);
    bob.receive(&tcp_ack).unwrap();
    let event = bob.poll(now).unwrap().unwrap();
    let bob_cxn_handle = match event {
        Event::TcpConnectionEstablished(h) => h,
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let alice_cxn_handle = fut.poll(now).unwrap().unwrap();

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
    let data_in = IoVec::from(vec![0xab; 10]);
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let (bytes, seq_num) = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.as_slice(), &data_in[0]);
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes.as_slice()).unwrap();
    match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert!(data_in.structural_eq(data_out));

    info!("Bob writes data to the TCP connection...");
    cxn.bob
        .tcp_write(cxn.bob_cxn_handle, data_in.clone())
        .unwrap();
    cxn.now += Duration::from_micros(1);
    let event = cxn.bob.poll(cxn.now).unwrap().unwrap();
    let (bytes, seq_num) = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.as_slice(), &data_in[0]);
            assert!(segment.ack);
            assert_eq!(
                seq_num
                    + Wrapping(u32::try_from(data_in.byte_count()).unwrap()),
                segment.ack_num
            );
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing data segment to Alice...");
    cxn.now += Duration::from_micros(1);
    cxn.alice.receive(bytes.as_slice()).unwrap();
    match cxn.alice.poll(cxn.now).unwrap().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.alice_cxn_handle, handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    let data_out = cxn.alice.tcp_read(cxn.alice_cxn_handle).unwrap();
    assert!(data_in.structural_eq(data_out));

    info!("waiting for trailing ACK timeout to pass...");
    cxn.now += cxn.alice.options().tcp.trailing_ack_delay();
    assert!(cxn.alice.poll(cxn.now).is_none());

    cxn.now += Duration::from_micros(1);
    let pure_ack = match cxn.alice.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.payload.len());
            assert!(segment.ack);
            assert_eq!(
                seq_num
                    + Wrapping(u32::try_from(data_in.byte_count()).unwrap()),
                segment.ack_num
            );
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing pure ACK segment to Bob...");
    cxn.bob.receive(pure_ack.as_slice()).unwrap();
}

#[test]
fn packetization() {
    let mut cxn = establish_connection();

    // transmitting 2k bytes of data should produce an equivalent `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in =
        IoVec::from(vec![
            0xab;
            cxn.alice.tcp_mss(cxn.alice_cxn_handle).unwrap() + 1
        ]);
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let (bytes0, seq_num) = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let bytes1 = match event {
        Event::Transmit(bytes) => bytes,
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing data segments to Bob...");
    cxn.now += Duration::from_micros(1);
    // ACK timeout starts from here.
    cxn.bob.receive(bytes0.as_slice()).unwrap();
    match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes1.as_slice()).unwrap();
    // Event::TcpBytesAvailable won't be emitted unless the read buffer starts
    // out empty.
    assert!(cxn.bob.poll(cxn.now).is_none());

    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, data_out);

    info!("waiting for trailing ACK timeout to pass...");
    cxn.now +=
        cxn.bob.options().tcp.trailing_ack_delay() - Duration::from_micros(1);
    assert!(cxn.bob.poll(cxn.now).is_none());

    cxn.now += Duration::from_micros(1);
    let pure_ack = match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.payload.len());
            assert!(segment.ack);
            assert_eq!(
                seq_num
                    + Wrapping(u32::try_from(data_in.byte_count()).unwrap()),
                segment.ack_num
            );
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing pure ACK segment to Alice...");
    cxn.alice.receive(pure_ack.as_slice()).unwrap();
}

#[test]
fn multiple_writes() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in = IoVec::from(vec![0xab; 10]);
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let (bytes, seq_num) = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.as_slice(), &data_in[0]);
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    // ACK timeout starts from here.
    cxn.bob.receive(bytes.as_slice()).unwrap();
    match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    info!("Alice writes more data to the TCP connection...");
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let bytes = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.as_slice(), &data_in[0]);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing second data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes.as_slice()).unwrap();
    // Event::TcpBytesAvailable won't be emitted unless the read buffer starts
    // out empty.
    assert!(cxn.bob.poll(cxn.now).is_none());

    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(
        data_out,
        IoVec::from(vec![data_in[0].to_vec(), data_in[0].to_vec()])
    );

    info!("waiting for trailing ACK timeout to pass...");
    cxn.now +=
        cxn.bob.options().tcp.trailing_ack_delay() - Duration::from_micros(2);
    assert!(cxn.bob.poll(cxn.now).is_none());

    cxn.now += Duration::from_micros(1);
    let pure_ack = match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.payload.len());
            assert!(segment.ack);
            assert_eq!(
                seq_num
                    + Wrapping(
                        u32::try_from(data_in.byte_count()).unwrap() * 2
                    ),
                segment.ack_num
            );
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing pure ACK segment to Alice...");
    cxn.alice.receive(pure_ack.as_slice()).unwrap();
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
    let mut retry = Retry::binary_exponential(timeout, retries);

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

#[test]
fn retransmission_fail() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in = IoVec::from(vec![0xab; 10]);
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let dropped_segment = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.as_slice(), &data_in[0]);
            segment
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    let rto = cxn.alice.tcp_rto(cxn.alice_cxn_handle).unwrap();
    let retries = cxn.alice.options().tcp.retries2();
    let mut retry = Retry::binary_exponential(rto, retries);
    for i in 0..(retries - 1) {
        let timeout = retry.next().unwrap();
        cxn.now += timeout;
        assert!(cxn.alice.poll(cxn.now).is_none());
        info!("retransmission(): retry #{}", i + 1);
        cxn.now += Duration::from_micros(1);
        let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
        match event {
            Event::Transmit(bytes) => {
                assert_eq!(
                    dropped_segment,
                    TcpSegment::decode(bytes.as_slice()).unwrap()
                );
            }
            e => panic!("got unanticipated event `{:?}`", e),
        }
    }

    cxn.now += retry.next().unwrap();
    assert!(retry.next().is_none());
    assert!(cxn.alice.poll(cxn.now).is_none());
    cxn.now += Duration::from_micros(1);
    match cxn.alice.poll(cxn.now).unwrap().unwrap() {
        Event::TcpConnectionClosed { handle, error } => {
            assert_eq!(handle, cxn.alice_cxn_handle);
            match error {
                Some(Fail::Timeout {}) => (),
                _ => panic!("expected a timeout error"),
            }
        }
        _ => panic!("unexpected event"),
    }
}

#[test]
fn retransmission_recovery() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("Alice writes data to the TCP connection...");
    let data_in = IoVec::from(vec![0xab; 10]);
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let dropped_segment = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.as_slice(), &data_in[0]);
            segment
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("dropping data segment and attempting retry...");
    let rto = cxn.alice.tcp_rto(cxn.alice_cxn_handle).unwrap();
    let retries = cxn.alice.options().tcp.retries2();
    let mut retry = Retry::binary_exponential(rto, retries);
    let timeout = retry.next().unwrap();
    cxn.now += timeout;
    assert!(cxn.alice.poll(cxn.now).is_none());
    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let (bytes, seq_num) = {
        match event {
            Event::Transmit(bytes) => {
                let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
                assert_eq!(dropped_segment, segment);

                (bytes, segment.seq_num)
            }
            e => panic!("got unanticipated event `{:?}`", e),
        }
    };

    info!("passing data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    // ACK timeout starts from here.
    cxn.bob.receive(bytes.as_slice()).unwrap();
    match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    info!("Alice writes more data to the TCP connection...");
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    let event = cxn.alice.poll(cxn.now).unwrap().unwrap();
    let bytes = match event {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.as_slice(), &data_in[0]);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing second data segment to Bob...");
    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes.as_slice()).unwrap();
    // Event::TcpBytesAvailable won't be emitted unless the read buffer starts
    // out empty.
    assert!(cxn.bob.poll(cxn.now).is_none());

    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(
        data_out,
        IoVec::from(vec![data_in[0].to_vec(), data_in[0].to_vec()])
    );

    info!("waiting for trailing ACK timeout to pass...");
    cxn.now +=
        cxn.bob.options().tcp.trailing_ack_delay() - Duration::from_micros(2);
    assert!(cxn.bob.poll(cxn.now).is_none());

    cxn.now += Duration::from_micros(1);
    let pure_ack = match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.payload.len());
            assert!(segment.ack);
            assert_eq!(
                seq_num
                    + Wrapping(
                        u32::try_from(data_in.byte_count()).unwrap() * 2
                    ),
                segment.ack_num
            );
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("passing pure ACK segment to Alice...");
    cxn.alice.receive(pure_ack.as_slice()).unwrap();
}

#[test]
fn flow_control() {
    let mut cxn = establish_connection();

    // transmitting 10 bytes of data should produce an identical `IoVec` upon
    // reading.
    info!("flow_control(): Alice writes data to the TCP connection...");
    let data_in =
        IoVec::from(vec![0xab; cxn.bob.options().tcp.receive_window_size()]);
    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();

    cxn.now += Duration::from_micros(1);
    let (bytes0, seq_num) = match cxn.alice.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            (bytes, segment.seq_num)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.now += Duration::from_micros(1);
    let bytes1 = match cxn.alice.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
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
    match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes1.as_slice()).unwrap();
    // Event::TcpBytesAvailable won't be emitted unless the read buffer starts
    // out empty.
    let zero_window_advertisement =
        match cxn.bob.poll(cxn.now).unwrap().unwrap() {
            Event::Transmit(bytes) => {
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
    assert!(cxn.alice.poll(cxn.now).is_none());

    cxn.alice
        .tcp_write(cxn.alice_cxn_handle, data_in.clone())
        .unwrap();
    assert!(cxn.alice.poll(cxn.now).is_none());

    info!(
        "flow_control(): waiting for Alice to start sending window probes..."
    );
    let rto = cxn.alice.tcp_rto(cxn.alice_cxn_handle).unwrap();
    let retries = cxn.alice.options().tcp.retries2();
    let mut retry = Retry::binary_exponential(rto, retries);
    for i in 0..(retries - 1) {
        let timeout = retry.next().unwrap();
        cxn.now += timeout;
        assert!(cxn.alice.poll(cxn.now).is_none());
        info!("flow_control(): try #{}", i + 1);
        cxn.now += Duration::from_micros(1);
        let window_probe = match cxn.alice.poll(cxn.now).unwrap().unwrap() {
            Event::Transmit(bytes) => {
                let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
                assert_eq!(segment.payload.len(), 1);
                bytes
            }
            e => panic!("got unanticipated event `{:?}`", e),
        };

        debug!("flow_control(): passing window probe to Bob...");
        cxn.bob.receive(window_probe.as_slice()).unwrap();
        let zero_window_advertisement =
            match cxn.bob.poll(cxn.now).unwrap().unwrap() {
                Event::Transmit(bytes) => {
                    let segment =
                        TcpSegment::decode(bytes.as_slice()).unwrap();
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
        assert!(cxn.alice.poll(cxn.now).is_none());

        // this is acceptable, since we increment the time by `timeout` when we
        // start a new loop iteration.
        cxn.now -= Duration::from_micros(1);
    }

    info!("flow_control(): reading available bytes from Bob's TCP window...");
    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, data_out);

    cxn.now += Duration::from_micros(1);
    let window_advertisement = match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_ne!(0, segment.window_size);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("flow_control(): passing window advertisement to Alice...");
    cxn.now += Duration::from_micros(1);
    cxn.alice.receive(window_advertisement.as_slice()).unwrap();
    let bytes0 = match cxn.alice.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(segment.payload.len(), 1);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.now += Duration::from_micros(1);
    let bytes1 = match cxn.alice.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_ne!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    cxn.now += Duration::from_micros(1);
    let bytes2 = match cxn.alice.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_ne!(segment.payload.len(), 0);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("flow_control(): passing data segments to Bob...");
    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes0.as_slice()).unwrap();
    match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::TcpBytesAvailable(handle) => {
            assert_eq!(cxn.bob_cxn_handle, handle)
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes1.as_slice()).unwrap();
    assert!(cxn.bob.poll(cxn.now).is_none());

    cxn.now += Duration::from_micros(1);
    cxn.bob.receive(bytes2.as_slice()).unwrap();
    let _ = match cxn.bob.poll(cxn.now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let segment = TcpSegment::decode(bytes.as_slice()).unwrap();
            assert_eq!(0, segment.window_size);
            bytes
        }
        e => panic!("got unanticipated event `{:?}`", e),
    };

    info!("flow_control(): reading available bytes from Bob's TCP window...");
    let data_out = cxn.bob.tcp_read(cxn.bob_cxn_handle).unwrap();
    assert_eq!(data_in, data_out);
}
