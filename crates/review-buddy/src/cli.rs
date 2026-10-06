// Command-line definitions for the `review-buddy` binary.
//
// Shared by `main.rs` (via `mod cli`) and `build.rs` (via `include!`) so the
// man page is generated from the same `Cli` type used for parsing.

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Every pull and merge request, in one quiet queue.
#[derive(Parser, Debug)]
#[command(
    name = "review-buddy",
    version,
    about = "Every pull and merge request, in one quiet queue."
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    /// Demo scene to open (requires --demo)
    #[arg(long, value_name = "NAME", requires = "demo")]
    pub demo_scene: Option<String>,

    /// Pin Jax's mood (requires --demo)
    #[arg(long, value_name = "MOOD", requires = "demo")]
    pub jax_mood: Option<String>,

    /// Render at a fixed size, e.g. 160x40 (requires --demo)
    #[arg(long, value_name = "COLSxROWS", requires = "demo")]
    pub size: Option<String>,

    /// Connect your accounts and write a config (run again any time)
    #[arg(long)]
    pub setup: bool,

    /// Run --setup as plain prompts instead of the full-screen interface
    #[arg(long, alias = "no-tui", requires = "setup")]
    pub plain: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Flags every command accepts, before or after the command name.
#[derive(Args, Debug, Clone, Default)]
pub struct GlobalArgs {
    /// Use one config file instead of the XDG user config
    #[arg(long, value_name = "PATH", global = true)]
    pub config: Option<std::path::PathBuf>,

    /// Limit to one configured source (repeatable)
    #[arg(
        short = 's',
        long = "source",
        value_name = "NAME",
        env = "REVIEW_BUDDY_SOURCE",
        value_delimiter = ',',
        global = true
    )]
    pub sources: Vec<String>,

    /// Limit to one repository, as [host/]owner/repo
    #[arg(
        short = 'R',
        long,
        value_name = "[HOST/]OWNER/REPO",
        env = "REVIEW_BUDDY_REPO",
        global = true
    )]
    pub repo: Option<String>,

    /// Print JSON with the named fields; with no fields, list the available ones
    #[arg(
        long,
        value_name = "FIELDS",
        value_delimiter = ',',
        num_args = 0..=1,
        default_missing_value = "",
        global = true
    )]
    pub json: Option<Vec<String>>,

    /// Filter the JSON with a jq expression
    #[arg(short = 'q', long, value_name = "EXPR", global = true)]
    pub jq: Option<String>,

    /// Open the result in the browser instead of printing it
    #[arg(short = 'w', long, global = true)]
    pub web: bool,

    /// When to use colour
    #[arg(long, value_enum, value_name = "WHEN", global = true)]
    pub color: Option<ColorWhen>,

    /// Shorthand for --color never
    #[arg(long, conflicts_with = "color", global = true)]
    pub no_color: bool,

    /// Run against offline fixtures; nothing is sent
    #[arg(long, global = true)]
    pub demo: bool,

    /// Freeze the clock at an ISO 8601 time (requires --demo)
    #[arg(long, value_name = "ISO", requires = "demo", global = true)]
    pub frozen_time: Option<String>,

    /// Answer yes to a confirmation; required for writes without a terminal
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketArg {
    Wait,
    Look,
    Later,
    Noise,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateArg {
    Open,
    Closed,
    Merged,
    All,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Jump straight to one change's diff
    Open {
        /// Pull or merge request URL, reference, number or branch
        selector: Option<String>,
    },
    /// Your triaged queue, by bucket
    Queue(QueueArgs),
    /// Work with pull and merge requests
    #[command(alias = "mr")]
    Pr {
        #[command(subcommand)]
        action: PrAction,
    },
    /// Check who you are signed in as
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Inspect configured sources
    Source {
        #[command(subcommand)]
        action: SourceAction,
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
    /// Print a shell completion script
    Completion {
        #[arg(value_enum)]
        shell: ShellArg,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellArg {
    Bash,
    Zsh,
    Fish,
    Elvish,
    Powershell,
}

#[derive(Args, Debug, Clone, Default)]
pub struct QueueArgs {
    /// Only these buckets (repeatable)
    #[arg(long, value_enum)]
    pub bucket: Vec<BucketArg>,
    /// Which relationships to show, e.g. reviewing,assigned,authored,drafts,noise
    #[arg(long, value_delimiter = ',', value_name = "LIST")]
    pub show: Vec<String>,
    /// Ignore the bucket limit
    #[arg(long)]
    pub all: bool,
}

#[derive(Subcommand, Debug)]
pub enum PrAction {
    /// List changes with filters
    List(PrListArgs),
    /// Title, state, reviewers, checks and description
    View {
        selector: Option<String>,
        /// Append the conversation
        #[arg(long)]
        comments: bool,
    },
    /// The patch
    Diff {
        selector: Option<String>,
        /// Print only the changed file names
        #[arg(long)]
        name_only: bool,
        /// Print a diffstat
        #[arg(long)]
        stat: bool,
        /// Limit to these paths (repeatable)
        #[arg(long, value_name = "PATH")]
        file: Vec<String>,
        /// Force the raw patch on a terminal
        #[arg(long)]
        patch: bool,
    },
    /// Check runs or pipeline jobs
    Checks {
        selector: Option<String>,
        /// Refresh until everything settles
        #[arg(long)]
        watch: bool,
        /// Seconds between refreshes
        #[arg(long, value_name = "SECS", default_value_t = 10)]
        interval: u64,
        /// Exit on the first failure
        #[arg(long)]
        fail_fast: bool,
        /// Only required checks
        #[arg(long)]
        required: bool,
    },
    /// Open in the browser
    Open { selector: Option<String> },
}

#[derive(Args, Debug, Clone)]
pub struct PrListArgs {
    /// Which changes to list
    #[arg(long, value_enum, default_value = "open")]
    pub state: StateArg,
    #[arg(long)]
    pub author: Option<String>,
    #[arg(long)]
    pub assignee: Option<String>,
    #[arg(long)]
    pub reviewer: Option<String>,
    /// Only changes with this label (repeatable)
    #[arg(long)]
    pub label: Vec<String>,
    /// Only drafts
    #[arg(long)]
    pub draft: bool,
    /// Search query, passed to the forge where supported
    #[arg(long, value_name = "QUERY")]
    pub search: Option<String>,
    /// Maximum number of changes
    #[arg(short = 'L', long, default_value_t = 30)]
    pub limit: u32,
}

#[derive(Subcommand, Debug)]
pub enum AuthAction {
    /// Who you are on every host, and how you're signed in
    Status,
}

#[derive(Subcommand, Debug)]
pub enum SourceAction {
    /// Configured sources, enabled state and auth method
    List,
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
    /// One resolved value and where it came from
    Get { key: String },
    /// Every resolved value with its origin
    List,
}

#[derive(Subcommand, Debug)]
pub enum TriageAction {
    /// Show which rule or built-in bucketed a change
    Explain { selector: String },
}
