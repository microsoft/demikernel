use super::*;
use crate::test;

#[test]
fn serialization() {
    // ensures that a IPv4 header serializes correctly.
    trace!("serialization()");
    let mut bytes = [0; IPV4_HEADER_SIZE];
    let mut header = Ipv4HeaderMut::new(&mut bytes);
    header.version(IPV4_VERSION);
    header.ihl(IPV4_IHL_NO_OPTIONS);
    header.dscp(0x12);
    header.ecn(0x3);
    header.total_len(0x4567);
    header.id(0x89ab);
    header.flags(0x6);
    header.frag_offset(0x1cde);
    header.ttl(DEFAULT_IPV4_TTL);
    header.protocol(Ipv4Protocol::Udp);
    header.src_addr(*test::alice_ipv4_addr());
    header.dest_addr(*test::bob_ipv4_addr());
    let header = Ipv4Header::new(&bytes);
    assert_eq!(IPV4_VERSION, header.version());
    assert_eq!(IPV4_IHL_NO_OPTIONS, header.ihl());
    assert_eq!(0x12, header.dscp());
    assert_eq!(0x3, header.ecn());
    assert_eq!(0x4567, header.total_len());
    assert_eq!(0x89ab, header.id());
    assert_eq!(0x6, header.flags());
    assert_eq!(0x1cde, header.frag_offset());
    assert_eq!(DEFAULT_IPV4_TTL, header.ttl());
    assert_eq!(Ipv4Protocol::Udp, header.protocol().unwrap());
    assert_eq!(*test::alice_ipv4_addr(), header.src_addr());
    assert_eq!(*test::bob_ipv4_addr(), header.dest_addr());
}
