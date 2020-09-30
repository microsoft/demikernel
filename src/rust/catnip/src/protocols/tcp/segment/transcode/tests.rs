// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    TcpSegmentEncoder,
    TcpSegmentOptions,
    MAX_TCP_HEADER_SIZE,
};
use crate::{
    protocols::{
        ip,
        ipv4,
    },
    test_helpers,
};
use byteorder::{
    NetworkEndian,
    WriteBytesExt,
};
use std::{
    convert::TryFrom,
    num::Wrapping,
};

#[test]
fn checksum() {
    // ensures that a TCP segment checksum works correctly.
    trace!("checksum()");
    let mut bytes = ipv4::Datagram::new_vec(4 + MAX_TCP_HEADER_SIZE);
    let mut segment = TcpSegmentEncoder::attach(&mut bytes);
    segment.text().write_u32::<NetworkEndian>(0x1234).unwrap();
    let mut tcp_header = segment.header();
    tcp_header.dest_port(ip::Port::try_from(0x1234).unwrap());
    tcp_header.src_port(ip::Port::try_from(0x5678).unwrap());
    tcp_header.seq_num(Wrapping(0x9abc_def0));
    tcp_header.ack_num(Wrapping(0x1234_5678));
    let mut options = TcpSegmentOptions::new();
    options.set_mss(0x1234);
    tcp_header.options(options);
    let mut ipv4_header = segment.ipv4().header();
    ipv4_header.protocol(ipv4::Protocol::Tcp);
    ipv4_header.src_addr(test_helpers::BOB_IPV4);
    ipv4_header.dest_addr(test_helpers::ALICE_IPV4);
    let mut frame_header = segment.ipv4().frame().header();
    frame_header.src_addr(test_helpers::BOB_MAC);
    frame_header.dest_addr(test_helpers::ALICE_MAC);
    let _ = segment.seal().unwrap();
}
