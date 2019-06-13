use super::*;
use crate::{prelude::*, protocols::ethernet2, test};
use std::{
    io::Cursor,
    time::{Duration, Instant},
};

#[test]
fn serialization() {
    // ensures that a UDP header serializes correctly.
    trace!("*");
    let header = Ipv4Header {
        protocol: Ipv4Protocol::Udp,
        src_addr: *test::alice_ipv4_addr(),
        dest_addr: *test::bob_ipv4_addr(),
    };
    let mut bytes = Vec::new();
    header.write(&mut bytes, 1).unwrap();
    let header2 = Ipv4Header::read(&mut Cursor::new(&bytes), 1).unwrap();
    assert_eq!(header, header2);
}
