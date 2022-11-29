// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::HashTtlCache;
use std::time::{
    Duration,
    Instant,
};

/// Tests that objects with an explicit TTL get evicted at the right time.
#[test]
fn evict_by_explicit_ttl_case1() {
    let now = Instant::now();
    let ttl = Duration::from_secs(1);
    let later = now + ttl;
    let mut cache = HashTtlCache::new(now, None);

    // Insert an object in the cache with a TTL.
    cache.insert_with_ttl("a", 'a', Some(ttl));
    assert!(cache.get(&"a") == Some(&'a'));

    // Advance clock and make sure that the object is not in the cache.
    cache.advance_clock(later);
    cache.cleanup();
    assert!(cache.get(&"a").is_none());
}

/// Tests that objects with an explicit TTL get evicted at the right time.
#[test]
fn evict_by_explicit_ttl_case2() {
    let now = Instant::now();
    let ttl = Duration::from_secs(1);
    let later = now + ttl;
    let mut cache = HashTtlCache::new(now, Some(ttl + ttl));

    // Insert an object in the cache with a TTL.
    cache.insert_with_ttl("a", 'a', Some(ttl));
    assert!(cache.get(&"a") == Some(&'a'));

    // Advance clock and make sure that the object is not in the cache.
    cache.advance_clock(later);
    cache.cleanup();
    assert!(cache.get(&"a").is_none());
}

/// Tests that objects without an explicit TTL get evicted using the default TTL.
#[test]
fn evict_by_default_ttl() {
    let now = Instant::now();
    let ttl = Duration::from_secs(1);
    let later = now + ttl;
    let mut cache = HashTtlCache::new(now, Some(ttl));

    // Insert an object in the cache with the default TTL.
    cache.insert("a", 'a');
    assert!(cache.get(&"a") == Some(&'a'));

    // Advance clock and make sure that the object is not in the cache.
    cache.advance_clock(later);
    cache.cleanup();
    assert!(cache.get(&"a").is_none());
}

/// Tests that objects without a TTL do not get evicted.
#[test]
fn no_evict_excplicit() {
    let now = Instant::now();
    let ttl = Duration::from_secs(1);
    let later = now + ttl;
    let mut cache = HashTtlCache::new(now, Some(ttl));

    // Insert an object in the cache without a TTL.
    cache.insert_with_ttl("a", 'a', None);
    assert!(cache.get(&"a") == Some(&'a'));

    // Advance clock and make sure that the object is in the cache.
    cache.advance_clock(later);
    cache.cleanup();
    assert!(cache.get(&"a") == Some(&'a'));
}

/// Tests that objects with none as default TTL do not get evicted.
#[test]
fn no_evict_default() {
    let now = Instant::now();
    let ttl = Duration::from_secs(1);
    let later = now + ttl;
    let mut cache = HashTtlCache::new(now, None);

    // Insert an object in the cache with the default TTL.
    cache.insert("a", 'a');
    assert!(cache.get(&"a") == Some(&'a'));

    // Advance clock and make sure that the object is in the cache.
    cache.advance_clock(later);
    cache.cleanup();
    assert!(cache.get(&"a") == Some(&'a'));
}

// Tests if objects that are replaced do not get prematurely evicted.
#[test]
fn replace_object() {
    let now = Instant::now();
    let ttl = Duration::from_secs(2);
    let default_ttl = Duration::from_secs(1);
    let later = now + default_ttl;
    let even_later = now + ttl;
    let mut cache = HashTtlCache::new(now, Some(default_ttl));

    // Insert an object in the cache with the default TTL.
    cache.insert("a", 'a');
    assert!(cache.get(&"a") == Some(&'a'));

    // Replace object using an explicit TTL.
    let replaced = cache.insert_with_ttl("a", 'b', Some(ttl));
    assert!(replaced == Some('a'));
    assert!(cache.get(&"a") == Some(&'b'));

    // Make sure that the object was replaced and is in the cache.
    cache.advance_clock(later);
    cache.cleanup();
    assert!(cache.get(&"a") == Some(&'b'));

    // Advance clock and make sure that the object is not in the cache.
    cache.advance_clock(even_later);
    cache.cleanup();
    assert!(cache.get(&"a").is_none());
}

#[test]
fn add_and_remove_object() {
    let now: Instant = Instant::now();
    let ttl: Duration = Duration::from_secs(2);
    let default_ttl: Duration = Duration::from_secs(1);
    let mut cache: HashTtlCache<&str, char> = HashTtlCache::<&str, char>::new(now, Some(default_ttl));
    // insert object with default TTL
    cache.insert("a", 'a');
    // insert object with some TTL
    cache.insert_with_ttl("b", 'b', Some(ttl));

    // make sure object is in the cache
    assert!(cache.get(&"a") == Some(&'a'));
    assert!(cache.get(&"b") == Some(&'b'));

    // remove object
    cache.remove(&"a");
    cache.remove(&"b");

    // make sure object is not in cache
    assert!(cache.get(&"a").is_none());
    assert!(cache.get(&"b").is_none());
}
