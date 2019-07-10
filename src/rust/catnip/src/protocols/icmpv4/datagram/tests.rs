use super::{Icmpv4Datagram, Icmpv4DatagramMut, Icmpv4Type};
use crate::test;
use byteorder::{NetworkEndian, WriteBytesExt};

#[test]
fn checksum() {
    // ensures that a IPv4 datagram checksum works correctly.
    trace!("checksum()");
    let mut bytes = Icmpv4Datagram::new(4);
    let mut datagram = Icmpv4DatagramMut::attach(&mut bytes);
    datagram.text().write_u32::<NetworkEndian>(0x1234).unwrap();
    let mut icmpv4_header = datagram.header();
    icmpv4_header.r#type(Icmpv4Type::DestinationUnreachable);
    icmpv4_header.code(1);
    let mut ipv4_header = datagram.ipv4().header();
    ipv4_header.src_addr(*test::bob_ipv4_addr());
    ipv4_header.dest_addr(*test::alice_ipv4_addr());
    let mut frame_header = datagram.ipv4().frame().header();
    frame_header.src_addr(*test::bob_link_addr());
    frame_header.dest_addr(*test::alice_link_addr());
    let _ = datagram.seal().unwrap();
}
