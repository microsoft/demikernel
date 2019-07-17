use super::super::segment::TcpSegmentDecoder;
use crate::{
    prelude::*,
    protocols::{ip, ipv4, tcp},
    r#async::Async,
    test,
};
use std::time::{Duration, Instant};

#[test]
fn closed_port() {
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

    alice
        .tcp_connect(ipv4::Endpoint::new(*test::bob_ipv4_addr(), bob_port))
        .unwrap();
    let now = now + Duration::from_millis(1);

    let (tcp_syn, private_port) = {
        let effect = alice.poll(now).unwrap().unwrap();
        let bytes = match effect {
            Effect::Transmit(segment) => segment.to_vec(),
            e => panic!("got unanticipated effect `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(segment.header().syn());
        let src_port = segment.header().src_port().unwrap();
        debug!("private_port => {:?}", src_port);
        (bytes, src_port)
    };

    info!("passing TCP SYN to bob...");
    bob.receive(&tcp_syn).unwrap();
    let effect = bob.poll(now).unwrap().unwrap();
    let tcp_rst = {
        let bytes = match effect {
            Effect::Transmit(bytes) => bytes,
            e => panic!("got unanticipated effect `{:?}`", e),
        };

        let segment = TcpSegmentDecoder::attach(&bytes).unwrap();
        assert!(segment.header().rst());
        assert_eq!(Some(private_port), segment.header().dest_port());
        bytes
    };

    info!("passing TCP RST segment to alice...");
    alice.receive(&tcp_rst).unwrap();
    let effect = alice.poll(now).unwrap().unwrap();
    match effect {
        Effect::TcpError(e) => {
            assert_eq!(e, tcp::Error::ConnectionRefused {},)
        }
        e => panic!("got unanticipated effect `{:?}`", e),
    }
}
