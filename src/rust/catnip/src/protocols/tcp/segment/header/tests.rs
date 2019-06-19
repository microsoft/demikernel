use super::*;

#[test]
fn serialization() {
    // ensures that a TCP header serializes correctly.

    trace!("serialization()");
    let mut bytes = [0; TCP_HEADER_SIZE];
    let mut header = TcpHeaderMut::new(&mut bytes);
    header.dest_port(0x1234);
    header.src_port(0x5678);
    header.seq_num(0x9abc_def0);
    header.ack_num(0x1234_5678);
    assert!(header.header_len(0x9).is_err());
    header.header_len(0x20).unwrap();
    header.ns(true);
    header.cwr(true);
    header.ece(true);
    header.urg(true);
    header.ack(true);
    header.psh(true);
    header.rst(true);
    header.syn(true);
    header.fin(true);
    header.window_sz(0xbcde);
    header.checksum(0xf0ed);
    header.urg_ptr(0xcba9);
    let header = TcpHeader::new(&bytes);
    assert_eq!(0x1234, header.dest_port());
    assert_eq!(0x5678, header.src_port());
    assert_eq!(0x9abc_def0, header.seq_num());
    assert_eq!(0x1234_5678, header.ack_num());
    assert_eq!(0x20, header.header_len());
    assert!(header.ns());
    assert!(header.cwr());
    assert!(header.ece());
    assert!(header.urg());
    assert!(header.ack());
    assert!(header.psh());
    assert!(header.rst());
    assert!(header.syn());
    assert!(header.fin());
    assert_eq!(0xbcde, header.window_sz());
    assert_eq!(0xf0ed, header.checksum());
    assert_eq!(0xcba9, header.urg_ptr());
}
