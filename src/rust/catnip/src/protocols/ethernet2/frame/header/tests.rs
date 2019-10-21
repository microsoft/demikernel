// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::*;
use crate::test;

#[test]
fn serialization() {
    // ensures that a UDP header serializes correctly.
    trace!("serialization()");
    let mut bytes = [0; ETHERNET2_HEADER_SIZE];
    let mut header = Ethernet2HeaderMut::new(&mut bytes);
    header.src_addr(*test::alice_link_addr());
    header.dest_addr(*test::bob_link_addr());
    header.ether_type(EtherType::Arp);
    let header = Ethernet2Header::new(&bytes);
    assert_eq!(*test::alice_link_addr(), header.src_addr());
    assert_eq!(*test::bob_link_addr(), header.dest_addr());
    assert_eq!(EtherType::Arp, header.ether_type().unwrap());
}
