//! DNS caching layer.
//!
//! Provides a thread-safe cache for DNS lookup results with TTL-based expiration.

use parking_lot::RwLock;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::lookup::LookupIp;

/// Configuration for the DNS cache.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum number of entries in the cache.
    pub max_entries: usize,
    /// Minimum TTL (floor) for cache entries.
    pub min_ttl: Duration,
    /// Maximum TTL (ceiling) for cache entries.
    pub max_ttl: Duration,
    /// TTL for negative cache entries (NXDOMAIN).
    pub negative_ttl: Duration,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            min_ttl: Duration::from_mins(1),
            max_ttl: Duration::from_hours(24), // 24 hours
            negative_ttl: Duration::from_secs(30),
        }
    }
}

/// A cache entry with expiration time.
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    expires_at: Instant,
    inserted_at: Instant,
}

impl<T> CacheEntry<T> {
    fn new(data: T, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            data,
            expires_at: now + ttl,
            inserted_at: now,
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    fn remaining_ttl(&self) -> Duration {
        self.expires_at.saturating_duration_since(Instant::now())
    }
}

/// Thread-safe DNS cache.
#[derive(Debug)]
pub struct DnsCache {
    ip_cache: RwLock<HashMap<String, CacheEntry<LookupIp>>>,
    config: CacheConfig,
    stat_hits: AtomicU64,
    stat_misses: AtomicU64,
    stat_evictions: AtomicU64,
}

impl DnsCache {
    /// Creates a new DNS cache with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    /// Creates a new DNS cache with custom configuration.
    #[must_use]
    pub fn with_config(config: CacheConfig) -> Self {
        Self {
            ip_cache: RwLock::new(HashMap::new()),
            config,
            stat_hits: AtomicU64::new(0),
            stat_misses: AtomicU64::new(0),
            stat_evictions: AtomicU64::new(0),
        }
    }

    /// Looks up an IP address result from the cache.
    pub fn get_ip(&self, host: &str) -> Option<LookupIp> {
        let key = normalize_host_key(host);

        // Fast path: check expiry under read lock, clone only the data.
        {
            let cache = self.ip_cache.read();
            if let Some(entry) = cache.get(key.as_ref()) {
                if !entry.is_expired() {
                    self.stat_hits.fetch_add(1, Ordering::Relaxed);
                    return Some(entry.data.clone());
                }
            } else {
                self.stat_misses.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        }

        // Slow path: entry was expired — upgrade to write lock to evict.
        let mut evicted_expired = false;
        let refreshed = {
            let mut cache = self.ip_cache.write();
            let expired = cache.get(key.as_ref()).is_some_and(CacheEntry::is_expired);
            if expired {
                cache.remove(key.as_ref());
                evicted_expired = true;
                None
            } else {
                // Another thread may have refreshed the entry between locks.
                cache.get(key.as_ref()).map(|entry| entry.data.clone())
            }
        };
        if evicted_expired {
            self.stat_evictions.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(data) = refreshed {
            self.stat_hits.fetch_add(1, Ordering::Relaxed);
            return Some(data);
        }
        self.stat_misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Inserts an IP address lookup result into the cache.
    pub fn put_ip(&self, host: &str, lookup: &LookupIp) {
        if self.config.max_entries == 0 {
            let evicted = {
                let mut cache = self.ip_cache.write();
                if cache.is_empty() {
                    0
                } else {
                    let evicted = cache.len();
                    cache.clear();
                    evicted
                }
            };
            if evicted > 0 {
                self.stat_evictions
                    .fetch_add(evicted as u64, Ordering::Relaxed);
            }
            return;
        }

        let ttl = self.clamp_ttl(lookup.ttl());
        let key = normalize_host_key(host);

        let mut cache = self.ip_cache.write();
        let is_update = cache.contains_key(key.as_ref());

        // Evict only when inserting a new key at capacity.
        // Updating an existing key must not evict unrelated entries.
        if !is_update && cache.len() >= self.config.max_entries {
            self.evict_expired_locked(&mut cache);

            // If still at capacity, remove oldest
            if cache.len() >= self.config.max_entries {
                if let Some(oldest_key) = Self::find_oldest_key(&cache) {
                    cache.remove(&oldest_key);
                    self.stat_evictions.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        cache.insert(key.into_owned(), CacheEntry::new(lookup.clone(), ttl));
    }

    /// Removes an entry from the cache.
    pub fn remove(&self, host: &str) {
        let key = normalize_host_key(host);
        let mut cache = self.ip_cache.write();
        cache.remove(key.as_ref());
    }

    /// Clears all entries from the cache.
    pub fn clear(&self) {
        let mut cache = self.ip_cache.write();
        cache.clear();
        drop(cache);

        self.stat_hits.store(0, Ordering::Relaxed);
        self.stat_misses.store(0, Ordering::Relaxed);
        self.stat_evictions.store(0, Ordering::Relaxed);
    }

    /// Evicts expired entries from the cache.
    pub fn evict_expired(&self) {
        let mut cache = self.ip_cache.write();
        self.evict_expired_locked(&mut cache);
    }

    /// Returns cache statistics.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn stats(&self) -> CacheStats {
        let cache = self.ip_cache.read();
        let hits = self.stat_hits.load(Ordering::Relaxed);
        let misses = self.stat_misses.load(Ordering::Relaxed);
        let evictions = self.stat_evictions.load(Ordering::Relaxed);

        CacheStats {
            size: cache.len(),
            hits,
            misses,
            evictions,
            hit_rate: if hits + misses > 0 {
                hits as f64 / (hits + misses) as f64
            } else {
                0.0
            },
        }
    }

    fn clamp_ttl(&self, ttl: Duration) -> Duration {
        ttl.max(self.config.min_ttl).min(self.config.max_ttl)
    }

    fn evict_expired_locked(&self, cache: &mut HashMap<String, CacheEntry<LookupIp>>) {
        let before = cache.len();
        cache.retain(|_, entry| !entry.is_expired());
        let evicted = before - cache.len();

        if evicted > 0 {
            self.stat_evictions
                .fetch_add(evicted as u64, Ordering::Relaxed);
        }
    }

    fn find_oldest_key(cache: &HashMap<String, CacheEntry<LookupIp>>) -> Option<String> {
        cache
            .iter()
            .min_by(|(left_key, left_entry), (right_key, right_entry)| {
                left_entry
                    .inserted_at
                    .cmp(&right_entry.inserted_at)
                    .then_with(|| left_key.cmp(right_key))
            })
            .map(|(key, _)| key.clone())
    }
}

impl Default for DnsCache {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_host_key(host: &str) -> Cow<'_, str> {
    if host.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(host.to_ascii_lowercase())
    } else {
        Cow::Borrowed(host)
    }
}

/// Statistics about cache usage.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of entries currently in the cache.
    pub size: usize,
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of entries evicted.
    pub evictions: u64,
    /// Hit rate (hits / (hits + misses)).
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn init_test(name: &str) {
        crate::test_utils::init_test_logging();
        crate::test_phase!(name);
    }

    // =========================================================================
    // Wave 46 – pure data-type trait coverage
    // =========================================================================

    #[test]
    fn cache_config_debug_clone_default() {
        let def = CacheConfig::default();
        assert_eq!(def.max_entries, 10_000);
        assert_eq!(def.min_ttl, Duration::from_mins(1));
        assert_eq!(def.negative_ttl, Duration::from_secs(30));
        let dbg = format!("{def:?}");
        assert!(dbg.contains("CacheConfig"), "{dbg}");
        let cloned = def.clone();
        assert_eq!(cloned.max_entries, def.max_entries);
    }

    #[test]
    fn cache_stats_debug_clone_default() {
        let stats = CacheStats::default();
        assert_eq!(stats.size, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.evictions, 0);
        assert!(
            (stats.hit_rate).abs() < f64::EPSILON,
            "expected 0.0, got {}",
            stats.hit_rate
        );
        let dbg = format!("{stats:?}");
        assert!(dbg.contains("CacheStats"), "{dbg}");
        let cloned = stats.clone();
        assert_eq!(cloned.size, stats.size);
    }

    #[test]
    fn dns_cache_debug_default() {
        let cache = DnsCache::default();
        let dbg = format!("{cache:?}");
        assert!(dbg.contains("DnsCache"), "{dbg}");
    }

    #[test]
    fn cache_hit_miss() {
        init_test("cache_hit_miss");
        let cache = DnsCache::new();

        // Miss
        let miss = cache.get_ip("example.com");
        crate::assert_with_log!(miss.is_none(), "cache miss", true, miss.is_none());
        let misses = cache.stats().misses;
        crate::assert_with_log!(misses == 1, "misses", 1, misses);
        let hits = cache.stats().hits;
        crate::assert_with_log!(hits == 0, "hits", 0, hits);

        // Insert
        let lookup = LookupIp::new(
            vec!["192.0.2.1".parse::<IpAddr>().unwrap()],
            Duration::from_mins(5),
        );
        cache.put_ip("example.com", &lookup);

        // Hit
        let result = cache.get_ip("example.com");
        crate::assert_with_log!(result.is_some(), "cache hit", true, result.is_some());
        let hits = cache.stats().hits;
        crate::assert_with_log!(hits == 1, "hits", 1, hits);
        crate::test_complete!("cache_hit_miss");
    }

    #[test]
    fn cache_lookup_is_case_insensitive() {
        init_test("cache_lookup_is_case_insensitive");
        let cache = DnsCache::new();
        let lookup = LookupIp::new(
            vec!["192.0.2.10".parse::<IpAddr>().unwrap()],
            Duration::from_mins(5),
        );

        cache.put_ip("Example.COM", &lookup);

        let lower = cache.get_ip("example.com");
        let upper = cache.get_ip("EXAMPLE.COM");
        crate::assert_with_log!(lower.is_some(), "lower lookup hit", true, lower.is_some());
        crate::assert_with_log!(upper.is_some(), "upper lookup hit", true, upper.is_some());

        let stats = cache.stats();
        crate::assert_with_log!(stats.size == 1, "cache size", 1, stats.size);
        crate::assert_with_log!(stats.hits == 2, "cache hits", 2, stats.hits);
        crate::test_complete!("cache_lookup_is_case_insensitive");
    }

    #[test]
    fn cache_expiration() {
        init_test("cache_expiration");
        let config = CacheConfig {
            min_ttl: Duration::from_millis(1),
            max_ttl: Duration::from_millis(50),
            ..Default::default()
        };
        let cache = DnsCache::with_config(config);

        let lookup = LookupIp::new(
            vec!["192.0.2.1".parse::<IpAddr>().unwrap()],
            Duration::from_millis(1), // Very short TTL
        );
        cache.put_ip("example.com", &lookup);

        // Should be in cache immediately
        let immediate = cache.get_ip("example.com");
        crate::assert_with_log!(
            immediate.is_some(),
            "immediate hit",
            true,
            immediate.is_some()
        );

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(10));

        // Should be expired
        let expired = cache.get_ip("example.com");
        crate::assert_with_log!(expired.is_none(), "expired", true, expired.is_none());
        let size = cache.stats().size;
        crate::assert_with_log!(size == 0, "expired evicted", 0, size);
        crate::test_complete!("cache_expiration");
    }

    #[test]
    fn cache_clear() {
        init_test("cache_clear");
        let cache = DnsCache::new();

        let lookup = LookupIp::new(
            vec!["192.0.2.1".parse::<IpAddr>().unwrap()],
            Duration::from_mins(5),
        );
        cache.put_ip("example.com", &lookup);
        let size = cache.stats().size;
        crate::assert_with_log!(size > 0, "size > 0", ">0", size);

        cache.clear();
        let size = cache.stats().size;
        crate::assert_with_log!(size == 0, "size 0", 0, size);
        crate::test_complete!("cache_clear");
    }

    #[test]
    fn cache_ttl_clamping() {
        init_test("cache_ttl_clamping");
        let config = CacheConfig {
            min_ttl: Duration::from_mins(1),
            max_ttl: Duration::from_hours(1),
            ..Default::default()
        };
        let cache = DnsCache::with_config(config);

        // TTL below minimum should be clamped
        let lookup = LookupIp::new(
            vec!["192.0.2.1".parse::<IpAddr>().unwrap()],
            Duration::from_secs(10), // Below minimum
        );
        cache.put_ip("example.com", &lookup);

        // Entry should exist
        let result = cache.get_ip("example.com");
        crate::assert_with_log!(result.is_some(), "entry exists", true, result.is_some());
        crate::test_complete!("cache_ttl_clamping");
    }

    #[test]
    fn cache_max_entries_zero_disables_inserts() {
        init_test("cache_max_entries_zero");
        let config = CacheConfig {
            max_entries: 0,
            ..Default::default()
        };
        let cache = DnsCache::with_config(config);

        let lookup = LookupIp::new(
            vec!["192.0.2.1".parse::<IpAddr>().unwrap()],
            Duration::from_mins(5),
        );
        cache.put_ip("example.com", &lookup);

        let result = cache.get_ip("example.com");
        crate::assert_with_log!(result.is_none(), "no entry", true, result.is_none());
        let size = cache.stats().size;
        crate::assert_with_log!(size == 0, "size 0", 0, size);
        crate::test_complete!("cache_max_entries_zero");
    }

    #[test]
    fn cache_update_existing_key_at_capacity_does_not_evict_other_entry() {
        init_test("cache_update_existing_key_at_capacity_does_not_evict_other_entry");
        let config = CacheConfig {
            max_entries: 2,
            ..Default::default()
        };
        let cache = DnsCache::with_config(config);

        let a1 = LookupIp::new(
            vec!["192.0.2.1".parse::<IpAddr>().expect("ip parse")],
            Duration::from_mins(5),
        );
        let b1 = LookupIp::new(
            vec!["192.0.2.2".parse::<IpAddr>().expect("ip parse")],
            Duration::from_mins(5),
        );
        let b2 = LookupIp::new(
            vec!["192.0.2.20".parse::<IpAddr>().expect("ip parse")],
            Duration::from_mins(5),
        );

        cache.put_ip("a.example", &a1);
        cache.put_ip("b.example", &b1);
        cache.put_ip("b.example", &b2);

        let a = cache.get_ip("a.example");
        let b = cache.get_ip("b.example");
        crate::assert_with_log!(a.is_some(), "a still present", true, a.is_some());
        crate::assert_with_log!(b.is_some(), "b still present", true, b.is_some());

        let b_first = b.and_then(|lookup| lookup.first());
        crate::assert_with_log!(
            b_first == Some("192.0.2.20".parse::<IpAddr>().expect("ip parse")),
            "b updated in place",
            "192.0.2.20",
            format!("{b_first:?}")
        );

        let stats = cache.stats();
        crate::assert_with_log!(stats.size == 2, "size remains at capacity", 2, stats.size);
        crate::assert_with_log!(
            stats.evictions == 0,
            "no unrelated eviction on update",
            0,
            stats.evictions
        );
        crate::test_complete!("cache_update_existing_key_at_capacity_does_not_evict_other_entry");
    }

    #[test]
    fn cache_capacity_eviction_breaks_equal_insert_times_deterministically() {
        init_test("cache_capacity_eviction_breaks_equal_insert_times_deterministically");
        let config = CacheConfig {
            max_entries: 2,
            ..Default::default()
        };
        let cache = DnsCache::with_config(config);
        let inserted_at = Instant::now();
        let ttl = Duration::from_mins(5);

        let alpha = LookupIp::new(vec!["192.0.2.1".parse::<IpAddr>().expect("ip parse")], ttl);
        let zeta = LookupIp::new(vec!["192.0.2.2".parse::<IpAddr>().expect("ip parse")], ttl);
        let middle = LookupIp::new(vec!["192.0.2.3".parse::<IpAddr>().expect("ip parse")], ttl);

        {
            let mut entries = cache.ip_cache.write();
            entries.insert(
                "zeta.example".to_string(),
                CacheEntry {
                    data: zeta,
                    inserted_at,
                    expires_at: inserted_at + ttl,
                },
            );
            entries.insert(
                "alpha.example".to_string(),
                CacheEntry {
                    data: alpha,
                    inserted_at,
                    expires_at: inserted_at + ttl,
                },
            );
        }

        cache.put_ip("middle.example", &middle);

        let alpha_cached = cache.get_ip("alpha.example");
        let zeta_cached = cache.get_ip("zeta.example");
        let middle_cached = cache.get_ip("middle.example");
        crate::assert_with_log!(
            alpha_cached.is_none(),
            "lexicographically smallest equal-age key evicted first",
            true,
            alpha_cached.is_none()
        );
        crate::assert_with_log!(
            zeta_cached.is_some(),
            "later lexical key remains when insert times tie",
            true,
            zeta_cached.is_some()
        );
        crate::assert_with_log!(
            middle_cached.is_some(),
            "new entry inserted after deterministic eviction",
            true,
            middle_cached.is_some()
        );

        let stats = cache.stats();
        crate::assert_with_log!(stats.size == 2, "size stays at capacity", 2, stats.size);
        crate::assert_with_log!(
            stats.evictions == 1,
            "single eviction recorded",
            1,
            stats.evictions
        );
        crate::test_complete!(
            "cache_capacity_eviction_breaks_equal_insert_times_deterministically"
        );
    }
}
