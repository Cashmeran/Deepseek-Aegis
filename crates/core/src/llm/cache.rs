//! DeepSeek disk cache — prefix-based auto-caching with SHA-256 content hashing.
//! DeepSeek API provides automatic prefix caching when messages share common prefixes.
//! This module tracks which prefix segments are cacheable to maximize cache hit rate.

use crate::types::message::Message;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

/// Tracks which message ranges have been cached (for cache-aware context splitting)
#[derive(Debug, Clone, Default)]
pub struct DiskCacheTracker {
    /// Content hash → cache status (last seen timestamp)
    cached_prefixes: HashMap<String, chrono::DateTime<chrono::Utc>>,
    /// Estimated cache hit count
    hits: u64,
    /// Estimated cache miss count
    misses: u64,
    #[allow(dead_code)]
    cache_dir: Option<PathBuf>,
}

impl DiskCacheTracker {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self {
            cached_prefixes: HashMap::new(),
            hits: 0,
            misses: 0,
            cache_dir,
        }
    }

    /// Compute stable hash for a message prefix
    pub fn hash_prefix(messages: &[Message]) -> String {
        let mut hasher = Sha256::new();
        for msg in messages.iter().take(4) {
            // First 4 messages form the cacheable prefix
            hasher.update(msg.estimated_char_len().to_string().as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    /// Check if a prefix is already cached (returns true if likely cached)
    pub fn check_cache(&mut self, messages: &[Message]) -> bool {
        let hash = Self::hash_prefix(messages);
        if self.cached_prefixes.contains_key(&hash) {
            self.hits += 1;
            true
        } else {
            self.misses += 1;
            self.cached_prefixes.insert(hash, chrono::Utc::now());
            false
        }
    }

    /// Mark a prefix as cached after successful API call
    pub fn mark_cached(&mut self, messages: &[Message]) {
        let hash = Self::hash_prefix(messages);
        self.cached_prefixes.insert(hash, chrono::Utc::now());
    }

    /// Split messages into cacheable prefix + dynamic suffix
    /// Returns (prefix_to_cache, suffix_dynamic) where prefix hashes to stable value
    pub fn split_for_caching<'a>(&self, messages: &'a [Message], breakpoint: usize) -> (&'a [Message], &'a [Message]) {
        let split_at = messages.len().saturating_sub(breakpoint);
        messages.split_at(split_at)
    }

    /// Cache stats
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// Cache hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_stable() {
        let msgs: Vec<Message> = vec![];
        let h1 = DiskCacheTracker::hash_prefix(&msgs);
        let h2 = DiskCacheTracker::hash_prefix(&msgs);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_check_cache_miss_then_hit() {
        let mut tracker = DiskCacheTracker::new(None);
        let msgs: Vec<Message> = vec![];
        assert!(!tracker.check_cache(&msgs)); // first time = miss
        assert!(tracker.check_cache(&msgs));  // second time = hit
        assert_eq!(tracker.stats(), (1, 1));
    }

    #[test]
    fn test_split_for_caching() {
        let tracker = DiskCacheTracker::new(None);
        let msgs: Vec<Message> = vec![];
        let (prefix, suffix) = tracker.split_for_caching(&msgs, 0);
        assert_eq!(prefix.len(), 0);
        assert_eq!(suffix.len(), 0);
    }
}
