use super::*;

#[test]
fn mss() {
    let mut options = TcpSegmentOptions::new();
    options.set_mss(0x1234);
    let length = options.encoded_length();
    assert_eq!(0, length % 4);
    let mut bytes = vec![0; length];
    options.encode(&mut bytes);
    debug!("bytes = {:?}", bytes);
    let options = TcpSegmentOptions::parse(bytes.as_slice()).unwrap();
    assert_eq!(Some(0x1234), options.get_mss());
}
