use crate::language::LanguageRegistry;
use crate::parser::CodeParser;
use crate::store::GraphStore;
use crate::types::*;
use aegis_core::error::{AgentError, AgentResult};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 增量索引器 — 后台全量扫描 + 文件变更检测
pub struct IncrementalIndexer {
    store: Arc<dyn GraphStore>,
    parser: Arc<CodeParser>,
    registry: Arc<LanguageRegistry>,
    #[allow(dead_code)]
    concurrency_limit: usize, // reserved for parallel batch indexing
}

impl IncrementalIndexer {
    pub fn new(
        store: Arc<dyn GraphStore>,
        parser: Arc<CodeParser>,
        registry: Arc<LanguageRegistry>,
    ) -> Self {
        let concurrency_limit =
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Self { store, parser, registry, concurrency_limit }
    }

    /// 处理单个文件: 检测变更 → 解析 → 提取 → 存储
    pub fn process_file(&self, file_path: &Path) -> AgentResult<FileChange> {
        let content = std::fs::read_to_string(file_path).map_err(|e| {
            AgentError::FileNotFound { path: format!("{}: {}", file_path.display(), e) }
        })?;

        let new_hash = {
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        // 快速路径: hash 未变则跳过
        let path_str = file_path.to_string_lossy().replace('\\', "/");
        if let Ok(Some(old_hash)) = self.store.get_file_hash(Path::new(&path_str)) {
            if old_hash == new_hash {
                return Ok(FileChange::Unchanged);
            }
        }

        // 检测语言
        let lang = self.parser.detect_language(&path_str).ok_or_else(|| {
            AgentError::ConfigError(format!("Unsupported file: {}", path_str))
        })?;

        // 解析
        let tree = self.parser.parse(&content, lang.as_ref(), None)?;
        let parse_errors = CodeParser::count_parse_errors(&tree, content.as_bytes());

        // 提取
        let (nodes, edges) =
            crate::extractor::GraphExtractor::extract(lang.as_ref(), &tree, &content, &path_str, &new_hash)?;

        let n_count = nodes.len();
        let e_count = edges.len();

        // 存储
        self.store.upsert_file_nodes(file_path, &nodes, &edges)?;

        Ok(FileChange::Updated { nodes_added: n_count, edges_added: e_count, parse_errors })
    }

    /// 移除已从磁盘删除的文件
    pub fn remove_deleted_files(&self, known_files: &HashSet<String>) -> AgentResult<usize> {
        let indexed = self.store.list_files()?;
        let mut removed = 0;
        for path in indexed {
            let path_str = path.to_string_lossy().replace('\\', "/");
            if !known_files.contains(&path_str) {
                self.store.remove_file(&path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// 全量扫描工作区（同步版本）
    pub fn full_scan(&self, workspace_root: &Path) -> AgentResult<FullScanResult> {
        use walkdir::WalkDir;
        let start = std::time::Instant::now();

        let exts = self.registry.supported_extensions();
        let mut all_files: Vec<PathBuf> = Vec::new();
        let mut skipped = 0usize;
        let mut updated = 0usize;
        let mut errors = 0usize;

        for entry in WalkDir::new(workspace_root)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.')
                    && name != "target"
                    && name != "node_modules"
                    && name != ".git"
            })
        {
            if let Ok(entry) = entry {
                if entry.file_type().is_file() {
                    if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                        if exts.iter().any(|s| s == ext) {
                            all_files.push(entry.path().to_path_buf());
                        }
                    }
                }
            }
        }

        let total = all_files.len();
        let mut known: HashSet<String> = HashSet::new();
        let mut first_error: Option<String> = None;

        for file_path in all_files {
            let path_str = file_path.to_string_lossy().replace('\\', "/");
            known.insert(path_str.clone());

            match self.process_file(&file_path) {
                Ok(FileChange::Updated { .. }) => updated += 1,
                Ok(FileChange::Unchanged) => skipped += 1,
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(format!("{}: {e}", path_str));
                    }
                    errors += 1;
                    tracing::warn!("Failed to index {}: {}", path_str, e);
                }
            }
        }

        let removed = self.remove_deleted_files(&known)?;

        // Resolve cross-file call edges (e.g., main.rs calling fn defined in calculator.rs)
        let resolved = self.store.resolve_cross_file_calls().unwrap_or(0);
        if resolved > 0 {
            tracing::info!("Resolved {} cross-file call edges", resolved);
        }

        Ok(FullScanResult {
            total_files: total,
            updated,
            skipped,
            removed,
            errors,
            first_error,
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }
}
