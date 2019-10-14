use super::*;

lazy_static! {
    static ref TTL: Duration = Duration::new(1, 0);
    static ref ALICE_MAC: MacAddress =
        MacAddress::new([0x12, 0x34, 0x56, 0xab, 0xcd, 0xef]);
    static ref ALICE_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
}

#[test]
fn with_default_ttl() {
    // tests to ensure that an entry without an explicit TTL gets evicted at
    // the right time (using the default TTL).
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    let mut cache = ArpCache::new(now, Some(Duration::from_secs(1)));
    cache.insert(ALICE_IP.clone(), ALICE_MAC.clone());
    assert!(cache.get_link_addr(ALICE_IP.clone()) == Some(&ALICE_MAC));
    assert!(cache.get_ipv4_addr(ALICE_MAC.clone()) == Some(&ALICE_IP));
    cache.advance_clock(later);
    let evicted = cache.try_evict(usize::max_value());
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&ALICE_IP));
    assert!(cache.get_link_addr(ALICE_IP.clone()).is_none());
}
