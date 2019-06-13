use super::*;
use crate::test;
use std::io::Cursor;

#[test]
fn serialization() {
    trace!("entering `ethernet2::tests::serialization()`");
    let header = Header {
        dest_addr: *test::alice_link_addr(),
        src_addr: *test::bob_link_addr(),
        ether_type: EtherType::Arp,
    };
    let mut bytes = Vec::new();
    header.write(&mut bytes).unwrap();
    let header2 = Header::read(&mut Cursor::new(&bytes)).unwrap();
    assert_eq!(header, header2);
}
