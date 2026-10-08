use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::de::{self, Deserializer};
use serde::Deserialize;

use crate::ui::layout::Size;

fn size_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Size>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Number(i64),
        Text(String),
    }
    match Raw::deserialize(d)? {
        Raw::Number(n) => Size::parse(&n.to_string()),
        Raw::Text(t) => Size::parse(&t),
    }
    .map(Some)
    .map_err(de::Error::custom)
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ui: UiConfig,
    pub review: ReviewConfig,
    pub diff: DiffConfig,
    pub refresh: RefreshConfig,
    pub triage: TriageSettings,
    pub checkout: CheckoutConfig,
    /// Key overrides, interpreted by the keymap.
    pub keys: BTreeMap<String, String>,
    #[serde(rename = "source")]
    pub sources: Vec<SourceConfig>,
}

macro_rules! choice {
    ($(#[$m:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }
    };
}

choice!(Layout { Panes = "panes", Split = "split", Queue = "queue" });
choice!(SourcesLayout { Auto = "auto", Left = "left", Top = "top" });
choice!(DetailMode { Auto = "auto", Open = "open", Closed = "closed" });
choice!(DetailPosition { Auto = "auto", Right = "right", Left = "left", Top = "top", Bottom = "bottom" });
choice!(ColourDepth { Auto = "auto", Truecolor = "truecolor", Colour256 = "256", Colour16 = "16" });
choice!(Images { Auto = "auto", Off = "off", Halfblocks = "halfblocks", ForgeOnly = "forge-only" });
choice!(Background { Theme = "theme", Yes = "yes", No = "no" });
choice!(MergeMethod { Merge = "merge", Squash = "squash", Rebase = "rebase" });
choice!(DiffView { Unified = "unified", SideBySide = "side-by-side" });
choice!(CloneIfMissing { Ask = "ask", Always = "always", Never = "never" });
choice!(DraftStorage { Local = "local", Off = "off" });
choice!(ShowFilter { Reviewing = "reviewing", Assigned = "assigned", Authored = "authored", Drafts = "drafts", Noise = "noise" });

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub theme: String,
    pub layout: Layout,
    pub sources: SourcesLayout,
    pub detail: DetailMode,
    pub detail_position: DetailPosition,
    /// Starting Queue width beside Detail: columns, or a percentage like `60%`.
    #[serde(deserialize_with = "size_option")]
    pub queue_width: Option<Size>,
    /// Starting Queue height when stacked above or below Detail.
    #[serde(deserialize_with = "size_option")]
    pub queue_height: Option<Size>,
    pub jax: bool,
    pub reduced_motion: bool,
    /// Pictures in descriptions: how they are drawn and where they may come from.
    pub images: Images,
    pub unicode: bool,
    pub colour_depth: ColourDepth,
    pub background: Background,
    /// Per-theme `background` settings, by theme id; each beats the global one.
    pub theme_background: BTreeMap<String, Background>,
    pub mouse: bool,
    /// Remember the panel layout between runs in the state directory's `session.toml`.
    pub remember_layout: bool,
    /// Where unsent review comments are kept: `local` saves them to the state directory,
    /// `off` keeps them in memory only.
    pub drafts: DraftStorage,
    pub date_locale: String,
    /// The terminal pane (`t`).
    pub terminal: TerminalConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "liminal-hq".into(),
            layout: Layout::Panes,
            sources: SourcesLayout::Auto,
            detail: DetailMode::Auto,
            detail_position: DetailPosition::Auto,
            queue_width: None,
            queue_height: None,
            jax: true,
            reduced_motion: false,
            images: Images::Auto,
            unicode: true,
            colour_depth: ColourDepth::Auto,
            background: Background::Theme,
            theme_background: BTreeMap::new(),
            mouse: true,
            remember_layout: true,
            drafts: DraftStorage::Local,
            date_locale: "en-CA".into(),
            terminal: TerminalConfig::default(),
        }
    }
}

choice!(TerminalStart { Ask = "ask", Worktree = "worktree", Current = "current" });

/// `[ui.terminal]`: the terminal pane next to the change.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// The program and its arguments. Empty runs `$SHELL` (`%COMSPEC%` on Windows).
    pub command: Vec<String>,
    /// The chord that starts leaving the pane, for example `ctrl-\` or `ctrl-]`.
    pub escape: String,
    /// Where the pane sits: `auto`, `right`, `left`, `top` or `bottom`.
    pub position: DetailPosition,
    /// Starting size of the pane: columns or rows, or a percentage like `40%`.
    #[serde(deserialize_with = "size_option")]
    pub size: Option<Size>,
    /// Lines of history kept for scrolling back.
    pub scrollback: usize,
    /// Where to start: `ask` offers a managed worktree or the current directory, `worktree` offers
    /// only the worktree, `current` starts in the current directory without asking.
    pub start: TerminalStart,
    /// Local clones to make worktrees from, by `owner/name`. The directory Review Buddy was
    /// started in is tried first.
    pub checkouts: BTreeMap<String, String>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            command: Vec::new(),
            escape: "ctrl-\\".into(),
            position: DetailPosition::Auto,
            size: None,
            scrollback: 10_000,
            start: TerminalStart::Ask,
            checkouts: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewConfig {
    pub merge_method: MergeMethod,
    pub confirm_merge: bool,
    pub confirm_post_now: bool,
    pub delete_branch_on_merge: bool,
    pub mark_viewed_on_open: bool,
    pub request_changes_needs_summary: bool,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            merge_method: MergeMethod::Squash,
            confirm_merge: true,
            confirm_post_now: true,
            delete_branch_on_merge: true,
            mark_viewed_on_open: true,
            request_changes_needs_summary: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiffConfig {
    pub view: DiffView,
    pub auto_side_by_side: bool,
    pub side_by_side_min_cols: u16,
    pub context_lines: u16,
    pub ignore_whitespace: bool,
    pub syntax_highlight: bool,
    pub tab_width: u8,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            view: DiffView::Unified,
            auto_side_by_side: true,
            side_by_side_min_cols: 160,
            context_lines: 3,
            ignore_whitespace: false,
            syntax_highlight: true,
            tab_width: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RefreshConfig {
    /// `None` means `"off"`: manual refresh only.
    #[serde(deserialize_with = "optional_duration")]
    pub interval: Option<Duration>,
    pub on_focus: bool,
    pub max_concurrency_per_host: u8,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            interval: Some(Duration::from_secs(300)),
            on_focus: true,
            max_concurrency_per_host: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TriageSettings {
    pub noise_authors: Vec<String>,
    pub bucket_limit: u32,
    pub show: Vec<ShowFilter>,
    pub noise_collapsed: bool,
    #[serde(deserialize_with = "duration")]
    pub stale_after: Duration,
    /// Raw `[[triage.rule]]` tables, interpreted by the triage rules engine.
    pub rule: Vec<toml::Table>,
}

impl Default for TriageSettings {
    fn default() -> Self {
        let core = rb_core::TriageConfig::default();
        Self {
            noise_authors: core.noise_authors,
            bucket_limit: 20,
            show: vec![
                ShowFilter::Reviewing,
                ShowFilter::Assigned,
                ShowFilter::Authored,
            ],
            noise_collapsed: true,
            stale_after: core.stale_after,
            rule: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CheckoutConfig {
    pub root: String,
    pub use_worktree_if_dirty: bool,
    pub clone_if_missing: CloneIfMissing,
}

impl Default for CheckoutConfig {
    fn default() -> Self {
        Self {
            root: "~/src/{repo}".into(),
            use_worktree_if_dirty: true,
            clone_if_missing: CloneIfMissing::Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Github,
    Gitlab,
}

/// How a source gets its token. Only the method is stored, never a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthSetting {
    Cli,
    Token,
    Env(String),
    Command,
}

impl<'de> Deserialize<'de> for AuthSetting {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        match text.as_str() {
            "cli" => Ok(Self::Cli),
            "token" => Ok(Self::Token),
            "command" => Ok(Self::Command),
            other => match other.strip_prefix("env:") {
                Some(var) if !var.is_empty() => Ok(Self::Env(var.to_string())),
                _ => Err(de::Error::custom(format!(
                    "unknown auth `{other}`, expected `cli`, `token`, `command` or `env:VAR_NAME`"
                ))),
            },
        }
    }
}

impl fmt::Display for AuthSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli => f.write_str("cli"),
            Self::Token => f.write_str("token"),
            Self::Command => f.write_str("command"),
            Self::Env(v) => write!(f, "env:{v}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScopeConfig {
    pub orgs: Vec<String>,
    pub repos: Vec<String>,
    pub user: bool,
    pub groups: Vec<String>,
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub name: String,
    pub kind: Kind,
    pub host: String,
    #[serde(default)]
    pub api_url: Option<String>,
    /// `None` means "use `cli` if signed in, else `token`", decided at run time.
    #[serde(default)]
    pub auth: Option<AuthSetting>,
    #[serde(default)]
    pub token_command: Option<String>,
    #[serde(default)]
    pub scope: ScopeConfig,
    #[serde(default = "yes")]
    pub in_all: bool,
    #[serde(default)]
    pub include_drafts: bool,
    #[serde(default)]
    pub tag_colour: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Projects (`owner/repo` or a GitLab path, optionally ending in `*`) the queue leaves out.
    #[serde(default)]
    pub hide_repos: Vec<String>,
}

fn yes() -> bool {
    true
}

/// Parses `"90s"`, `"5m"`, `"2h"`, `"14d"` or `"2w"`.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let bad =
        || format!("`{text}` isn't a duration, try something like \"5m\", \"12h\" or \"14d\"");
    let split = text.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?;
    let (digits, unit) = text.split_at(split);
    let n: u64 = digits.parse().map_err(|_| bad())?;
    let unit_secs = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return Err(bad()),
    };
    n.checked_mul(unit_secs)
        .map(Duration::from_secs)
        .ok_or_else(bad)
}

fn duration<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    parse_duration(&String::deserialize(d)?).map_err(de::Error::custom)
}

fn optional_duration<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
    let text = String::deserialize(d)?;
    if matches!(text.trim(), "off" | "0") {
        return Ok(None);
    }
    match parse_duration(&text) {
        Ok(d) if d.is_zero() => Ok(None),
        Ok(d) => Ok(Some(d)),
        Err(e) => Err(de::Error::custom(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_with_units() {
        assert_eq!(parse_duration("5m"), Ok(Duration::from_secs(300)));
        assert_eq!(parse_duration("14d"), Ok(Duration::from_secs(14 * 86_400)));
        assert_eq!(parse_duration("2w"), Ok(Duration::from_secs(1_209_600)));
        assert!(parse_duration("5").is_err());
        assert!(parse_duration("m").is_err());
        assert!(parse_duration("5y").is_err());
    }

    #[test]
    fn the_refresh_interval_can_be_switched_off() {
        let parse = |text: &str| {
            toml::from_str::<RefreshConfig>(&format!("interval = \"{text}\"")).map(|c| c.interval)
        };
        assert_eq!(parse("2m"), Ok(Some(Duration::from_secs(120))));
        assert_eq!(parse("90s"), Ok(Some(Duration::from_secs(90))));
        assert_eq!(parse("off"), Ok(None));
        assert_eq!(parse("0"), Ok(None));
        assert_eq!(parse("0s"), Ok(None));
        assert!(parse("soon").is_err());
    }

    #[test]
    fn defaults_match_the_spec() {
        let c = Config::default();
        assert_eq!(c.ui.theme, "liminal-hq");
        assert_eq!(c.diff.side_by_side_min_cols, 160);
        assert_eq!(c.refresh.interval, Some(Duration::from_secs(300)));
        assert_eq!(c.triage.bucket_limit, 20);
        assert_eq!(c.checkout.root, "~/src/{repo}");
        assert!(c.sources.is_empty());
    }

    #[test]
    fn empty_document_equals_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c, Config::default());
    }
}
