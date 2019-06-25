use super::*;

#[test]
fn serialization() {
    // ensures that a ICMPv4 Echo datagram serializes correctly.
    trace!("serialization()");
    let mut bytes = Icmpv4EchoMut::new_bytes();
    let mut datagram = Icmpv4EchoMut::from_bytes(&mut bytes).unwrap();
    datagram.r#type(Icmpv4EchoType::Reply);
    datagram.id(0xab);
    datagram.seq_num(0xcd);
    let datagram = datagram.seal().unwrap();
    assert_eq!(Icmpv4EchoType::Reply, datagram.r#type());
    assert_eq!(0xab, datagram.id());
    assert_eq!(0xcd, datagram.seq_num());
}
