use super::*;
use crate::protocols::ip;

#[test]
fn no_options() {
    trace!("no_options()");
    let mut bytes = [0u8; MAX_TCP_HEADER_SIZE];
    let mut header = TcpHeaderEncoder::attach(bytes.as_mut());
    let dest_port = ip::Port::try_from(0x1234).unwrap();
    let src_port = ip::Port::try_from(0x5678).unwrap();
    header.dest_port(dest_port);
    header.src_port(src_port);
    header.seq_num(Wrapping(0x9abc_def0));
    header.ack_num(Wrapping(0x1234_5678));
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
    let no_options = TcpSegmentOptions::new();
    header.options(no_options.clone());
    let header = TcpHeaderDecoder::attach(&bytes).unwrap();
    assert_eq!(Some(dest_port), header.dest_port());
    assert_eq!(Some(src_port), header.src_port());
    assert_eq!(0x9abc_def0, header.seq_num().0);
    assert_eq!(0x1234_5678, header.ack_num().0);
    assert_eq!(no_options.header_length(), header.header_len());
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

#[test]
fn mss() {
    trace!("mss()");
    let mut bytes = [0; MAX_TCP_HEADER_SIZE];
    let mut header = TcpHeaderEncoder::attach(&mut bytes);
    let mut options = TcpSegmentOptions::new();
    options.set_mss(0x1234);
    header.options(options);
    let header = TcpHeaderDecoder::attach(&bytes).unwrap();
    let options = header.options();
    assert_eq!(Some(0x1234), options.get_mss());
}
