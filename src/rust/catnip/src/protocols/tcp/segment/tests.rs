use super::{TcpOptions, TcpSegmentMut};
use crate::test;
use byteorder::{NetworkEndian, WriteBytesExt};

#[test]
fn checksum() {
    // ensures that a IPv4 segment checksum works correctly.
    trace!("checksum()");
    let mut bytes = TcpSegmentMut::new_bytes(4);
    let mut segment = TcpSegmentMut::from_bytes(&mut bytes);
    segment.text().write_u32::<NetworkEndian>(0x1234).unwrap();
    let mut tcp_header = segment.header();
    tcp_header.dest_port(0x1234);
    tcp_header.src_port(0x5678);
    tcp_header.seq_num(0x9abc_def0);
    tcp_header.ack_num(0x1234_5678);
    let mut options = TcpOptions::new();
    options.set_mss(0x1234);
    tcp_header.options(options);
    let mut ipv4_header = segment.ipv4().header();
    ipv4_header.src_addr(*test::bob_ipv4_addr());
    ipv4_header.dest_addr(*test::alice_ipv4_addr());
    let mut frame_header = segment.ipv4().frame().header();
    frame_header.src_addr(*test::bob_link_addr());
    frame_header.dest_addr(*test::alice_link_addr());
    let _ = segment.seal().unwrap();
}
