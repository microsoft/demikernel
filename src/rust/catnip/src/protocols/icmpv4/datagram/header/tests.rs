// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::*;

#[test]
fn serialization() {
    // ensures that a ICMPv4 header serializes correctly.
    trace!("serialization()");
    let mut bytes = [0; ICMPV4_HEADER_SIZE];
    let mut header = Icmpv4HeaderMut::new(&mut bytes);
    header.r#type(Icmpv4Type::EchoReply);
    header.code(0xab);
    header.checksum(0x1234);
    let header = Icmpv4Header::new(&bytes);
    assert_eq!(Icmpv4Type::EchoReply, header.r#type().unwrap());
    assert_eq!(0xab, header.code());
    assert_eq!(0x1234, header.checksum());
}
