use super::*;

lazy_static! {
    static ref TTL: Duration = Duration::new(1, 0);
    static ref ALICE_MAC: MacAddress =
        MacAddress::new([0x12, 0x34, 0x56, 0xab, 0xcd, 0xef]);
    static ref BOB_MAC: MacAddress =
        MacAddress::new([0xfe, 0xdc, 0xbd, 0x65, 0x43, 0x21]);
    static ref ALICE_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
    static ref BOB_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
}

#[test]
fn black_box() {
    let now = Instant::now();
    let later = now + *TTL;
    let mut state = State::new(*TTL);
    state.insert(ALICE_IP.clone(), ALICE_MAC.clone(), now);
    assert!(state.get_link_addr(&ALICE_IP, now) == Some(&ALICE_MAC));
    assert!(state.get_link_addr(&ALICE_IP, later).is_none());
    state.flush(later);
    assert!(state.get_link_addr(&ALICE_IP, later).is_none());
}
