//! Shared state for tool coordination — read tracking, session state.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Per-file read record — tracks when a file was last read and its content hash.
#[derive(Debug, Clone)]
pub struct ReadRecord {
    pub path: PathBuf,
    pub timestamp: SystemTime,
    pub content_hash: u64,
    pub is_partial: bool, // true if offset/limit was used
}

/// Thread-safe read tracker shared between FileReadTool and FileEditTool.
/// Enforces read-before-edit: FileEditTool rejects edits to files that
/// haven't been read this session.
#[derive(Clone, Default)]
pub struct ReadTracker {
    records: Arc<Mutex<HashSet<String>>>,
    detailed: Arc<Mutex<Vec<ReadRecord>>>,
}

impl ReadTracker {
    pub fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(HashSet::new())),
            detailed: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Record that a file was read. Called by FileReadTool after successful read.
    pub fn record_read(&self, path: &str) {
        let normalized = path.replace('\\', "/");
        let mut records = self.records.lock().unwrap();
        records.insert(normalized.clone());

        let mut detailed = self.detailed.lock().unwrap();
        let ts_ns = SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        detailed.push(ReadRecord {
            path: PathBuf::from(&normalized),
            timestamp: SystemTime::now(),
            content_hash: ts_ns,
            is_partial: false,
        });
    }

    /// Record a partial read (with offset/limit).
    pub fn record_partial_read(&self, path: &str) {
        let normalized = path.replace('\\', "/");
        let mut records = self.records.lock().unwrap();
        records.insert(normalized.clone());

        let mut detailed = self.detailed.lock().unwrap();
        detailed.push(ReadRecord {
            path: PathBuf::from(&normalized),
            timestamp: SystemTime::now(),
            content_hash: 0,
            is_partial: true,
        });
    }

    /// Check if a file has been read this session.
    pub fn has_been_read(&self, path: &str) -> bool {
        let normalized = path.replace('\\', "/");
        let records = self.records.lock().unwrap();
        records.contains(&normalized)
    }

    /// Get the last read record for a file.
    pub fn get_last_read(&self, path: &str) -> Option<ReadRecord> {
        let normalized = path.replace('\\', "/");
        let detailed = self.detailed.lock().unwrap();
        detailed.iter()
            .filter(|r| r.path.to_string_lossy().replace('\\', "/") == normalized)
            .next_back()
            .cloned()
    }

    /// Clear all read records (e.g., on session reset).
    pub fn clear(&self) {
        self.records.lock().unwrap().clear();
        self.detailed.lock().unwrap().clear();
    }

    /// Number of tracked files.
    pub fn len(&self) -> usize {
        self.records.lock().unwrap().len()
    }

    /// Check if a file was modified since the last read record — concurrent modification detection.
    pub fn was_modified_since_read(&self, path: &str) -> Option<bool> {
        let last = self.get_last_read(path)?;
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta.modified().ok()?;
        // If file was modified after our last read, it's been changed externally
        Some(mtime > last.timestamp)
    }
}

/// Find files with similar names in the same parent directory.
/// Used to suggest correct paths when a file isn't found.
pub fn find_similar_file(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    let parent = p.parent()?;
    let name = p.file_name()?.to_str()?;

    let entries: Vec<_> = std::fs::read_dir(parent).ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            let score = similarity_score(&n, name);
            if score > 0.5 { Some((n, score)) } else { None }
        })
        .collect();

    if entries.is_empty() { return None; }

    // Find the best match
    let best = entries.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;
    Some(best.0.clone())
}

/// Suggest a path relative to cwd. If the path doesn't exist, try prepending the current dir.
pub fn suggest_path_under_cwd(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    if p.exists() { return None; }
    if p.is_absolute() { return None; }
    let cwd = std::env::current_dir().ok()?;
    let try_path = cwd.join(p);
    if try_path.exists() {
        Some(try_path.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Simple similarity score based on common prefix and length ratio.
/// Returns 0.0-1.0.
fn similarity_score(a: &str, b: &str) -> f64 {
    let common_prefix = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let max_len = a.len().max(b.len()) as f64;
    if max_len == 0.0 { return 1.0; }
    let prefix_score = common_prefix as f64 / max_len;
    let length_score = 1.0 - (a.len() as f64 - b.len() as f64).abs() / max_len;
    (prefix_score * 0.6 + length_score * 0.4).clamp(0.0, 1.0)
}
