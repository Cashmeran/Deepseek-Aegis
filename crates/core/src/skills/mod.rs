use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ═══════════════════════════════════════════════════════════════
// Skill 系统 — 严格对齐 Claude Code 源码设计
//
// 参考: cc源码/skills/loadSkillsDir.ts + bundledSkills.ts
//
// 格式: .agent/skills/<skill-name>/SKILL.md (目录格式，唯一支持)
// 优先级: 深层目录 > 浅层目录 (先加载者胜)
// 加载源: project (.agent/skills/) > user (~/.aegis/skills/) > bundled
// ═══════════════════════════════════════════════════════════════

/// YAML frontmatter 元数据。parseSkillFrontmatterFields()。
#[derive(Debug, Clone, Default)]
pub struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    /// 何时应注入此 Skill (触发条件描述)
    pub when_to_use: Option<String>,
    /// 用户可通过 /skill-name 手动调用
    pub user_invocable: bool,
    /// 允许的工具列表 (空=全部)
    pub allowed_tools: Vec<String>,
    /// 禁止模型自动调用
    pub disable_model_invocation: bool,
    /// 版本
    pub version: Option<String>,
    /// 条件激活: 匹配的文件路径 glob (空=始终激活)
    pub paths: Vec<String>,
    /// 推荐模型
    pub model: Option<String>,
    /// 执行上下文: inline | fork
    pub context: Option<String>,
    /// 参数提示
    pub argument_hint: Option<String>,
    /// 参数名列表
    pub arguments: Vec<String>,
}

impl SkillFrontmatter {
    /// 解析 YAML frontmatter。格式parseFrontmatter()。
    pub fn parse(content: &str) -> (Self, &str) {
        let content = content.trim_start();
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return (Self::default(), content);
        }

        let after_first = &content[4..];
        if let Some(end) = after_first.find("\n---") {
            let fm_text = &after_first[..end];
            let body_start = 4 + end + 4; // 4 (leading "---\n") + end + 4 ("\n---")
            let body = content[body_start..].trim_start();
            (Self::parse_yaml(fm_text), body)
        } else {
            (Self::default(), content)
        }
    }

    fn parse_yaml(text: &str) -> Self {
        let mut fm = Self {
            user_invocable: true, // CC 默认: user-invocable=true
            ..Default::default()
        };

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim();
                match key {
                    "name" => fm.name = Some(value.into()),
                    "description" => fm.description = Some(value.into()),
                    "when_to_use" | "when-to-use" => fm.when_to_use = Some(value.into()),
                    "user-invocable" | "user_invocable" => fm.user_invocable = value != "false",
                    "disable-model-invocation" | "disable_model_invocation" => {
                        fm.disable_model_invocation = value == "true"
                    }
                    "allowed-tools" | "allowed_tools" => fm.allowed_tools = parse_list(value),
                    "paths" => fm.paths = parse_list(value),
                    "arguments" => fm.arguments = parse_list(value),
                    "version" => fm.version = Some(value.into()),
                    "model" => fm.model = Some(value.into()),
                    "argument-hint" | "argument_hint" => fm.argument_hint = Some(value.into()),
                    "context" => fm.context = Some(value.into()),
                    _ => {}
                }
            }
        }
        fm
    }
}

fn parse_list(value: &str) -> Vec<String> {
    let value = value.trim_matches(|c| c == '[' || c == ']');
    if value.is_empty() {
        return vec![];
    }
    value
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Skill 实例。Command (type='prompt')。
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub has_user_specified_description: bool,
    pub frontmatter: SkillFrontmatter,
    /// 不含 frontmatter 的 Markdown 内容
    pub content: String,
    /// 原始文件内容 (含 frontmatter)
    pub raw_content: String,
    /// SKILL.md 所在目录 (作为 baseDir)
    pub skill_root: Option<PathBuf>,
    /// 加载来源
    pub loaded_from: LoadedFrom,
}

/// 加载来源枚举。LoadedFrom。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadedFrom {
    /// `.agent/skills/<name>/SKILL.md`
    Skills,
    /// `~/.aegis/skills/<name>/SKILL.md`
    User,
    /// 编译进 binary 的内置 Skill
    Bundled,
}

impl Skill {
    /// 从 SKILL.md 文件加载。目录名即为 Skill 名，文件必须名为 SKILL.md。
    /// loadSkillsFromSkillsDir()。
    pub fn from_skill_md(skill_dir: &Path) -> Result<Self, std::io::Error> {
        let skill_file = skill_dir.join("SKILL.md");
        let raw_content = std::fs::read_to_string(&skill_file)?;

        let (frontmatter, content) = SkillFrontmatter::parse(&raw_content);

        let name = frontmatter
            .name
            .clone()
            .unwrap_or_else(|| {
                skill_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unnamed")
                    .to_string()
            });

        let description = frontmatter
            .description
            .clone()
            .unwrap_or_else(|| {
                content
                    .lines()
                    .find(|l| !l.starts_with('#') && !l.trim().is_empty())
                    .unwrap_or("No description")
                    .to_string()
            });

        Ok(Self {
            name,
            has_user_specified_description: frontmatter.description.is_some(),
            description,
            frontmatter,
            content: content.to_string(),
            raw_content,
            skill_root: Some(skill_dir.to_path_buf()),
            loaded_from: LoadedFrom::Skills,
        })
    }

    /// 创建内置 Skill。
    pub fn bundled(name: &str, description: &str, content: &str, when_to_use: Option<&str>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            has_user_specified_description: true,
            frontmatter: SkillFrontmatter {
                name: Some(name.into()),
                description: Some(description.into()),
                when_to_use: when_to_use.map(|s| s.into()),
                user_invocable: true,
                ..Default::default()
            },
            content: content.into(),
            raw_content: content.into(),
            skill_root: None,
            loaded_from: LoadedFrom::Bundled,
        }
    }

    pub fn is_user_invocable(&self) -> bool {
        self.frontmatter.user_invocable
    }

    /// 格式化为系统提示注入。getPromptForCommand()。
    pub fn to_prompt_injection(&self) -> String {
        let mut injection = String::new();
        injection.push_str("<skill-pin>\n");

        // baseDir 前缀 (对齐 CC: 让模型知道 skill 文件位置)
        if let Some(ref root) = self.skill_root {
            injection.push_str(&format!(
                "Base directory for this skill: {}\n\n",
                root.display()
            ));
        }

        injection.push_str(&format!("## Skill: {}\n", self.name));
        if let Some(ref when) = self.frontmatter.when_to_use {
            injection.push_str(&format!("*When to use: {}*\n\n", when));
        }
        injection.push_str(&self.content);
        injection.push_str("\n</skill-pin>\n");
        injection
    }
}

/// Skill 注册中心。
///
/// getSkillDirCommands() 的多源加载 + 去重逻辑:
/// - 深层目录的 skills 优先于浅层 (先加载者胜)
/// - Project (.agent/skills/) > User (~/.aegis/skills/) > Bundled
/// - 条件 Skills (paths frontmatter) 暂存，匹配文件时激活
pub struct SkillRegistry {
    /// 激活的 Skills (name → Skill)
    skills: HashMap<String, Skill>,
    /// 条件 Skills (有 paths frontmatter)
    conditional_skills: HashMap<String, Skill>,
    /// 预格式化的注入文本缓存
    cached_injection: String,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
            conditional_skills: HashMap::new(),
            cached_injection: String::new(),
        }
    }

    // ═══════════════════════════════════════════════
    // 加载 — 多源加载顺序
    // ═══════════════════════════════════════════════

    /// 从项目目录树加载 Skills。
    /// 对齐 CC: 从 cwd 向上走到根，扫描每个 `.agent/skills/` 目录。
    /// 深层优先 (先加载者胜)。
    pub fn load_project_skills(&mut self, cwd: &str) -> Result<usize, std::io::Error> {
        let mut count = 0;
        let mut current = Path::new(cwd).canonicalize().unwrap_or_else(|_| PathBuf::from(cwd));

        // 收集从 cwd 到根的所有 .agent/skills 目录 (深层优先)
        let mut dirs = Vec::new();
        loop {
            let skills_dir = current.join(".agent").join("skills");
            if skills_dir.is_dir() {
                dirs.push(skills_dir);
            }
            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            } else {
                break;
            }
        }

        // 深层优先加载
        for dir in &dirs {
            count += self.load_skills_dir(dir, LoadedFrom::Skills)?;
        }

        Ok(count)
    }

    /// 从用户目录加载 Skills。
    /// 对齐 CC: `~/.aegis/skills/`
    pub fn load_user_skills(&mut self) -> Result<usize, std::io::Error> {
        let home = dirs_next().unwrap_or_else(|| PathBuf::from("."));
        let user_dir = home.join(".aegis").join("skills");
        if user_dir.is_dir() {
            self.load_skills_dir(&user_dir, LoadedFrom::User)
        } else {
            Ok(0)
        }
    }

    /// 加载单个 skills 目录。每个子目录若包含 SKILL.md 即为一个 Skill。
    /// loadSkillsFromSkillsDir(): 只支持目录格式，不支持单 .md 文件。
    fn load_skills_dir(&mut self, dir: &Path, source: LoadedFrom) -> Result<usize, std::io::Error> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(0),
        };

        let mut count = 0;
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue; // 跳过非目录
            }

            // 检查 SKILL.md 是否存在
            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }

            match Skill::from_skill_md(&path) {
                Ok(mut skill) => {
                    skill.loaded_from = source;
                    let name = skill.name.clone();
                    let is_conditional = !skill.frontmatter.paths.is_empty();

                    if is_conditional {
                        self.conditional_skills.entry(name.clone()).or_insert(skill);
                    } else {
                        // 先加载者胜 (深层优先)
                        self.skills.entry(name.clone()).or_insert(skill);
                    }
                    count += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to load skill from {}: {}", path.display(), e);
                }
            }
        }

        if count > 0 {
            self.rebuild_cache();
        }
        Ok(count)
    }

    /// 注册内置 Skill。registerBundledSkill()。
    pub fn register_bundled(&mut self, name: &str, description: &str, content: &str, when_to_use: Option<&str>) {
        let skill = Skill::bundled(name, description, content, when_to_use);
        self.skills.entry(name.to_string()).or_insert(skill);
        self.rebuild_cache();
    }

    // ═══════════════════════════════════════════════
    // 条件激活 — activateConditionalSkillsForPaths()
    // ═══════════════════════════════════════════════

    /// 根据文件路径激活条件 Skills。返回新激活的 Skill 名。
    pub fn activate_for_paths(&mut self, file_paths: &[&str]) -> Vec<String> {
        let mut activated = Vec::new();
        let cond_names: Vec<String> = self.conditional_skills.keys().cloned().collect();

        for name in cond_names {
            if let Some(skill) = self.conditional_skills.get(&name) {
                let should_activate = skill.frontmatter.paths.iter().any(|pattern| {
                    file_paths.iter().any(|fp| fp.contains(pattern.as_str()))
                });

                if should_activate {
                    let skill = self.conditional_skills.remove(&name).unwrap();
                    activated.push(skill.name.clone());
                    self.skills.insert(skill.name.clone(), skill);
                }
            }
        }

        if !activated.is_empty() {
            self.rebuild_cache();
        }
        activated
    }

    // ═══════════════════════════════════════════════
    // 查询
    // ═══════════════════════════════════════════════

    pub fn injection_text(&self) -> &str {
        &self.cached_injection
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<(String, String, bool)> {
        self.skills
            .values()
            .map(|s| (s.name.clone(), s.description.clone(), s.is_user_invocable()))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn conditional_count(&self) -> usize {
        self.conditional_skills.len()
    }

    // ═══════════════════════════════════════════════
    // 修改
    // ═══════════════════════════════════════════════

    pub fn register(&mut self, skill: Skill) {
        self.skills.insert(skill.name.clone(), skill);
        self.rebuild_cache();
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let removed =
            self.skills.remove(name).is_some() || self.conditional_skills.remove(name).is_some();
        if removed {
            self.rebuild_cache();
        }
        removed
    }

    fn rebuild_cache(&mut self) {
        self.cached_injection = self
            .skills
            .values()
            .map(|s| s.to_prompt_injection())
            .collect::<Vec<_>>()
            .join("\n");
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 获取用户 home 目录 (跨平台)。
fn dirs_next() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_skill_dir(root: &Path, name: &str, content: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = std::fs::File::create(dir.join("SKILL.md")).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    // ── Frontmatter 解析 ──

    #[test]
    fn test_parse_frontmatter_all_fields() {
        let content = "---\ndescription: \"Test skill\"\nwhen_to_use: \"When testing\"\nuser-invocable: true\nallowed-tools: [bash, file_read]\nversion: \"1.0\"\npaths: [src/**/*.rs]\n---\n\n## Body\nContent.";
        let (fm, body) = SkillFrontmatter::parse(content);
        assert_eq!(fm.description.as_deref(), Some("Test skill"));
        assert_eq!(fm.when_to_use.as_deref(), Some("When testing"));
        assert!(fm.user_invocable);
        assert_eq!(fm.allowed_tools, vec!["bash", "file_read"]);
        assert_eq!(fm.paths, vec!["src/**/*.rs"]);
        assert!(body.contains("## Body"));
        assert!(!body.contains("---"));
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let content = "# Just markdown";
        let (fm, body) = SkillFrontmatter::parse(content);
        assert!(fm.description.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_frontmatter_user_invocable_false() {
        let (fm, _) = SkillFrontmatter::parse("---\nuser-invocable: false\n---\n\nBody");
        assert!(!fm.user_invocable);
    }

    #[test]
    fn test_parse_frontmatter_arguments() {
        let (fm, _) = SkillFrontmatter::parse("---\narguments: [file, function]\n---\n\nBody");
        assert_eq!(fm.arguments, vec!["file", "function"]);
    }

    // ── Skill 加载 ──

    #[test]
    fn test_skill_from_skill_md() {
        let root = std::env::temp_dir().join("aegis_skill_md_test");
        let _ = std::fs::remove_dir_all(&root);
        create_skill_dir(
            &root,
            "my-skill",
            "---\ndescription: \"A custom skill\"\n---\n\n## Instructions\nDo the thing.",
        );

        let skill = Skill::from_skill_md(&root.join("my-skill")).unwrap();
        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.description, "A custom skill");
        assert!(skill.has_user_specified_description);
        assert!(skill.content.contains("Do the thing"));
        assert!(skill.skill_root.is_some());
    }

    #[test]
    fn test_skill_injection_includes_base_dir() {
        let root = std::env::temp_dir().join("aegis_skill_md_test2");
        let _ = std::fs::remove_dir_all(&root);
        create_skill_dir(&root, "rust-skill", "# Rust\nWrite safe Rust code.");

        let skill = Skill::from_skill_md(&root.join("rust-skill")).unwrap();
        let injection = skill.to_prompt_injection();
        assert!(injection.contains("Base directory for this skill"));
        assert!(injection.contains("rust-skill"));
        assert!(injection.contains("<skill-pin>"));
    }

    // ── Registry: 目录格式加载 ──

    #[test]
    fn test_load_skills_dir_skips_files() {
        let root = std::env::temp_dir().join("aegis_skill_dir_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // 目录格式: 应被加载
        create_skill_dir(&root, "valid-skill", "# Valid\nContent.");
        // 单文件格式: 应被跳过 (CC 在 /skills/ 不支持单 .md)
        let mut f = std::fs::File::create(root.join("invalid.md")).unwrap();
        f.write_all(b"# Invalid\nNot a skill.").unwrap();

        let mut registry = SkillRegistry::new();
        let count = registry.load_skills_dir(&root, LoadedFrom::Skills).unwrap();
        assert_eq!(count, 1);
        assert!(registry.get("valid-skill").is_some());
        assert!(registry.get("invalid").is_none());
    }

    #[test]
    fn test_load_skills_dir_conditional() {
        let root = std::env::temp_dir().join("aegis_skill_cond_test");
        let _ = std::fs::remove_dir_all(&root);
        create_skill_dir(
            &root,
            "rust-only",
            "---\npaths: [src/]\n---\n\n# Rust\nRust only.",
        );
        create_skill_dir(&root, "always-on", "# Always\nAlways active.");

        let mut registry = SkillRegistry::new();
        registry.load_skills_dir(&root, LoadedFrom::Skills).unwrap();

        assert_eq!(registry.len(), 1); // "always-on" 直接激活
        assert_eq!(registry.conditional_count(), 1); // "rust-only" 条件

        let activated = registry.activate_for_paths(&["src/main.rs"]);
        assert_eq!(activated, vec!["rust-only"]);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_bundled_skill() {
        let mut registry = SkillRegistry::new();
        registry.register_bundled("code-review", "Review code", "# Review\nCheck bugs.", None);

        assert_eq!(registry.len(), 1);
        let skill = registry.get("code-review").unwrap();
        assert_eq!(skill.loaded_from, LoadedFrom::Bundled);
        assert!(skill.skill_root.is_none()); // bundled skills 无文件路径
    }

    #[test]
    fn test_first_loaded_wins() {
        let root = std::env::temp_dir().join("aegis_skill_priority");
        let _ = std::fs::remove_dir_all(&root);

        let deep = root.join("deep").join(".agent").join("skills");
        let shallow = root.join(".agent").join("skills");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(&shallow).unwrap();

        create_skill_dir(&deep, "same-name", "---\ndescription: \"Deep version\"\n---\n\nDeep content.");
        create_skill_dir(&shallow, "same-name", "---\ndescription: \"Shallow version\"\n---\n\nShallow content.");

        let mut registry = SkillRegistry::new();
        // 先加载深层
        registry.load_skills_dir(&deep, LoadedFrom::Skills).unwrap();
        // 后加载浅层 (不应覆盖)
        registry.load_skills_dir(&shallow, LoadedFrom::Skills).unwrap();

        let skill = registry.get("same-name").unwrap();
        assert_eq!(skill.description, "Deep version"); // 深层优先
    }
}
