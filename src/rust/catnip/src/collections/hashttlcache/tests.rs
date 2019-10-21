// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::*;

#[test]
fn with_default_ttl() {
    // tests to ensure that an entry without an explicit TTL gets evicted at
    // the right time (using the default TTL).
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    let mut cache = HashTtlCache::new(now, Some(Duration::from_secs(1)));
    cache.insert("a", 'a');
    assert!(cache.get(&"a") == Some(&'a'));
    cache.advance_clock(later);
    let evicted = cache.try_evict(usize::max_value());
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&"a"));
    assert!(cache.get(&"a").is_none());
}

#[test]
fn without_default_ttl() {
    // tests to ensure that entries without a TTL do not get evicted.
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    let mut cache = HashTtlCache::new(now, Some(Duration::from_secs(1)));
    cache.insert_with_ttl("a", 'a', None);
    assert!(cache.get(&"a") == Some(&'a'));
    cache.advance_clock(later);
    let evicted = cache.try_evict(usize::max_value());
    assert_eq!(evicted.len(), 0);
    assert!(cache.get(&"a") == Some(&'a'));
}

#[test]
fn with_explicit_ttl() {
    // tests to ensure that an entry with an explicit TTL gets evicted at the
    // right time.
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    let mut cache = HashTtlCache::new(now, Some(Duration::from_secs(2)));
    cache.insert_with_ttl("a", 'a', Some(Duration::from_secs(1)));
    assert!(cache.get(&"a") == Some(&'a'));
    cache.advance_clock(later);
    let evicted = cache.try_evict(usize::max_value());
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&"a"));
    assert!(cache.get(&"a").is_none());
}

#[test]
fn replace_entry() {
    // tests to ensure that an entry that gets replaced doesn't get prematurely
    // evicted.
    let now = Instant::now();
    let later = now + Duration::from_secs(1);
    let even_later = now + Duration::from_secs(2);

    let mut cache = HashTtlCache::new(now, Some(Duration::from_secs(1)));
    cache.insert("a", 'a');
    assert!(cache.get(&"a") == Some(&'a'));
    cache.insert_with_ttl("a", 'a', Some(Duration::from_secs(2)));
    cache.advance_clock(later);
    let evicted = cache.try_evict(usize::max_value());
    assert_eq!(evicted.len(), 0);
    assert!(cache.get(&"a") == Some(&'a'));
    cache.advance_clock(even_later);
    let evicted = cache.try_evict(usize::max_value());
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&"a"));
    assert!(cache.get(&"a").is_none());
}

#[test]
fn limited_evictions() {
    // tests to ensure that an entry without an explicit TTL gets evicted at
    // the right time (using the default TTL).
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    let mut cache = HashTtlCache::new(now, Some(Duration::from_secs(1)));
    cache.insert_with_ttl("a", 'a', Some(Duration::from_millis(500)));
    assert!(cache.get(&"a") == Some(&'a'));
    cache.insert("b", 'b');
    assert!(cache.get(&"b") == Some(&'b'));
    cache.advance_clock(later);
    let evicted = cache.try_evict(1);
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&"a"));
    assert!(cache.get(&"a").is_none());
    let evicted = cache.try_evict(1);
    assert_eq!(evicted.len(), 1);
    assert!(evicted.contains_key(&"b"));
    assert!(cache.get(&"b").is_none());
}
