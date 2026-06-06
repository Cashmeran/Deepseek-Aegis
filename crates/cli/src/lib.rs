// aegis-cli — terminal UI and app layer

pub mod bridge;
pub mod app;
pub mod error;
pub mod logging;
pub mod perf;
pub mod ui;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum DiagnosticsPreset {
    Runtime,
    Session,
    Render,
    Bridge,
    Full,
}

impl DiagnosticsPreset {
    #[must_use]
    pub fn filter_directives(&self) -> &'static str {
        match self {
            Self::Runtime => "info,bridge.lifecycle=debug,bridge.protocol=debug,app.session=debug,app.tool=debug,app.command=debug,app.permission=debug,app.network=debug,app.update=debug",
            Self::Session => "info,bridge.lifecycle=debug,bridge.protocol=debug,app.session=debug,app.permission=debug,app.command=debug",
            Self::Render => "info,app.render=trace,app.cache=debug,app.input=debug,app.paste=debug,app.perf=info",
            Self::Bridge => "info,bridge.lifecycle=debug,bridge.protocol=debug,bridge.sdk=debug,bridge.permission=debug,bridge.mcp=debug",
            Self::Full => "info,app.render=trace,app.perf=info,bridge.lifecycle=debug,bridge.protocol=debug,bridge.sdk=debug,bridge.permission=debug,bridge.mcp=debug,app.session=debug,app.tool=debug,app.command=debug,app.permission=debug,app.network=debug,app.update=debug,app.cache=debug,app.input=debug,app.paste=debug,app.config=debug,app.auth=debug",
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "aegis", version = "0.1.0", about = "Aegis coding agent")]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(long)]
    pub no_update_check: bool,

    #[arg(long, short = 'C')]
    pub dir: Option<std::path::PathBuf>,

    #[arg(long)]
    pub bridge_script: Option<std::path::PathBuf>,

    #[arg(long)]
    pub enable_logs: bool,

    #[arg(long, value_enum)]
    pub diagnostics_preset: Option<DiagnosticsPreset>,

    #[arg(long, value_name = "PATH")]
    pub log_file: Option<std::path::PathBuf>,

    #[arg(long, value_name = "FILTER")]
    pub log_filter: Option<String>,

    #[arg(long)]
    pub log_append: bool,

    #[arg(long)]
    pub enable_perf: bool,

    #[arg(long, value_name = "PATH")]
    pub perf_log: Option<std::path::PathBuf>,

    #[arg(long)]
    pub perf_append: bool,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Command {
    Resume { session_id: Option<String> },
}
