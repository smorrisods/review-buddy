//! The first-run flow as a state machine with pure transitions.
//!
//! [`Flow::apply`] takes one [`Input`] and returns the [`Effect`]s to run. It does no I/O: the
//! full-screen first run and the plain prompts both drive it, and feed effect results back in as
//! more inputs.

use std::fmt;
use std::path::PathBuf;

use rb_core::ForgeKind;
use rb_platform::auth::CliTool;
use rb_platform::Secret;
use rb_theme::BUILTIN_IDS;

use super::detect::{Detection, FoundHost};
use super::source::{AuthKind, SourceSpec};

/// What each forge's token needs, shown beside the token field.
pub fn required_scopes(kind: ForgeKind) -> &'static [&'static str] {
    match kind {
        ForgeKind::GitHub => &["repo", "read:org"],
        ForgeKind::GitLab => &["api", "read_user"],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    #[default]
    Welcome,
    Connect,
    Scope,
    Look,
    Jax,
    Summary,
    Done,
}

impl Step {
    pub fn previous(self) -> Option<Self> {
        match self {
            Self::Welcome | Self::Done => None,
            Self::Connect => Some(Self::Welcome),
            Self::Scope => Some(Self::Connect),
            Self::Look => Some(Self::Scope),
            Self::Jax => Some(Self::Look),
            Self::Summary => Some(Self::Jax),
        }
    }
}

/// A token being typed. It never shows in `Debug` output and is cleared once it is sent.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct TokenInput(String);

impl TokenInput {
    pub fn len(&self) -> usize {
        self.0.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn push_str(&mut self, text: &str) {
        self.0.extend(text.chars().filter(|c| !c.is_control()));
    }

    fn pop(&mut self) {
        self.0.pop();
    }

    fn take(&mut self) -> Secret {
        Secret::new(std::mem::take(&mut self.0).trim())
    }
}

impl fmt::Debug for TokenInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TokenInput(<{} chars>)", self.len())
    }
}

/// What a forge said about a token or sign-in. No secrets in here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProbeInfo {
    pub login: String,
    pub scopes: Vec<String>,
    pub orgs: Vec<String>,
    /// Calm one-liners: a missing scope, an SSO step, or a check that isn't built yet.
    pub hints: Vec<String>,
    /// `false` when the forge can't be asked yet, so the sign-in is taken on trust.
    pub checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conn {
    /// Nothing to reuse: a token is needed.
    NeedsToken,
    Checking,
    Connected(ProbeInfo),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRow {
    pub host: String,
    pub kind: ForgeKind,
    pub found: String,
    pub cli: Option<(CliTool, String)>,
    pub selected: bool,
    pub conn: Conn,
    pub auth: AuthKind,
    pub user: bool,
    /// Organisations or groups, each with whether it is included.
    pub orgs: Vec<(String, bool)>,
}

impl HostRow {
    fn from_found(found: &FoundHost) -> Self {
        let cli = found.cli().map(|(t, u)| (t, u.to_string()));
        Self {
            host: found.host.clone(),
            kind: found.kind,
            found: found.found_in(),
            selected: cli.is_some(),
            conn: if cli.is_some() {
                Conn::Checking
            } else {
                Conn::NeedsToken
            },
            cli,
            auth: AuthKind::Cli,
            user: false,
            orgs: Vec::new(),
        }
    }

    pub fn connected(&self) -> Option<&ProbeInfo> {
        match &self.conn {
            Conn::Connected(info) => Some(info),
            _ => None,
        }
    }

    pub fn needs_token(&self) -> bool {
        matches!(self.conn, Conn::NeedsToken | Conn::Failed(_))
    }

    /// `✓ GH github.com  signed in as smorris via gh · 2 orgs`, without the host: a glyph and
    /// the words that follow it.
    pub fn status(&self) -> (char, String) {
        match &self.conn {
            Conn::Checking => ('…', "checking…".to_string()),
            Conn::NeedsToken => (
                '○',
                format!("{}, no credentials yet › add a token", self.found),
            ),
            Conn::Failed(reason) => ('○', reason.clone()),
            Conn::Connected(info) => {
                let via = self
                    .cli
                    .as_ref()
                    .map_or("your keyring", |(tool, _)| super::detect::tool_name(*tool));
                let mut text = match self.account() {
                    Some(who) => format!("signed in as {who} via {via}"),
                    None => format!("token saved in {via}"),
                };
                match info.orgs.len() {
                    0 => {}
                    1 => text.push_str(" · 1 org"),
                    n => text.push_str(&format!(" · {n} orgs")),
                }
                if !info.checked {
                    text.push_str(" · not checked yet");
                }
                ('✓', text)
            }
        }
    }

    /// The account to show for a connected row.
    pub fn account(&self) -> Option<String> {
        let info = self.connected()?;
        if !info.login.is_empty() {
            return Some(info.login.clone());
        }
        self.cli.as_ref().map(|(_, user)| user.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// The token `gh` or `glab` already holds.
    Cli,
    /// A token the person just typed. The effect tests it, then keeps it in the OS keyring.
    Pasted(Secret),
}

/// The whole choice, ready to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub target: PathBuf,
    pub theme: String,
    pub jax: bool,
    pub sources: Vec<SourceSpec>,
    /// The person said yes to replacing the file that is already there.
    pub replace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Detect,
    Probe {
        host: String,
        kind: ForgeKind,
        credential: Credential,
    },
    Write(Plan),
}

/// A pointer press on part of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Move to this row (and toggle it).
    Row(usize),
    Next,
    Back,
    Skip,
    Theme(usize),
    Jax,
    Yes,
    No,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Detected(Detection),
    Probed {
        host: String,
        result: Result<ProbeInfo, String>,
    },
    Written(Result<Written, String>),
    Next,
    Back,
    Skip,
    Up,
    Down,
    Left,
    Right,
    Toggle,
    Char(char),
    Backspace,
    Paste(String),
    /// Answer to "replace the existing file?".
    Confirm(bool),
    Click(Click),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Written {
        path: PathBuf,
        backup: Option<PathBuf>,
        sources: Vec<String>,
    },
    Skipped,
}

/// One item on the scope step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeItem {
    User(usize),
    Org(usize, usize),
}

#[derive(Debug, Clone)]
pub struct Flow {
    pub step: Step,
    pub hosts: Vec<HostRow>,
    pub notes: Vec<String>,
    pub detecting: bool,
    pub cursor: usize,
    /// The token field is open for `hosts[cursor]`.
    pub token_open: bool,
    pub token: TokenInput,
    pub theme: usize,
    pub jax: bool,
    pub target: PathBuf,
    /// A config file is already at `target`.
    pub existing: bool,
    pub asking_replace: bool,
    pub writing: bool,
    pub message: Option<String>,
    pub outcome: Option<Outcome>,
    api_urls: Vec<(String, String)>,
}

impl Flow {
    pub fn new(target: PathBuf, existing: bool, theme: &str, jax: bool) -> Self {
        Self {
            step: Step::Welcome,
            hosts: Vec::new(),
            notes: Vec::new(),
            detecting: true,
            cursor: 0,
            token_open: false,
            token: TokenInput::default(),
            theme: BUILTIN_IDS.iter().position(|id| *id == theme).unwrap_or(0),
            jax,
            target,
            existing,
            asking_replace: false,
            writing: false,
            message: None,
            outcome: None,
            api_urls: Vec::new(),
        }
    }

    /// Keeps the `api_url` already set for a host, so a re-run doesn't lose it.
    pub fn with_api_urls(mut self, urls: Vec<(String, String)>) -> Self {
        self.api_urls = urls;
        self
    }

    /// The effects that get things going.
    pub fn start(&self) -> Vec<Effect> {
        vec![Effect::Detect]
    }

    pub fn theme_id(&self) -> &'static str {
        BUILTIN_IDS[self.theme]
    }

    pub fn is_done(&self) -> bool {
        self.step == Step::Done
    }

    pub fn row(&self, host: &str) -> Option<&HostRow> {
        self.hosts.iter().find(|h| h.host == host)
    }

    /// Selected hosts that are connected, in order.
    pub fn chosen(&self) -> impl Iterator<Item = (usize, &HostRow)> {
        self.hosts
            .iter()
            .enumerate()
            .filter(|(_, h)| h.selected && h.connected().is_some())
    }

    /// What the scope step lists: each chosen host's account, then its organisations.
    pub fn scope_items(&self) -> Vec<ScopeItem> {
        let mut items = Vec::new();
        for (i, host) in self.chosen() {
            if host.kind == ForgeKind::GitHub {
                items.push(ScopeItem::User(i));
            }
            items.extend((0..host.orgs.len()).map(|o| ScopeItem::Org(i, o)));
        }
        items
    }

    pub fn plan(&self, replace: bool) -> Plan {
        let sources = self
            .chosen()
            .map(|(_, h)| SourceSpec {
                name: h.host.clone(),
                kind: h.kind,
                host: h.host.clone(),
                api_url: self
                    .api_urls
                    .iter()
                    .find(|(host, _)| *host == h.host)
                    .map(|(_, url)| url.clone()),
                auth: h.auth.clone(),
                scope_user: h.user,
                owners: h
                    .orgs
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(name, _)| name.clone())
                    .collect(),
            })
            .collect();
        Plan {
            target: self.target.clone(),
            theme: self.theme_id().to_string(),
            jax: self.jax,
            sources,
            replace,
        }
    }

    pub fn apply(&mut self, input: Input) -> Vec<Effect> {
        self.message = None;
        match input {
            Input::Detected(detection) => self.on_detected(detection),
            Input::Probed { host, result } => self.on_probed(&host, result),
            Input::Written(result) => self.on_written(result),
            Input::Click(click) => self.on_click(click),
            Input::Skip => self.skip(),
            Input::Back => self.back(),
            Input::Next => self.next(),
            Input::Up => self.shift(-1),
            Input::Down => self.shift(1),
            Input::Left => self.look(-1),
            Input::Right => self.look(1),
            Input::Toggle => self.toggle(),
            Input::Char(c) => self.typed(c),
            Input::Backspace => {
                self.token.pop();
                Vec::new()
            }
            Input::Paste(text) => {
                if self.token_open {
                    self.token.push_str(&text);
                }
                Vec::new()
            }
            Input::Confirm(yes) => self.confirm(yes),
        }
    }

    fn on_detected(&mut self, detection: Detection) -> Vec<Effect> {
        self.detecting = false;
        self.notes = detection.notes;
        self.hosts = detection.hosts.iter().map(HostRow::from_found).collect();
        self.cursor = 0;
        self.hosts
            .iter()
            .filter(|h| h.cli.is_some())
            .map(|h| Effect::Probe {
                host: h.host.clone(),
                kind: h.kind,
                credential: Credential::Cli,
            })
            .collect()
    }

    fn on_probed(&mut self, host: &str, result: Result<ProbeInfo, String>) -> Vec<Effect> {
        let field = self.token_open_for(host);
        let Some(row) = self.hosts.iter_mut().find(|h| h.host == host) else {
            return Vec::new();
        };
        match result {
            Ok(info) => {
                row.orgs = info.orgs.iter().map(|o| (o.clone(), false)).collect();
                row.selected = true;
                row.conn = Conn::Connected(info);
                if field {
                    self.token_open = false;
                }
            }
            Err(reason) => {
                row.conn = Conn::Failed(reason);
                row.selected = field;
            }
        }
        Vec::new()
    }

    fn token_open_for(&self, host: &str) -> bool {
        self.token_open && self.hosts.get(self.cursor).is_some_and(|h| h.host == host)
    }

    fn on_written(&mut self, result: Result<Written, String>) -> Vec<Effect> {
        self.writing = false;
        match result {
            Ok(written) => {
                self.outcome = Some(Outcome::Written {
                    path: written.path,
                    backup: written.backup,
                    sources: self.chosen().map(|(_, h)| h.host.clone()).collect(),
                });
                self.step = Step::Done;
            }
            Err(reason) => self.message = Some(reason),
        }
        Vec::new()
    }

    fn skip(&mut self) -> Vec<Effect> {
        if self.token_open {
            self.close_token();
            return Vec::new();
        }
        if self.asking_replace {
            self.asking_replace = false;
            return Vec::new();
        }
        self.outcome = Some(Outcome::Skipped);
        self.step = Step::Done;
        Vec::new()
    }

    fn back(&mut self) -> Vec<Effect> {
        if self.token_open {
            self.close_token();
            return Vec::new();
        }
        self.asking_replace = false;
        if let Some(prev) = self.step.previous() {
            self.step = prev;
            self.cursor = 0;
        }
        Vec::new()
    }

    /// Closing the field means "not this one": the row goes back to unselected.
    fn close_token(&mut self) {
        self.token_open = false;
        self.token = TokenInput::default();
        if let Some(row) = self.hosts.get_mut(self.cursor) {
            if row.needs_token() {
                row.selected = false;
            }
        }
    }

    fn next(&mut self) -> Vec<Effect> {
        match self.step {
            Step::Welcome => {
                self.step = Step::Connect;
                Vec::new()
            }
            Step::Connect => self.next_from_connect(),
            Step::Scope => self.go(Step::Look),
            Step::Look => self.go(Step::Jax),
            Step::Jax => self.go(Step::Summary),
            Step::Summary => self.next_from_summary(),
            Step::Done => Vec::new(),
        }
    }

    fn go(&mut self, step: Step) -> Vec<Effect> {
        self.step = step;
        self.cursor = 0;
        Vec::new()
    }

    fn next_from_connect(&mut self) -> Vec<Effect> {
        if self.token_open {
            return self.submit_token();
        }
        if let Some(row) = self.hosts.get(self.cursor) {
            if row.selected && row.needs_token() {
                self.open_token();
                return Vec::new();
            }
        }
        if let Some(row) = self
            .hosts
            .iter()
            .find(|h| h.selected && h.conn == Conn::Checking)
        {
            self.message = Some(format!("Still checking {}. One moment.", row.host));
            return Vec::new();
        }
        if self.chosen().next().is_none() {
            self.message =
                Some("Pick an account to connect, or press esc to skip for now.".to_string());
            return Vec::new();
        }
        self.go(Step::Scope)
    }

    fn open_token(&mut self) {
        self.token_open = true;
        self.token = TokenInput::default();
    }

    fn submit_token(&mut self) -> Vec<Effect> {
        if self.token.is_empty() {
            self.message = Some("Paste a token first, or press esc to cancel.".to_string());
            return Vec::new();
        }
        let secret = self.token.take();
        let Some(row) = self.hosts.get_mut(self.cursor) else {
            return Vec::new();
        };
        row.conn = Conn::Checking;
        row.auth = AuthKind::Token;
        row.cli = None;
        vec![Effect::Probe {
            host: row.host.clone(),
            kind: row.kind,
            credential: Credential::Pasted(secret),
        }]
    }

    fn next_from_summary(&mut self) -> Vec<Effect> {
        if self.writing {
            return Vec::new();
        }
        if self.existing && !self.asking_replace {
            self.asking_replace = true;
            return Vec::new();
        }
        if self.asking_replace {
            // Enter alone is not a yes.
            self.asking_replace = false;
            return Vec::new();
        }
        self.write(false)
    }

    fn confirm(&mut self, yes: bool) -> Vec<Effect> {
        if self.step != Step::Summary || !self.asking_replace {
            return Vec::new();
        }
        self.asking_replace = false;
        if yes {
            self.write(true)
        } else {
            Vec::new()
        }
    }

    fn write(&mut self, replace: bool) -> Vec<Effect> {
        if self.chosen().next().is_none() {
            self.message =
                Some("There's nothing to write yet. Go back and connect an account.".into());
            return Vec::new();
        }
        self.writing = true;
        vec![Effect::Write(self.plan(replace))]
    }

    fn shift(&mut self, delta: isize) -> Vec<Effect> {
        let len = match self.step {
            Step::Connect => self.hosts.len(),
            Step::Scope => self.scope_items().len(),
            Step::Look => return self.look(delta),
            _ => 0,
        };
        if len == 0 || self.token_open {
            return Vec::new();
        }
        self.cursor = self.cursor.saturating_add_signed(delta).min(len - 1);
        Vec::new()
    }

    fn look(&mut self, delta: isize) -> Vec<Effect> {
        if self.step == Step::Look {
            let n = BUILTIN_IDS.len();
            self.theme = (self.theme + n).wrapping_add_signed(delta) % n;
        }
        Vec::new()
    }

    fn toggle(&mut self) -> Vec<Effect> {
        match self.step {
            Step::Connect if !self.token_open => self.toggle_host(),
            Step::Scope => self.toggle_scope(),
            Step::Jax => {
                self.jax = !self.jax;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn toggle_host(&mut self) -> Vec<Effect> {
        let Some(row) = self.hosts.get_mut(self.cursor) else {
            return Vec::new();
        };
        row.selected = !row.selected;
        if row.selected && row.needs_token() {
            self.open_token();
        }
        Vec::new()
    }

    fn toggle_scope(&mut self) -> Vec<Effect> {
        match self.scope_items().get(self.cursor).copied() {
            Some(ScopeItem::User(h)) => self.hosts[h].user = !self.hosts[h].user,
            Some(ScopeItem::Org(h, o)) => {
                let org = &mut self.hosts[h].orgs[o];
                org.1 = !org.1;
            }
            None => {}
        }
        Vec::new()
    }

    fn typed(&mut self, c: char) -> Vec<Effect> {
        if self.token_open {
            self.token.push_str(c.encode_utf8(&mut [0; 4]));
            return Vec::new();
        }
        match (self.step, c) {
            (Step::Jax, 'j' | 'J') => self.toggle(),
            (_, ' ') | (Step::Connect | Step::Scope, 'x') => self.toggle(),
            (Step::Summary, 'y' | 'Y') => self.confirm(true),
            (Step::Summary, 'n' | 'N') => self.confirm(false),
            (_, 'k') => self.shift(-1),
            (_, 'j') => self.shift(1),
            (Step::Look, 'h') => self.look(-1),
            (Step::Look, 'l') => self.look(1),
            _ => Vec::new(),
        }
    }

    fn on_click(&mut self, click: Click) -> Vec<Effect> {
        match click {
            Click::Next => self.next(),
            Click::Back => self.back(),
            Click::Skip => self.skip(),
            Click::Yes => self.confirm(true),
            Click::No => self.confirm(false),
            Click::Jax => {
                if self.step == Step::Jax {
                    self.jax = !self.jax;
                }
                Vec::new()
            }
            Click::Theme(i) => {
                if i < BUILTIN_IDS.len() {
                    self.theme = i;
                }
                Vec::new()
            }
            Click::Row(i) => {
                if self.token_open {
                    return Vec::new();
                }
                let len = match self.step {
                    Step::Connect => self.hosts.len(),
                    Step::Scope => self.scope_items().len(),
                    _ => 0,
                };
                if i < len {
                    self.cursor = i;
                    return self.toggle();
                }
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::detect::Evidence;

    fn found(host: &str, kind: ForgeKind, cli: Option<(CliTool, &str)>) -> FoundHost {
        let mut evidence = vec![Evidence::GitConfig];
        if let Some((tool, user)) = cli {
            evidence = vec![Evidence::Cli {
                tool,
                user: user.into(),
            }];
        }
        FoundHost {
            host: host.into(),
            kind,
            evidence,
        }
    }

    fn detection() -> Detection {
        Detection {
            hosts: vec![
                found(
                    "github.com",
                    ForgeKind::GitHub,
                    Some((CliTool::Gh, "smorris")),
                ),
                found("gitlab.work.ca", ForgeKind::GitLab, None),
            ],
            notes: Vec::new(),
        }
    }

    fn info(login: &str, orgs: &[&str]) -> ProbeInfo {
        ProbeInfo {
            login: login.into(),
            orgs: orgs.iter().map(|o| (*o).to_string()).collect(),
            checked: true,
            ..ProbeInfo::default()
        }
    }

    fn flow() -> Flow {
        Flow::new(PathBuf::from("/c/config.toml"), false, "liminal-hq", true)
    }

    /// A flow past detection with github.com connected through gh.
    fn connected() -> Flow {
        let mut f = flow();
        f.apply(Input::Detected(detection()));
        f.apply(Input::Probed {
            host: "github.com".into(),
            result: Ok(info("smorris", &["liminal-hq", "acme"])),
        });
        f
    }

    #[test]
    fn starting_asks_for_detection_and_detection_probes_signed_in_hosts() {
        let mut f = flow();
        assert_eq!(f.start(), [Effect::Detect]);
        let effects = f.apply(Input::Detected(detection()));
        assert_eq!(
            effects,
            [Effect::Probe {
                host: "github.com".into(),
                kind: ForgeKind::GitHub,
                credential: Credential::Cli
            }]
        );
        assert!(!f.detecting);
        assert!(f.hosts[0].selected && f.hosts[0].conn == Conn::Checking);
        assert!(!f.hosts[1].selected && f.hosts[1].conn == Conn::NeedsToken);
    }

    #[test]
    fn a_probe_fills_in_the_account_and_its_orgs() {
        let f = connected();
        assert_eq!(f.hosts[0].account().as_deref(), Some("smorris"));
        assert_eq!(f.hosts[0].orgs.len(), 2);
        assert!(f.hosts[0].orgs.iter().all(|(_, on)| !on));
    }

    #[test]
    fn welcome_leads_to_connect_and_back() {
        let mut f = connected();
        f.apply(Input::Next);
        assert_eq!(f.step, Step::Connect);
        f.apply(Input::Back);
        assert_eq!(f.step, Step::Welcome);
        f.apply(Input::Back);
        assert_eq!(f.step, Step::Welcome);
    }

    #[test]
    fn continuing_waits_for_checks_and_needs_a_connection() {
        let mut f = flow();
        f.apply(Input::Detected(detection()));
        f.apply(Input::Next);
        f.apply(Input::Next);
        assert_eq!(f.step, Step::Connect);
        assert!(f.message.as_deref().unwrap().contains("Still checking"));

        let mut f = flow();
        f.apply(Input::Detected(Detection::default()));
        f.apply(Input::Next);
        f.apply(Input::Next);
        assert_eq!(f.step, Step::Connect);
        assert!(f.message.as_deref().unwrap().contains("esc to skip"));
    }

    #[test]
    fn selecting_a_host_without_credentials_opens_the_token_field() {
        let mut f = connected();
        f.apply(Input::Next);
        f.apply(Input::Down);
        f.apply(Input::Toggle);
        assert!(f.token_open && f.hosts[1].selected);
        for c in "glpat-abc".chars() {
            f.apply(Input::Char(c));
        }
        assert_eq!(f.token.len(), 9);
        assert!(!format!("{f:?}").contains("glpat"));
        let effects = f.apply(Input::Next);
        assert_eq!(f.hosts[1].conn, Conn::Checking);
        assert_eq!(f.hosts[1].auth, AuthKind::Token);
        assert!(f.token.is_empty());
        match &effects[..] {
            [Effect::Probe {
                host,
                credential: Credential::Pasted(s),
                ..
            }] => {
                assert_eq!(host, "gitlab.work.ca");
                assert_eq!(s.expose(), "glpat-abc");
            }
            other => panic!("{other:?}"),
        }
        f.apply(Input::Probed {
            host: "gitlab.work.ca".into(),
            result: Ok(ProbeInfo::default()),
        });
        assert!(!f.token_open);
        assert!(f.hosts[1].connected().is_some() && f.hosts[1].selected);
    }

    #[test]
    fn an_empty_token_is_not_sent_and_escape_cancels_the_field() {
        let mut f = connected();
        f.apply(Input::Next);
        f.apply(Input::Down);
        f.apply(Input::Toggle);
        assert!(f.apply(Input::Next).is_empty());
        assert!(f.message.as_deref().unwrap().contains("Paste a token"));
        f.apply(Input::Skip);
        assert!(!f.token_open);
        assert!(!f.hosts[1].selected);
        assert_eq!(f.step, Step::Connect);
    }

    #[test]
    fn a_rejected_token_keeps_the_field_for_another_try() {
        let mut f = connected();
        f.apply(Input::Next);
        f.apply(Input::Down);
        f.apply(Input::Toggle);
        f.apply(Input::Paste("bad".into()));
        f.apply(Input::Next);
        f.apply(Input::Probed {
            host: "gitlab.work.ca".into(),
            result: Err("That token was rejected.".into()),
        });
        assert!(f.token_open && f.hosts[1].selected);
        assert!(f.hosts[1].needs_token());
    }

    #[test]
    fn a_failed_sign_in_reuse_is_deselected() {
        let mut f = flow();
        f.apply(Input::Detected(detection()));
        f.apply(Input::Probed {
            host: "github.com".into(),
            result: Err("gh's token was rejected".into()),
        });
        assert!(!f.hosts[0].selected);
        assert!(f.hosts[0].needs_token());
    }

    #[test]
    fn scope_lists_the_account_and_each_org_and_toggles_them() {
        let mut f = connected();
        f.apply(Input::Next);
        f.apply(Input::Next);
        assert_eq!(f.step, Step::Scope);
        assert_eq!(
            f.scope_items(),
            [
                ScopeItem::User(0),
                ScopeItem::Org(0, 0),
                ScopeItem::Org(0, 1)
            ]
        );
        f.apply(Input::Toggle);
        f.apply(Input::Down);
        f.apply(Input::Down);
        f.apply(Input::Toggle);
        assert!(f.hosts[0].user);
        assert_eq!(f.hosts[0].orgs[1], ("acme".to_string(), true));
        f.apply(Input::Down);
        assert_eq!(f.cursor, 2, "the cursor stops at the last item");
    }

    #[test]
    fn look_cycles_themes_and_wraps() {
        let mut f = connected();
        f.step = Step::Look;
        assert_eq!(f.theme_id(), "liminal-hq");
        f.apply(Input::Right);
        assert_eq!(f.theme_id(), "dusk");
        f.apply(Input::Left);
        f.apply(Input::Left);
        assert_eq!(f.theme_id(), "afterglow-light");
        f.apply(Input::Click(Click::Theme(2)));
        assert_eq!(f.theme_id(), "afterglow-dark");
    }

    #[test]
    fn jax_toggles_with_space_and_j() {
        let mut f = connected();
        f.step = Step::Jax;
        f.apply(Input::Char('J'));
        assert!(!f.jax);
        f.apply(Input::Toggle);
        assert!(f.jax);
        f.apply(Input::Click(Click::Jax));
        assert!(!f.jax);
    }

    fn to_summary(f: &mut Flow) {
        f.apply(Input::Next);
        f.apply(Input::Next);
        f.apply(Input::Next);
        f.apply(Input::Next);
        f.apply(Input::Next);
        assert_eq!(f.step, Step::Summary);
    }

    #[test]
    fn summary_writes_a_plan_of_the_choices() {
        let mut f = connected();
        f.apply(Input::Next);
        f.apply(Input::Next);
        f.apply(Input::Down);
        f.apply(Input::Toggle);
        f.apply(Input::Next);
        f.apply(Input::Right);
        f.apply(Input::Next);
        f.apply(Input::Char('j'));
        f.apply(Input::Next);
        assert_eq!(f.step, Step::Summary);
        let effects = f.apply(Input::Next);
        let [Effect::Write(plan)] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert_eq!(plan.theme, "dusk");
        assert!(!plan.jax && !plan.replace);
        assert_eq!(plan.sources.len(), 1);
        let s = &plan.sources[0];
        assert_eq!(
            (s.name.as_str(), s.auth.clone(), s.scope_user),
            ("github.com", AuthKind::Cli, false)
        );
        assert_eq!(s.owners, ["liminal-hq"]);
        assert!(f.writing);
        f.apply(Input::Written(Ok(Written {
            path: "/c/config.toml".into(),
            backup: None,
        })));
        assert!(f.is_done());
        assert!(matches!(f.outcome, Some(Outcome::Written { .. })));
    }

    #[test]
    fn an_existing_file_needs_an_explicit_yes_and_defaults_to_no() {
        let mut f = connected();
        f.existing = true;
        to_summary(&mut f);
        assert!(f.apply(Input::Next).is_empty());
        assert!(f.asking_replace);
        assert!(f.apply(Input::Next).is_empty(), "enter alone is a no");
        assert!(!f.asking_replace && !f.writing);

        f.apply(Input::Next);
        assert!(f.apply(Input::Confirm(false)).is_empty());
        assert!(!f.writing);

        f.apply(Input::Next);
        let effects = f.apply(Input::Char('y'));
        let [Effect::Write(plan)] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert!(plan.replace);
    }

    #[test]
    fn a_failed_write_stays_on_the_summary_with_the_reason() {
        let mut f = connected();
        to_summary(&mut f);
        f.apply(Input::Next);
        f.apply(Input::Written(Err("Couldn't write it.".into())));
        assert_eq!(f.step, Step::Summary);
        assert!(!f.writing);
        assert_eq!(f.message.as_deref(), Some("Couldn't write it."));
    }

    #[test]
    fn escape_skips_without_a_plan() {
        let mut f = connected();
        f.apply(Input::Skip);
        assert_eq!(f.outcome, Some(Outcome::Skipped));
        assert!(f.is_done());
    }

    #[test]
    fn clicks_move_and_toggle_rows() {
        let mut f = connected();
        f.apply(Input::Next);
        f.apply(Input::Click(Click::Row(1)));
        assert_eq!(f.cursor, 1);
        assert!(f.token_open);
        f.apply(Input::Click(Click::Back));
        assert!(!f.token_open);
        f.apply(Input::Click(Click::Row(0)));
        assert!(!f.hosts[0].selected);
    }
}
