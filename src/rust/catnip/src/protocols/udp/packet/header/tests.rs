use super::*;

#[test]
fn serialization() {
    // ensures that a UDP header serializes correctly.
    trace!("serialization()");
    let mut bytes = [0; UDP_HEADER_SIZE];
    let mut header = UdpHeaderMut::new(&mut bytes);
    header.dest_port(0x1234);
    header.src_port(0x5678);
    header.length(0x9abc);
    header.checksum(0xdef0);
    let header = UdpHeader::new(&bytes);
    assert_eq!(0x1234, header.dest_port());
    assert_eq!(0x5678, header.src_port());
    assert_eq!(0x9abc, header.length());
    assert_eq!(0xdef0, header.checksum());
}
