use super::*;
use crate::test;
use std::io::Cursor;

#[test]
fn serialization() {
    trace!("entering `udp::tests::serialization()`");
    let header = Header {
        src_port: 12345,
        dest_port: 54321,
    };
    let mut bytes = Vec::new();
    header.write(&mut bytes, 1).unwrap();
    let header2 = Header::read(&mut Cursor::new(&bytes)).unwrap();
    assert_eq!(header, header2);
}
