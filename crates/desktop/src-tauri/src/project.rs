//! Project lifecycle management — .aegis/ directory creation, scanning, config.
//!
//! ## Directory structure created by `init()`:
//! ```text
//! .aegis/
//!   config.toml
//!   rules/
//!   memory.db
//!   graph.db
//!   skills/
//!   agents/
//!   sessions/
//!   plans/
//!   sandbox/
//! ```
//! Plus automatic `.gitignore` entries for local-only files.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub root: String,
    pub language: Option<String>,
    pub file_count: usize,
    pub has_aegis_dir: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub total_files: usize,
    pub total_functions: usize,
    pub total_modules: usize,
    pub languages: Vec<LanguageCount>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageCount {
    pub name: String,
    pub files: usize,
    pub functions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleFile {
    pub name: String,
    pub content: String,
}

const DEFAULT_CONFIG_TOML: &str = r#"# Aegis project configuration
# See: https://github.com/Cashmeran/Deepseek-Aegis

[model]
name = "deepseek-v4-pro"
effort = "max"

[permissions]
auto_allow = [
    "Bash(cargo *)",
    "Bash(git status)",
    "Bash(git diff *)",
    "Bash(git log *)",
    "Read",
    "Glob",
    "Grep",
    "Edit",
    "Write",
]
ask_before = [
    "Bash(git push *)",
    "Bash(git commit *)",
    "Bash(rm *)",
    "Bash(npm *)",
    "Bash(pip *)",
    "Bash(docker *)",
]
deny = [
    "Read(.env)",
    "Read(.env.*)",
    "Read(*credentials*)",
    "Bash(rm -rf /)",
    "Bash(sudo *)",
]

[project]
name = ""
language = ""

[index]
exclude_dirs = [
    "target", "node_modules", ".git", ".aegis",
    "__pycache__", "dist", "build", "out",
    ".next", ".nuxt", ".venv", "venv",
    ".pytest_cache", ".mypy_cache", ".cache",
    "coverage", ".turbo", ".vercel",
]
exclude_files = [
    "package-lock.json", "yarn.lock", "pnpm-lock.yaml",
    "Cargo.lock", "poetry.lock", "Pipfile.lock",
    "go.sum", ".DS_Store",
]
exclude_exts = [
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".ico",
    ".woff", ".woff2", ".ttf", ".eot", ".otf",
    ".mp4", ".avi", ".mov", ".mp3", ".wav",
    ".zip", ".tar", ".gz", ".rar", ".7z",
    ".pdf", ".doc", ".docx", ".xls", ".xlsx",
    ".exe", ".dll", ".so", ".dylib",
]

[context]
max_turns = 25
verify_before_output = true

[computer_use]
enabled = false
"#;

const GITIGNORE_ENTRIES: &str = "
# Aegis — local-only project files (do not commit)
.aegis/config.local.toml
.aegis/sessions/
.aegis/sandbox/workspace/
.aegis/memory.db
.aegis/graph.db
";

// ═══════════════════════════════════════════════════════════════
// ProjectManager
// ═══════════════════════════════════════════════════════════════

pub struct ProjectManager {
    pub root: PathBuf,
    pub aegis_dir: PathBuf,
    pub meta: ProjectMeta,
}

impl ProjectManager {
    /// Initialize a new `.aegis/` directory tree in the given project root.
    /// Idempotent — if `.aegis/` already exists, returns existing project.
    pub fn init(root: &Path) -> Result<Self, String> {
        let aegis_dir = root.join(".aegis");

        // Create directory tree
        for sub in &["rules", "skills", "agents", "sessions", "plans", "sandbox"] {
            fs::create_dir_all(aegis_dir.join(sub))
                .map_err(|e| format!("mkdir .aegis/{sub}: {e}"))?;
        }

        // Write default config if missing
        let config_path = aegis_dir.join("config.toml");
        if !config_path.exists() {
            let mut f = fs::File::create(&config_path)
                .map_err(|e| format!("create config.toml: {e}"))?;
            f.write_all(DEFAULT_CONFIG_TOML.as_bytes())
                .map_err(|e| format!("write config.toml: {e}"))?;
        }

        // Append to .gitignore (if exists) or create one
        let gitignore_path = root.join(".gitignore");
        let entries = GITIGNORE_ENTRIES.trim();
        if gitignore_path.exists() {
            let existing = fs::read_to_string(&gitignore_path).unwrap_or_default();
            if !existing.contains("# Aegis — local-only project files") {
                let mut f = fs::OpenOptions::new()
                    .append(true)
                    .open(&gitignore_path)
                    .map_err(|e| format!("append .gitignore: {e}"))?;
                f.write_all(format!("\n{entries}\n").as_bytes())
                    .map_err(|e| format!("write .gitignore: {e}"))?;
            }
        } else {
            fs::write(&gitignore_path, format!("{entries}\n"))
                .map_err(|e| format!("write .gitignore: {e}"))?;
        }

        // Detect project name from directory
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into());

        // Detect primary language
        let language = Self::detect_language(root);

        let meta = ProjectMeta {
            name,
            root: root.to_string_lossy().to_string(),
            language,
            file_count: 0,
            has_aegis_dir: true,
            created_at: now_ms(),
        };

        Ok(Self {
            root: root.to_path_buf(),
            aegis_dir,
            meta,
        })
    }

    /// Open an existing project (must have `.aegis/` directory).
    pub fn open(root: &Path) -> Result<Self, String> {
        let aegis_dir = root.join(".aegis");
        if !aegis_dir.exists() {
            return Err(format!(
                "No .aegis/ directory found in {}. Run project init first.",
                root.display()
            ));
        }

        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into());

        let language = Self::detect_language(root);
        let file_count = Self::count_files(root);

        Ok(Self {
            root: root.to_path_buf(),
            aegis_dir,
            meta: ProjectMeta {
                name,
                root: root.to_string_lossy().to_string(),
                language,
                file_count,
                has_aegis_dir: true,
                created_at: now_ms(),
            },
        })
    }

    /// Check if a directory has a `.aegis/` project initialized.
    pub fn exists_at(root: &Path) -> bool {
        root.join(".aegis").exists()
    }

    /// Scan project files and return counts (lightweight, no parsing).
    /// For full tree-sitter parsing, use the code-graph crate.
    pub fn scan_files(&self) -> ScanResult {
        let start = std::time::Instant::now();
        let mut total_files = 0usize;
        let mut languages: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        self.walk_project(&self.root, &mut |_path, ext| {
            total_files += 1;
            let lang = ext_to_language(ext);
            *languages.entry(lang.to_string()).or_insert(0) += 1;
        });

        let lang_counts: Vec<LanguageCount> = languages
            .into_iter()
            .map(|(name, files)| LanguageCount {
                name,
                files,
                functions: 0, // populated by tree-sitter scan (separate call)
            })
            .collect();

        ScanResult {
            total_files,
            total_functions: 0,
            total_modules: 0,
            languages: lang_counts,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Full scan with tree-sitter code graph building.
    /// Requires code-graph crate. Falls back to file-count-only scan on error.
    pub fn scan_with_graph(&self) -> Result<ScanResult, String> {
        // This delegates to aegis-code-graph's IncrementalIndexer for full parsing.
        // For now, fall through to lightweight scan.
        // TODO: integrate aegis-code-graph::IncrementalIndexer
        Ok(self.scan_files())
    }

    /// Load all rule files from `.aegis/rules/`.
    pub fn load_rules(&self) -> Vec<RuleFile> {
        let rules_dir = self.aegis_dir.join("rules");
        let mut rules = Vec::new();
        if let Ok(entries) = fs::read_dir(&rules_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "md") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        let name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if !content.trim().is_empty() {
                            rules.push(RuleFile { name, content });
                        }
                    }
                }
            }
        }
        rules
    }

    /// Save a rule file.
    pub fn save_rule(&self, name: &str, content: &str) -> Result<(), String> {
        let path = self.aegis_dir.join("rules").join(format!("{name}.md"));
        fs::write(&path, content).map_err(|e| format!("write rule {name}: {e}"))
    }

    /// List all sessions from `.aegis/sessions/`.
    pub fn list_sessions(&self) -> Vec<String> {
        let sessions_dir = self.aegis_dir.join("sessions");
        let mut sessions = Vec::new();
        if let Ok(entries) = fs::read_dir(&sessions_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".json") && name != "index.json" {
                    sessions.push(name.trim_end_matches(".json").to_string());
                }
            }
        }
        sessions.sort();
        sessions.reverse();
        sessions
    }

    /// Load project config (without local overrides).
    pub fn load_config(&self) -> Result<toml::Table, String> {
        let path = self.aegis_dir.join("config.toml");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("read config.toml: {e}"))?;
        toml::from_str(&content).map_err(|e| format!("parse config.toml: {e}"))
    }

    // ── internal helpers ──

    fn load_ignore_dirs(&self) -> Vec<String> {
        if let Ok(config) = self.load_config() {
            if let Some(arr) = config.get("index")
                .and_then(|i| i.get("exclude_dirs"))
                .and_then(|v| v.as_array())
            {
                return arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            }
        }
        vec!["target".into(), "node_modules".into(), ".git".into(), ".aegis".into()]
    }

    fn load_ignore_exts(&self) -> Vec<String> {
        if let Ok(config) = self.load_config() {
            if let Some(arr) = config.get("index")
                .and_then(|i| i.get("exclude_exts"))
                .and_then(|v| v.as_array())
            {
                return arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            }
        }
        vec![]
    }

    fn walk_project(&self, root: &Path, cb: &mut dyn FnMut(&Path, &str)) {
        let ignore_dirs = self.load_ignore_dirs();
        let ignore_exts = self.load_ignore_exts();
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name: String = path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name.starts_with('.') && name != ".aegis" {
                    continue;
                }
                if path.is_dir() {
                    if ignore_dirs.iter().any(|d| d.as_str() == name.as_str()) {
                        continue;
                    }
                    self.walk_project(&path, cb);
                } else if path.is_file() {
                    let ext: String = path.extension()
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if ignore_exts.iter().any(|e| format!(".{ext}") == *e || e == &ext) {
                        continue;
                    }
                    cb(&path, &ext);
                }
            }
        }
    }

    fn detect_language(root: &Path) -> Option<String> {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext_os) = path.extension() {
                        let ext: String = ext_os.to_string_lossy().into_owned();
                        let lang = ext_to_language(&ext).to_string();
                        *counts.entry(lang).or_insert(0) += 1;
                    }
                }
            }
        }
        counts.into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(lang, _)| lang)
    }

    fn count_files(root: &Path) -> usize {
        let mut count = 0usize;
        // Use lightweight hardcoded list for count (no config parse overhead)
        let ignore_dirs: &[&str] = &["target", "node_modules", ".git", ".aegis", "__pycache__", "dist", "build"];
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name: String = path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name.starts_with('.') && name != ".aegis" { continue; }
                if path.is_dir() {
                    if !ignore_dirs.contains(&name.as_ref()) {
                        count += Self::count_files(&path);
                    }
                } else if path.is_file() { count += 1; }
            }
        }
        count
    }
}

// ═══════════════════════════════════════════════════════════════
// Tauri commands
// ═══════════════════════════════════════════════════════════════

#[tauri::command]
pub fn project_init(cwd: String) -> Result<ProjectMeta, String> {
    let root = PathBuf::from(&cwd);
    let pm = ProjectManager::init(&root)?;
    let mut meta = pm.meta;
    meta.file_count = ProjectManager::count_files(&root);
    Ok(meta)
}

#[tauri::command]
pub fn project_open(cwd: String) -> Result<ProjectMeta, String> {
    let root = PathBuf::from(&cwd);
    let pm = ProjectManager::open(&root)?;
    Ok(pm.meta)
}

#[tauri::command]
pub fn project_scan(cwd: String) -> Result<ScanResult, String> {
    let root = PathBuf::from(&cwd);
    let pm = if ProjectManager::exists_at(&root) {
        ProjectManager::open(&root)?
    } else {
        ProjectManager::init(&root)?
    };
    Ok(pm.scan_files())
}

#[tauri::command]
pub fn project_check(cwd: String) -> Result<bool, String> {
    Ok(ProjectManager::exists_at(&PathBuf::from(&cwd)))
}

#[tauri::command]
pub fn project_list_rules(cwd: String) -> Result<Vec<RuleFile>, String> {
    let root = PathBuf::from(&cwd);
    let pm = ProjectManager::open(&root)?;
    Ok(pm.load_rules())
}

#[tauri::command]
pub fn project_save_rule(cwd: String, name: String, content: String) -> Result<(), String> {
    let root = PathBuf::from(&cwd);
    let pm = ProjectManager::open(&root)?;
    pm.save_rule(&name, &content)
}

#[tauri::command]
pub fn project_list_sessions(cwd: String) -> Result<Vec<String>, String> {
    let root = PathBuf::from(&cwd);
    let pm = ProjectManager::open(&root)?;
    Ok(pm.list_sessions())
}

#[tauri::command]
pub fn list_project_files(cwd: String) -> Result<Vec<String>, String> {
    let root = PathBuf::from(&cwd);
    let mut paths = Vec::new();
    collect_files(&root, &root, &mut paths);
    Ok(paths)
}

fn collect_files(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    let ignore: &[&str] = &["target", "node_modules", ".git", ".aegis", "__pycache__", "dist", "build", ".next"];
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') { continue; }
            if path.is_dir() {
                if ignore.contains(&name.as_str()) { continue; }
                collect_files(base, &path, out);
            } else {
                if let Ok(rel) = path.strip_prefix(base) {
                    out.push(rel.to_string_lossy().to_string());
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn ext_to_language(ext: &str) -> &str {
    match ext {
        "rs" => "Rust",
        "py" => "Python",
        "js" | "mjs" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "go" => "Go",
        "java" => "Java",
        "cpp" | "cc" | "cxx" => "C++",
        "c" => "C",
        "h" | "hpp" => "C/C++ Header",
        "rb" => "Ruby",
        "swift" => "Swift",
        "kt" | "kts" => "Kotlin",
        "scala" => "Scala",
        "php" => "PHP",
        "html" | "htm" => "HTML",
        "css" | "scss" | "less" => "CSS",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "md" | "mdx" => "Markdown",
        "sql" => "SQL",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" => "PowerShell",
        "dockerfile" => "Dockerfile",
        _ => "Other",
    }
}
