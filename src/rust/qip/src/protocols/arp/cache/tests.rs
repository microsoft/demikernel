use super::*;

lazy_static! {
    static ref TTL: Duration = Duration::new(1, 0);
    static ref ALICE_MAC: MacAddress =
        MacAddress::new([0x12, 0x34, 0x56, 0xab, 0xcd, 0xef]);
    static ref ALICE_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
}

#[test]
fn black_box() {
    let now = Instant::now();
    let later = now + *TTL;
    let mut state = ArpCache::new(*TTL, now);
    state.insert(ALICE_IP.clone(), ALICE_MAC.clone());
    assert!(state.get_link_addr(&ALICE_IP) == Some(&ALICE_MAC));
    assert!(state.get_ipv4_addr(&ALICE_MAC) == Some(&ALICE_IP));
    state.touch(later);
    assert!(state.get_link_addr(&ALICE_IP).is_none());
}
