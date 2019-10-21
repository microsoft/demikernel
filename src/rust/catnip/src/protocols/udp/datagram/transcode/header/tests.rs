// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::*;
use crate::{prelude::*, protocols::ip};

#[test]
fn serialization() {
    // ensures that a UDP header serializes correctly.
    trace!("serialization()");
    let mut bytes = [0; UDP_HEADER_SIZE];
    let mut header = UdpHeaderMut::new(&mut bytes);
    let src_port = ip::Port::try_from(0x1234).unwrap();
    let dest_port = ip::Port::try_from(0x5678).unwrap();
    header.dest_port(dest_port);
    header.src_port(src_port);
    header.length(0x9abc);
    header.checksum(0xdef0);
    let header = UdpHeader::new(&bytes);
    assert_eq!(Some(dest_port), header.dest_port());
    assert_eq!(Some(src_port), header.src_port());
    assert_eq!(0x9abc, header.length());
    assert_eq!(0xdef0, header.checksum());
}
