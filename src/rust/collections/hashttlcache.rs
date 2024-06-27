// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::{
    collections::{
        hash_map::Entry as HashMapEntry,
        HashMap,
    },
    hash::Hash,
    time::{
        Duration,
        Instant,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// # TTL Cache Entry
///
/// Values may be assigned to an expiration time or not.
struct Record<V> {
    /// Stored Value
    value: V,
    /// Expiration Time
    expiration: Option<Instant>,
}

impl<V> Record<V> {
    /// Asserts if the target cache entry has expired.
    fn has_expired(&self, now: Instant) -> bool {
        if let Some(e) = self.expiration {
            return now >= e;
        }
        false
    }
}

/// # TTL Cache
///
/// Entries in this structure fall in one of the following kinds: those that
/// have an expiration time, and those that don't. The latter are assigned to
/// `None` expiration.
pub struct HashTtlCache<K, V> {
    /// Living values.
    map: HashMap<K, Record<V>>,
    /// Dead values.
    graveyard: HashMap<K, V>,
    /// Default expiration.
    default_ttl: Option<Duration>,
    /// Current time.
    clock: Instant,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<K, V> HashTtlCache<K, V>
where
    K: Eq + Hash + Clone,
{
    /// Instantiates an TTL cache.
    pub fn new(now: Instant, default_ttl: Option<Duration>) -> HashTtlCache<K, V> {
        if let Some(ttl) = default_ttl {
            assert!(ttl > Duration::new(0, 0));
        };

        HashTtlCache {
            map: HashMap::default(),
            graveyard: HashMap::default(),
            default_ttl,
            clock: now,
        }
    }

    // Cleanups the cache.
    pub fn clear(&mut self) {
        self.graveyard.clear();
        self.map.clear();
    }

    #[cfg(test)]
    // Advances the internal clock of the cache.
    pub fn advance_clock(&mut self, now: Instant) {
        assert!(now >= self.clock);
        self.clock = now;
    }

    /// Inserts an entry in the cache. If there is an entry in the cache with
    /// the same key, the value of that entry is updated and the old one is
    /// returned.
    pub fn insert_with_ttl(&mut self, key: K, value: V, ttl: Option<Duration>) -> Option<V> {
        let expiration = ttl.map(|ttl| {
            assert!(ttl > Duration::new(0, 0));
            self.clock + ttl
        });

        self.cleanup();

        let r = Record { value, expiration };
        match self.map.entry(key) {
            HashMapEntry::Occupied(mut o) => {
                let old_record = o.insert(r);
                Some(old_record.value)
            },
            HashMapEntry::Vacant(e) => {
                e.insert(r);
                None
            },
        }
    }

    /// Inserts an entry in the cache using the default TTL value. If there is
    /// an entry in the cache with the same key, the value of that entry is
    /// updated and the old one is returned.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.insert_with_ttl(key, value, self.default_ttl)
    }

    #[cfg(test)]
    /// Removes an entry from the cache.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        match self.get(key) {
            Some(_) => {
                let (k, v): (K, Record<V>) = self.map.remove_entry(key).unwrap();
                // FIXME: should be returning the removed memory not the old one from
                // the graveyard, but we can't without cloning the removed value
                self.graveyard.insert(k, v.value)
            },
            None => {
                warn!("Trying to remove key that does not exist");
                None
            },
        }
    }

    // Gets an entry from the cache.
    pub fn get(&self, key: &K) -> Option<&V> {
        return self.map.get(key).map(|r| &r.value);
    }

    #[cfg(test)]
    // Iterator.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        let clock = self.clock;
        self.map.iter().flat_map(move |(key, record)| {
            if record.has_expired(clock) {
                None
            } else {
                Some((key, &record.value))
            }
        })
    }

    /// Collect dead entries in the cache.
    pub fn cleanup(&mut self) {
        let mut dead_entries: Vec<K> = Vec::new();

        // Collect dead entries.
        for (k, v) in self.map.iter() {
            if v.has_expired(self.clock) {
                dead_entries.push(k.clone());
            }
        }

        // Put dead_entries entries in the graveyard.
        for k in dead_entries {
            let (k, v) = self.map.remove_entry(&k).unwrap();
            self.graveyard.insert(k, v.value);
        }
    }
}

//======================================================================================================================
// Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use crate::collections::hashttlcache::HashTtlCache;
    use ::anyhow::Result;
    use ::std::time::{
        Duration,
        Instant,
    };

    /// Tests that objects with an explicit TTL get evicted at the right time.
    #[test]
    fn evict_by_explicit_ttl_case1() -> Result<()> {
        let now = Instant::now();
        let now = now;
        let ttl = Duration::from_secs(1);
        let later = now + ttl;
        let mut cache = HashTtlCache::new(now, None);

        // Insert an object in the cache with a TTL.
        cache.insert_with_ttl("a", 'a', Some(ttl));
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));

        // Advance clock and make sure that the object is not in the cache.
        cache.advance_clock(later);
        cache.cleanup();
        crate::ensure_eq!(cache.get(&"a"), None);

        Ok(())
    }

    /// Tests that objects with an explicit TTL get evicted at the right time.
    #[test]
    fn evict_by_explicit_ttl_case2() -> Result<()> {
        let now = Instant::now();
        let ttl = Duration::from_secs(1);
        let later = now + ttl;
        let mut cache = HashTtlCache::new(now, Some(ttl + ttl));

        // Insert an object in the cache with a TTL.
        cache.insert_with_ttl("a", 'a', Some(ttl));
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));

        // Advance clock and make sure that the object is not in the cache.
        cache.advance_clock(later);
        cache.cleanup();
        crate::ensure_eq!(cache.get(&"a"), None);

        Ok(())
    }

    /// Tests that objects without an explicit TTL get evicted using the default TTL.
    #[test]
    fn evict_by_default_ttl() -> Result<()> {
        let now = Instant::now();
        let ttl = Duration::from_secs(1);
        let later = now + ttl;
        let mut cache = HashTtlCache::new(now, Some(ttl));

        // Insert an object in the cache with the default TTL.
        cache.insert("a", 'a');
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));

        // Advance clock and make sure that the object is not in the cache.
        cache.advance_clock(later);
        cache.cleanup();
        crate::ensure_eq!(cache.get(&"a"), None);

        Ok(())
    }

    /// Tests that objects without a TTL do not get evicted.
    #[test]
    fn no_evict_excplicit() -> Result<()> {
        let now = Instant::now();
        let ttl = Duration::from_secs(1);
        let later = now + ttl;
        let mut cache = HashTtlCache::new(now, Some(ttl));

        // Insert an object in the cache without a TTL.
        cache.insert_with_ttl("a", 'a', None);
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));

        // Advance clock and make sure that the object is in the cache.
        cache.advance_clock(later);
        cache.cleanup();
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));

        Ok(())
    }

    /// Tests that objects with none as default TTL do not get evicted.
    #[test]
    fn no_evict_default() -> Result<()> {
        let now = Instant::now();
        let ttl = Duration::from_secs(1);
        let later = now + ttl;
        let mut cache = HashTtlCache::new(now, None);

        // Insert an object in the cache with the default TTL.
        cache.insert("a", 'a');
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));

        // Advance clock and make sure that the object is in the cache.
        cache.advance_clock(later);
        cache.cleanup();
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));

        Ok(())
    }

    // Tests if objects that are replaced do not get prematurely evicted.
    #[test]
    fn replace_object() -> Result<()> {
        let now = Instant::now();
        let ttl = Duration::from_secs(2);
        let default_ttl = Duration::from_secs(1);
        let later = now + default_ttl;
        let even_later = now + ttl;
        let mut cache = HashTtlCache::new(now, Some(default_ttl));

        // Insert an object in the cache with the default TTL.
        cache.insert("a", 'a');
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));

        // Replace object using an explicit TTL.
        let replaced = cache.insert_with_ttl("a", 'b', Some(ttl));
        crate::ensure_eq!(replaced, Some('a'));
        crate::ensure_eq!(cache.get(&"a"), Some(&'b'));

        // Make sure that the object was replaced and is in the cache.
        cache.advance_clock(later);
        cache.cleanup();
        crate::ensure_eq!(cache.get(&"a"), Some(&'b'));

        // Advance clock and make sure that the object is not in the cache.
        cache.advance_clock(even_later);
        cache.cleanup();
        crate::ensure_eq!(cache.get(&"a"), None);

        Ok(())
    }

    #[test]
    fn add_and_remove_object() -> Result<()> {
        let now: Instant = Instant::now();
        let ttl: Duration = Duration::from_secs(2);
        let default_ttl: Duration = Duration::from_secs(1);
        let mut cache: HashTtlCache<&str, char> = HashTtlCache::<&str, char>::new(now, Some(default_ttl));
        // insert object with default TTL
        cache.insert("a", 'a');
        // insert object with some TTL
        cache.insert_with_ttl("b", 'b', Some(ttl));

        // make sure object is in the cache
        crate::ensure_eq!(cache.get(&"a"), Some(&'a'));
        crate::ensure_eq!(cache.get(&"b"), Some(&'b'));

        // remove object
        cache.remove(&"a");
        cache.remove(&"b");

        // make sure object is not in cache
        crate::ensure_eq!(cache.get(&"a"), None);
        crate::ensure_eq!(cache.get(&"b"), None);

        Ok(())
    }
}
