// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{Ipv4Datagram, Ipv4DatagramMut, Ipv4Protocol};
use crate::test;

#[test]
fn checksum() {
    // ensures that a IPv4 datagram checksum works correctly.
    trace!("checksum()");
    // the IPv4 checksum does not include text.
    let mut bytes = Ipv4Datagram::new_vec(0);
    let mut datagram = Ipv4DatagramMut::attach(&mut bytes);
    let mut ipv4_header = datagram.header();
    ipv4_header.src_addr(*test::bob_ipv4_addr());
    ipv4_header.dest_addr(*test::alice_ipv4_addr());
    ipv4_header.protocol(Ipv4Protocol::Icmpv4);
    let mut frame_header = datagram.frame().header();
    frame_header.src_addr(*test::bob_link_addr());
    frame_header.dest_addr(*test::alice_link_addr());
    let _ = datagram.seal().unwrap();
}
