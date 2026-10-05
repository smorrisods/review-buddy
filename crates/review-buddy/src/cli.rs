// Command-line definitions for the `review-buddy` binary.
//
// Shared by `main.rs` (via `mod cli`) and `build.rs` (via `include!`) so the
// man page is generated from the same `Cli` type used for parsing.

use clap::{Parser, Subcommand};

/// Every pull and merge request, in one quiet queue.
#[derive(Parser, Debug)]
#[command(
    name = "review-buddy",
    version,
    about = "Every pull and merge request, in one quiet queue."
)]
pub struct Cli {
    /// Use one config file instead of the XDG user config
    #[arg(long, value_name = "PATH")]
    pub config: Option<std::path::PathBuf>,

    /// Run against offline fixtures; nothing is sent
    #[arg(long)]
    pub demo: bool,

    /// Demo scene to open (requires --demo)
    #[arg(long, value_name = "NAME", requires = "demo")]
    pub demo_scene: Option<String>,

    /// Freeze the clock at an ISO 8601 time (requires --demo)
    #[arg(long, value_name = "ISO", requires = "demo")]
    pub frozen_time: Option<String>,

    /// Pin Jax's mood (requires --demo)
    #[arg(long, value_name = "MOOD", requires = "demo")]
    pub jax_mood: Option<String>,

    /// Render at a fixed size, e.g. 160x40 (requires --demo)
    #[arg(long, value_name = "COLSxROWS", requires = "demo")]
    pub size: Option<String>,

    /// Re-run first run
    #[arg(long)]
    pub setup: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Jump straight to one change's diff
    Open {
        /// Pull or merge request URL
        url: String,
    },
    /// Inspect themes
    Theme {
        #[command(subcommand)]
        action: ThemeAction,
    },
    /// Check auth, scopes, rate limits, API versions and XDG paths
    Doctor,
    /// Inspect configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Explain triage decisions
    Triage {
        #[command(subcommand)]
        action: TriageAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ThemeAction {
    /// List available themes
    List,
    /// Check a theme's contrast and roles
    Check { id: String },
    /// Print a theme as TOML
    Export { id: String },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print every resolved directory and which config files were loaded
    Paths,
}

#[derive(Subcommand, Debug)]
pub enum TriageAction {
    /// Show which rule or built-in bucketed a change
    Explain { url: String },
}
