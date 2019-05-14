use super::*;

#[test]
fn with_default_ttl() {
    // tests to ensure that an entry without an explicit TTL gets evicted at the right time (using the default TTL).
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    let mut cache = HashTtlCache::new(Some(Duration::from_secs(1)), now);
    cache.insert("a", 'a');
    assert!(cache.get(&"a") == Some(&'a'));
    let evicted = cache.try_evict(later);
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&"a"));
    assert!(cache.get(&"a").is_none());
}

#[test]
fn without_default_ttl() {
    // tests to ensure that entries without a TTL do not get evicted.
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    let mut cache = HashTtlCache::new(Some(Duration::from_secs(1)), now);
    cache.insert_with_ttl("a", 'a', None);
    assert!(cache.get(&"a") == Some(&'a'));
    let evicted = cache.try_evict(later);
    assert_eq!(evicted.len(), 0);
    assert!(cache.get(&"a") == Some(&'a'));
}

#[test]
fn with_explicit_ttl() {
    // tests to ensure that an entry with an explicit TTL gets evicted at the right time.
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    let mut cache = HashTtlCache::new(Some(Duration::from_secs(2)), now);
    cache.insert_with_ttl("a", 'a', Some(Duration::from_secs(1)));
    assert!(cache.get(&"a") == Some(&'a'));
    let evicted = cache.try_evict(later);
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&"a"));
    assert!(cache.get(&"a").is_none());
}

#[test]
fn replace_entry() {
    // tests to ensure that an entry that gets replaced doesn't get prematurely evicted.
    let now = Instant::now();
    let later = now + Duration::from_secs(1);
    let even_later = now + Duration::from_secs(2);

    let mut cache = HashTtlCache::new(Some(Duration::from_secs(1)), now);
    cache.insert("a", 'a');
    assert!(cache.get(&"a") == Some(&'a'));
    cache.insert_with_ttl("a", 'a', Some(Duration::from_secs(2)));
    let evicted = cache.try_evict(later);
    assert_eq!(evicted.len(), 0);
    assert!(cache.get(&"a") == Some(&'a'));
    let evicted = cache.try_evict(even_later);
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&"a"));
    assert!(cache.get(&"a").is_none());
}
