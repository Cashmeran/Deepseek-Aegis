//! CLI argument parsing and configuration loading.

use clap::{Parser, Subcommand};
use serde::Deserialize;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "aegis", version = VERSION, about = "AI coding agent · DeepSeek V4")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Model to use (deepseek-v4-pro, deepseek-v4-flash)
    #[arg(short, long, default_value = "deepseek-v4-pro")]
    pub model: String,

    /// Reasoning effort (max, high, off)
    #[arg(short, long, default_value = "max")]
    pub effort: String,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start interactive chat (default)
    Chat,
    /// Show configuration path
    Config,
}

/// Agent configuration from file or environment.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AegisConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_effort")]
    pub effort: String,
    #[serde(default)]
    pub acp_port: u16,
}

fn default_model() -> String { "deepseek-v4-pro".into() }
fn default_effort() -> String { "max".into() }

/// Load config from `~/.aegis/config.toml` (if exists), fall back to env vars.
pub fn load_config() -> AegisConfig {
    let mut config = AegisConfig::default();

    // 1. Try ~/.aegis/config.toml
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".aegis").join("config.toml");
        if path.exists()
            && let Ok(content) = std::fs::read_to_string(&path)
                && let Ok(c) = toml::de::from_str::<AegisConfig>(&content) {
                    config = c;
                }
    }

    // 2. Env vars override file
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY")
        && !key.is_empty() { config.api_key = key; }
    if let Ok(model) = std::env::var("DEEPSEEK_MODEL")
        && !model.is_empty() { config.model = model; }
    if let Ok(effort) = std::env::var("DEEPSEEK_EFFORT")
        && !effort.is_empty() { config.effort = effort; }
    if let Ok(port) = std::env::var("AEGIS_ACP_PORT")
        && let Ok(p) = port.parse() { config.acp_port = p; }

    // Fallback: if nothing set model/effort, use defaults
    if config.model.is_empty() { config.model = default_model(); }
    if config.effort.is_empty() { config.effort = default_effort(); }

    config
}

/// Ensure config directory and file exist.
pub fn ensure_config_dir() -> std::path::PathBuf {
    let home = dirs::home_dir().expect("HOME not set");
    let dir = home.join(".aegis");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
