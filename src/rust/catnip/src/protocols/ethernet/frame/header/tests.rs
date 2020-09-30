// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::*;
use crate::test_helpers;

#[test]
fn serialization() {
    // ensures that a UDP header serializes correctly.
    trace!("serialization()");
    let mut bytes = [0; ETHERNET2_HEADER_SIZE];
    let mut header = Ethernet2HeaderMut::new(&mut bytes);
    header.src_addr(test_helpers::ALICE_MAC);
    header.dest_addr(test_helpers::BOB_MAC);
    header.ether_type(EtherType::Arp);
    let header = Ethernet2Header::new(&bytes);
    assert_eq!(test_helpers::ALICE_MAC, header.src_addr());
    assert_eq!(test_helpers::BOB_MAC, header.dest_addr());
    assert_eq!(EtherType::Arp, header.ether_type().unwrap());
}
