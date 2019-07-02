use super::*;

#[test]
fn mss() {
    let mut options = TcpOptions::new();
    options.set_mss(0x1234);
    let length = options.encoded_length();
    assert_eq!(0, length % 4);
    let mut bytes = vec![0; length];
    options.encode(&mut bytes);
    debug!("bytes = {:?}", bytes);
    let options = TcpOptions::parse(bytes.as_slice()).unwrap();
    assert_eq!(0x1234, options.get_mss());
}
