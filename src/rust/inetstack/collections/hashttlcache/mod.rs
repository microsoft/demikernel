// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(test)]
mod tests;

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
