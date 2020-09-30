// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{Icmpv4Datagram, Icmpv4DatagramMut, Icmpv4Type};
use crate::test_helpers;
use byteorder::{NetworkEndian, WriteBytesExt};

#[test]
fn checksum() {
    // ensures that a IPv4 datagram checksum works correctly.
    trace!("checksum()");
    let mut bytes = Icmpv4Datagram::new_vec(4);
    let mut datagram = Icmpv4DatagramMut::attach(&mut bytes);
    datagram.text().write_u32::<NetworkEndian>(0x1234).unwrap();
    let mut icmpv4_header = datagram.header();
    icmpv4_header.r#type(Icmpv4Type::DestinationUnreachable);
    icmpv4_header.code(1);
    let mut ipv4_header = datagram.ipv4().header();
    ipv4_header.src_addr(test_helpers::BOB_IPV4);
    ipv4_header.dest_addr(test_helpers::ALICE_IPV4);
    let mut frame_header = datagram.ipv4().frame().header();
    frame_header.src_addr(test_helpers::BOB_MAC);
    frame_header.dest_addr(test_helpers::ALICE_MAC);
    let _ = datagram.seal().unwrap();
}
