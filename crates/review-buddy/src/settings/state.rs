//! The Settings → Sources state machine. Pure: no I/O, so every transition is unit-tested.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_core::{AuthMode, ForgeKind, Source};
use rb_platform::Secret;

use crate::app::editor::Editor;
use crate::app::{Notice, NoticeKind};
use crate::setup::{AuthKind, Detection, FoundHost, SourceSpec};

/// One configured source as the table shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRow {
    pub spec: SourceSpec,
    pub enabled: bool,
    pub in_all: bool,
    pub include_drafts: bool,
    pub tag_colour: Option<String>,
    /// GitHub repositories or GitLab projects named in the scope. Shown, not edited here.
    pub repos: Vec<String>,
}

impl SourceRow {
    /// `liminal-hq, acme + you`, or `everything you can see`.
    pub fn scope_summary(&self) -> String {
        let spec = &self.spec;
        let mut parts: Vec<String> = spec.owners.clone();
        parts.extend(self.repos.iter().cloned());
        if spec.scope_user {
            parts.push("you".to_string());
        }
        if parts.is_empty() {
            "everything you can see".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// How the token is found: `gh`, `glab`, `keyring`, `env:NAME` or `command`.
    pub fn auth_label(&self) -> String {
        match &self.spec.auth {
            AuthKind::Cli => match self.spec.kind {
                ForgeKind::GitHub => "gh".to_string(),
                ForgeKind::GitLab => "glab".to_string(),
            },
            AuthKind::Token => "keyring".to_string(),
            AuthKind::Env(var) => format!("env:{var}"),
            AuthKind::Command(_) => "command".to_string(),
        }
    }
}

/// The config file the sources come from, when that isn't the write target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub path: PathBuf,
    /// `from /etc/xdg/review-buddy/config.toml` or `from config.d/10-work.toml`.
    pub label: String,
}

/// What the config says right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub rows: Vec<SourceRow>,
    pub write_target: PathBuf,
    /// The file the sources are defined in, if any are.
    pub origin: Option<Origin>,
    /// The sources live in the write target (or nowhere yet), so they can be changed here.
    pub editable: bool,
}

/// A token check that finished.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TestInfo {
    pub user: String,
    pub scopes: Vec<String>,
    pub expires: Option<String>,
    /// A calm remark, such as a missing scope.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Ok(TestInfo),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Row(usize),
    /// A field of the form, by position among the visible ones.
    Field(usize),
    /// A suggestion in the add list, or a button of the remove confirm.
    Choose(usize),
    Save,
    Cancel,
    Add,
    Test,
    Edit,
    Toggle,
    Remove,
    Back,
}

/// A change to write to the write target.
#[derive(Debug, Clone)]
pub enum Change {
    Add {
        spec: SourceSpec,
        token: Option<Secret>,
    },
    Edit {
        before: SourceSpec,
        after: SourceSpec,
        token: Option<Secret>,
    },
    Toggle {
        name: String,
        enabled: bool,
    },
    Remove {
        spec: SourceSpec,
        forget_token: bool,
    },
}

/// Work for the runtime. Results come back as [`Input`]s.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Effect {
    /// Read the layered config again.
    Load,
    Detect,
    Test {
        name: String,
    },
    Save {
        target: PathBuf,
        change: Change,
    },
}

#[derive(Debug, Clone)]
pub struct Saved {
    pub message: String,
    /// Select this source once the config is read again.
    pub select: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Input {
    Loaded(Result<Snapshot, String>),
    Detected(Detection),
    Tested {
        name: String,
        result: Result<TestInfo, String>,
    },
    Saved(Result<Saved, String>),
    Click(Click),
    Key(KeyEvent),
    Paste(String),
    Scroll {
        down: bool,
    },
}

/// What applying an input asks of the app around the state.
#[derive(Debug, Default)]
pub struct Out {
    pub effects: Vec<Effect>,
    pub notice: Option<Notice>,
    /// A change was written: rebuild the queue from the new config.
    pub reload: bool,
    /// Leave Settings.
    pub back: bool,
    /// The key wasn't for Settings (help, theme): let the app handle it.
    pub pass: bool,
}

impl Out {
    fn say(kind: NoticeKind, text: impl Into<String>) -> Self {
        Self {
            notice: Some(Notice::new(kind, text)),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Kind,
    Host,
    ApiUrl,
    Auth,
    Detail,
    Owners,
    User,
    Token,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthChoice {
    Cli,
    Token,
    Env,
    Command,
}

impl AuthChoice {
    const ALL: [Self; 4] = [Self::Cli, Self::Token, Self::Env, Self::Command];

    fn step(self, forward: bool) -> Self {
        let at = Self::ALL.iter().position(|c| *c == self).unwrap_or(0);
        let n = Self::ALL.len();
        Self::ALL[(at + if forward { 1 } else { n - 1 }) % n]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Add,
    Edit(Box<SourceSpec>),
}

/// An editor whose text stays out of `Debug` output.
#[derive(Clone)]
pub struct Hidden(pub Editor);

impl std::fmt::Debug for Hidden {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hidden(<redacted>)")
    }
}

/// The add and edit form.
#[derive(Debug, Clone)]
pub struct Form {
    pub mode: Mode,
    pub kind: ForgeKind,
    pub name: Editor,
    pub host: Editor,
    pub api_url: Editor,
    pub auth: AuthChoice,
    /// The variable name for `env:`, or the command for `command`.
    pub detail: Editor,
    /// Organisations or groups, separated by commas.
    pub owners: Editor,
    pub user: bool,
    /// The token as typed. Never drawn, never written to the config.
    pub token: Hidden,
    pub focus: Field,
    pub error: Option<String>,
    pub saving: bool,
}

impl Form {
    pub fn blank() -> Self {
        Self::with(ForgeKind::GitHub, "", AuthChoice::Token)
    }

    fn with(kind: ForgeKind, host: &str, auth: AuthChoice) -> Self {
        Self {
            mode: Mode::Add,
            kind,
            name: Editor::with_text(host),
            host: Editor::with_text(host),
            api_url: Editor::new(),
            auth,
            detail: Editor::new(),
            owners: Editor::new(),
            user: false,
            token: Hidden(Editor::new()),
            focus: if host.is_empty() {
                Field::Host
            } else {
                Field::Name
            },
            error: None,
            saving: false,
        }
    }

    /// A form for a host that detection found.
    pub fn from_found(found: &FoundHost) -> Self {
        let auth = if found.cli().is_some() {
            AuthChoice::Cli
        } else {
            AuthChoice::Token
        };
        Self::with(found.kind, &found.host, auth)
    }

    pub fn edit(spec: &SourceSpec) -> Self {
        let (auth, detail) = match &spec.auth {
            AuthKind::Cli => (AuthChoice::Cli, String::new()),
            AuthKind::Token => (AuthChoice::Token, String::new()),
            AuthKind::Env(var) => (AuthChoice::Env, var.clone()),
            AuthKind::Command(command) => (AuthChoice::Command, command.clone()),
        };
        Self {
            mode: Mode::Edit(Box::new(spec.clone())),
            kind: spec.kind,
            name: Editor::with_text(&spec.name),
            host: Editor::with_text(&spec.host),
            api_url: Editor::with_text(spec.api_url.as_deref().unwrap_or("")),
            auth,
            detail: Editor::with_text(&detail),
            owners: Editor::with_text(&spec.owners.join(", ")),
            user: spec.scope_user,
            token: Hidden(Editor::new()),
            focus: Field::Name,
            error: None,
            saving: false,
        }
    }

    pub fn is_edit(&self) -> bool {
        matches!(self.mode, Mode::Edit(_))
    }

    /// The fields on screen, in tab order.
    pub fn visible(&self) -> Vec<Field> {
        let mut fields = vec![Field::Name];
        if !self.is_edit() {
            fields.push(Field::Kind);
        }
        fields.extend([Field::Host, Field::ApiUrl, Field::Auth]);
        if matches!(self.auth, AuthChoice::Env | AuthChoice::Command) {
            fields.push(Field::Detail);
        }
        fields.push(Field::Owners);
        if self.kind == ForgeKind::GitHub {
            fields.push(Field::User);
        }
        if self.auth == AuthChoice::Token {
            fields.push(Field::Token);
        }
        fields
    }

    fn is_choice(field: Field) -> bool {
        matches!(field, Field::Kind | Field::Auth | Field::User)
    }

    fn editor_mut(&mut self, field: Field) -> Option<&mut Editor> {
        match field {
            Field::Name => Some(&mut self.name),
            Field::Host => Some(&mut self.host),
            Field::ApiUrl => Some(&mut self.api_url),
            Field::Detail => Some(&mut self.detail),
            Field::Owners => Some(&mut self.owners),
            Field::Token => Some(&mut self.token.0),
            Field::Kind | Field::Auth | Field::User => None,
        }
    }

    fn shift_focus(&mut self, forward: bool) {
        let fields = self.visible();
        let at = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        let n = fields.len();
        self.focus = fields[(at + if forward { 1 } else { n - 1 }) % n];
    }

    fn cycle(&mut self, forward: bool) {
        match self.focus {
            Field::Kind => {
                self.kind = match self.kind {
                    ForgeKind::GitHub => ForgeKind::GitLab,
                    ForgeKind::GitLab => ForgeKind::GitHub,
                };
                if self.kind == ForgeKind::GitLab {
                    self.user = false;
                }
            }
            Field::Auth => self.auth = self.auth.step(forward),
            Field::User => self.user = !self.user,
            _ => {}
        }
    }

    /// The token as typed, if there is one to save.
    pub fn token_secret(&self) -> Option<Secret> {
        if self.auth != AuthChoice::Token {
            return None;
        }
        let text = self.token.0.text();
        let text = text.trim();
        (!text.is_empty()).then(|| Secret::new(text))
    }

    /// The source this form describes, or what to fix first.
    pub fn spec(&self, rows: &[SourceRow]) -> Result<SourceSpec, String> {
        let name = self.name.text().trim().to_string();
        if name.is_empty() {
            return Err("Give the source a name, like work or github.com.".to_string());
        }
        let own = match &self.mode {
            Mode::Edit(before) => Some(before.name.as_str()),
            Mode::Add => None,
        };
        let taken = rows
            .iter()
            .any(|r| r.spec.name.eq_ignore_ascii_case(&name) && Some(r.spec.name.as_str()) != own);
        if taken {
            return Err(format!(
                "There's already a source called {name}. Pick another name."
            ));
        }
        let host = self.host.text().trim().to_string();
        if host.is_empty() || host.contains("://") || host.contains('/') || host.contains(' ') {
            return Err(
                "Host should be a bare name like github.com or gitlab.work.ca, with no https://."
                    .to_string(),
            );
        }
        let api_url = self.api_url.text().trim().to_string();
        let web = api_url.starts_with("https://") || api_url.starts_with("http://");
        if !api_url.is_empty() && !web {
            return Err("API address should start with https://, or be left blank.".to_string());
        }
        let detail = self.detail.text().trim().to_string();
        let auth = match self.auth {
            AuthChoice::Cli => AuthKind::Cli,
            AuthChoice::Token => AuthKind::Token,
            AuthChoice::Env if detail.is_empty() || detail.contains(' ') => {
                return Err(
                    "Name the environment variable that holds the token, like GITHUB_TOKEN."
                        .to_string(),
                )
            }
            AuthChoice::Env => AuthKind::Env(detail),
            AuthChoice::Command if detail.is_empty() => {
                return Err("Enter the command that prints the token.".to_string())
            }
            AuthChoice::Command => AuthKind::Command(detail),
        };
        let owners: Vec<String> = self
            .owners
            .text()
            .split([',', ' '])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Ok(SourceSpec {
            name,
            kind: self.kind,
            host,
            api_url: (!api_url.is_empty()).then_some(api_url),
            auth,
            scope_user: self.kind == ForgeKind::GitHub && self.user,
            owners,
        })
    }
}

/// The add list: hosts found on this machine, then "another host".
#[derive(Debug, Clone)]
pub struct Pick {
    pub hosts: Vec<FoundHost>,
    pub looking: bool,
    pub cursor: usize,
}

impl Pick {
    pub fn len(&self) -> usize {
        self.hosts.len() + 1
    }

    pub fn is_empty(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveChoice {
    Keep,
    Remove,
    RemoveAndToken,
}

/// The remove confirmation. It opens on **No**.
#[derive(Debug, Clone)]
pub struct Remove {
    pub spec: SourceSpec,
    /// Another source uses the same keyring entry, so it can't be removed with this one.
    pub shares_token: bool,
    pub choice: usize,
}

impl Remove {
    pub fn choices(&self) -> Vec<RemoveChoice> {
        let mut choices = vec![RemoveChoice::Keep, RemoveChoice::Remove];
        if self.spec.auth == AuthKind::Token && !self.shares_token {
            choices.push(RemoveChoice::RemoveAndToken);
        }
        choices
    }
}

#[derive(Debug, Clone)]
pub enum Modal {
    Pick(Pick),
    Form(Box<Form>),
    Remove(Remove),
}

#[derive(Debug, Clone)]
pub struct State {
    pub rows: Vec<SourceRow>,
    pub cursor: usize,
    pub loaded: bool,
    pub write_target: PathBuf,
    pub origin: Option<Origin>,
    pub editable: bool,
    pub demo: bool,
    pub checks: HashMap<String, Check>,
    pub testing: HashSet<String>,
    pub modal: Option<Modal>,
    select_after: Option<String>,
}

impl State {
    /// Settings just opened: the config is being read.
    pub fn loading() -> Self {
        Self {
            rows: Vec::new(),
            cursor: 0,
            loaded: false,
            write_target: PathBuf::new(),
            origin: None,
            editable: true,
            demo: false,
            checks: HashMap::new(),
            testing: HashSet::new(),
            modal: None,
            select_after: None,
        }
    }

    /// Demo mode: the demo sources, read-only. Nothing real is read or written.
    pub fn demo(sources: &[Source]) -> Self {
        let rows = sources
            .iter()
            .map(|s| SourceRow {
                spec: SourceSpec {
                    name: s.label.clone(),
                    kind: s.kind,
                    host: s.host.clone(),
                    api_url: None,
                    auth: match s.auth {
                        AuthMode::Cli => AuthKind::Cli,
                        AuthMode::Token => AuthKind::Token,
                    },
                    scope_user: s.scope.user,
                    owners: s.scope.owners.clone(),
                },
                enabled: true,
                in_all: s.in_all,
                include_drafts: s.include_drafts,
                tag_colour: s.tag_colour.clone(),
                repos: s.scope.repos.clone(),
            })
            .collect();
        Self {
            rows,
            loaded: true,
            demo: true,
            editable: false,
            ..Self::loading()
        }
    }

    pub fn selected(&self) -> Option<&SourceRow> {
        self.rows.get(self.cursor)
    }

    /// Why changes are off, if they are.
    pub fn locked(&self) -> Option<String> {
        if self.demo {
            return Some("Demo sources can't be changed (demo).".to_string());
        }
        if self.editable {
            return None;
        }
        let origin = self.origin.as_ref()?;
        Some(format!(
            "These sources come {}. Review Buddy doesn't edit that file. Change them there, or move them into {}.",
            origin.label,
            self.write_target.display()
        ))
    }

    pub fn is_modal(&self) -> bool {
        self.modal.is_some()
    }

    pub fn apply(&mut self, input: Input) -> Out {
        match input {
            Input::Loaded(result) => self.loaded(result),
            Input::Detected(found) => self.detected(found),
            Input::Tested { name, result } => self.tested(name, result),
            Input::Saved(result) => self.saved(result),
            Input::Click(click) => self.click(click),
            Input::Key(key) => self.key(key),
            Input::Paste(text) => self.paste(&text),
            Input::Scroll { down } => {
                if self.modal.is_none() {
                    self.step(down);
                }
                Out::default()
            }
        }
    }

    fn loaded(&mut self, result: Result<Snapshot, String>) -> Out {
        self.loaded = true;
        match result {
            Ok(snapshot) => {
                self.rows = snapshot.rows;
                self.write_target = snapshot.write_target;
                self.origin = snapshot.origin;
                self.editable = snapshot.editable;
                if let Some(name) = self.select_after.take() {
                    if let Some(at) = self.rows.iter().position(|r| r.spec.name == name) {
                        self.cursor = at;
                    }
                }
                self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
                Out::default()
            }
            Err(reason) => Out::say(
                NoticeKind::Warning,
                format!("Couldn't read your config: {reason}"),
            ),
        }
    }

    fn detected(&mut self, found: Detection) -> Out {
        let configured: Vec<&str> = self.rows.iter().map(|r| r.spec.host.as_str()).collect();
        if let Some(Modal::Pick(pick)) = &mut self.modal {
            pick.hosts = found
                .hosts
                .into_iter()
                .filter(|h| !configured.contains(&h.host.as_str()))
                .collect();
            pick.looking = false;
            pick.cursor = pick.cursor.min(pick.len() - 1);
        }
        Out::default()
    }

    fn tested(&mut self, name: String, result: Result<TestInfo, String>) -> Out {
        self.testing.remove(&name);
        let (kind, text) = match &result {
            Ok(info) if info.user.is_empty() => {
                (NoticeKind::Success, format!("{name} is signed in."))
            }
            Ok(info) => (
                NoticeKind::Success,
                format!("{name} is signed in as {}.", info.user),
            ),
            Err(reason) => (NoticeKind::Warning, reason.clone()),
        };
        self.checks.insert(
            name,
            match result {
                Ok(info) => Check::Ok(info),
                Err(reason) => Check::Failed(reason),
            },
        );
        Out::say(kind, text)
    }

    fn saved(&mut self, result: Result<Saved, String>) -> Out {
        match result {
            Ok(saved) => {
                self.modal = None;
                self.select_after = saved.select;
                Out {
                    notice: Some(Notice::new(NoticeKind::Success, saved.message)),
                    effects: vec![Effect::Load],
                    reload: true,
                    ..Out::default()
                }
            }
            Err(reason) => {
                if let Some(Modal::Form(form)) = &mut self.modal {
                    form.saving = false;
                    form.error = Some(reason);
                    Out::default()
                } else {
                    Out::say(NoticeKind::Warning, reason)
                }
            }
        }
    }

    fn step(&mut self, down: bool) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        self.cursor = if down {
            (self.cursor + 1).min(last)
        } else {
            self.cursor.saturating_sub(1)
        };
    }

    fn paste(&mut self, text: &str) -> Out {
        if let Some(Modal::Form(form)) = &mut self.modal {
            let field = form.focus;
            let line: String = text.lines().next().unwrap_or("").trim().to_string();
            if let Some(editor) = form.editor_mut(field) {
                editor.insert_str(&line);
            }
        }
        Out::default()
    }

    fn click(&mut self, click: Click) -> Out {
        match (&mut self.modal, click) {
            (None, Click::Row(i)) => {
                if i < self.rows.len() {
                    self.cursor = i;
                }
                Out::default()
            }
            (None, Click::Add) => self.add(),
            (None, Click::Test) => self.test(),
            (None, Click::Edit) => self.edit(),
            (None, Click::Toggle) => self.toggle(),
            (None, Click::Remove) => self.remove(),
            (None, Click::Back) => Out {
                back: true,
                ..Out::default()
            },
            (Some(Modal::Form(form)), Click::Field(i)) => {
                if let Some(field) = form.visible().get(i).copied() {
                    form.focus = field;
                    if Form::is_choice(field) {
                        form.cycle(true);
                    }
                }
                Out::default()
            }
            (Some(Modal::Form(_)), Click::Save) => self.save_form(),
            (Some(Modal::Pick(_)), Click::Choose(i)) => self.pick(i),
            (Some(Modal::Remove(r)), Click::Choose(i)) => {
                r.choice = i.min(r.choices().len() - 1);
                self.confirm_remove()
            }
            (Some(_), Click::Cancel) => {
                self.modal = None;
                Out::default()
            }
            _ => Out::default(),
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> Out {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT)
        {
            return Out::default();
        }
        match &self.modal {
            None => self.key_list(key),
            Some(Modal::Pick(_)) => self.key_pick(key),
            Some(Modal::Form(_)) => self.key_form(key),
            Some(Modal::Remove(_)) => self.key_remove(key),
        }
    }

    fn key_list(&mut self, key: KeyEvent) -> Out {
        match key.code {
            KeyCode::Esc => Out {
                back: true,
                ..Out::default()
            },
            KeyCode::Char('?' | 'T') => Out {
                pass: true,
                ..Out::default()
            },
            _ if !self.loaded => Out::default(),
            KeyCode::Down | KeyCode::Char('j') => {
                self.step(true);
                Out::default()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.step(false);
                Out::default()
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.cursor = 0;
                Out::default()
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.cursor = self.rows.len().saturating_sub(1);
                Out::default()
            }
            KeyCode::Char('t') => self.test(),
            KeyCode::Char('e') | KeyCode::Enter => self.edit(),
            KeyCode::Char('a' | 'n') => self.add(),
            KeyCode::Char(' ') => self.toggle(),
            KeyCode::Char('x') | KeyCode::Delete => self.remove(),
            _ => Out::default(),
        }
    }

    fn locked_out(&self) -> Option<Out> {
        self.locked().map(|why| Out::say(NoticeKind::Info, why))
    }

    fn add(&mut self) -> Out {
        if let Some(out) = self.locked_out() {
            return out;
        }
        self.modal = Some(Modal::Pick(Pick {
            hosts: Vec::new(),
            looking: true,
            cursor: 0,
        }));
        Out {
            effects: vec![Effect::Detect],
            ..Out::default()
        }
    }

    fn edit(&mut self) -> Out {
        if let Some(out) = self.locked_out() {
            return out;
        }
        if let Some(row) = self.selected() {
            self.modal = Some(Modal::Form(Box::new(Form::edit(&row.spec))));
        }
        Out::default()
    }

    fn toggle(&mut self) -> Out {
        if let Some(out) = self.locked_out() {
            return out;
        }
        let Some(row) = self.selected() else {
            return Out::default();
        };
        Out {
            effects: vec![Effect::Save {
                target: self.write_target.clone(),
                change: Change::Toggle {
                    name: row.spec.name.clone(),
                    enabled: !row.enabled,
                },
            }],
            ..Out::default()
        }
    }

    fn remove(&mut self) -> Out {
        if let Some(out) = self.locked_out() {
            return out;
        }
        let Some(row) = self.selected() else {
            return Out::default();
        };
        let shares_token = self.rows.iter().any(|r| {
            r.spec.name != row.spec.name
                && r.spec.host == row.spec.host
                && r.spec.auth == AuthKind::Token
        });
        self.modal = Some(Modal::Remove(Remove {
            spec: row.spec.clone(),
            shares_token,
            choice: 0,
        }));
        Out::default()
    }

    fn test(&mut self) -> Out {
        if self.demo {
            return Out::say(
                NoticeKind::Info,
                "Token checks are off in demo mode (demo).",
            );
        }
        let Some(row) = self.selected() else {
            return Out::default();
        };
        let name = row.spec.name.clone();
        if !self.testing.insert(name.clone()) {
            return Out::default();
        }
        Out {
            effects: vec![Effect::Test { name }],
            ..Out::default()
        }
    }

    fn key_pick(&mut self, key: KeyEvent) -> Out {
        let Some(Modal::Pick(pick)) = &mut self.modal else {
            return Out::default();
        };
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                Out::default()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                pick.cursor = (pick.cursor + 1).min(pick.len() - 1);
                Out::default()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                pick.cursor = pick.cursor.saturating_sub(1);
                Out::default()
            }
            KeyCode::Char(c @ '1'..='9') => {
                let i = usize::from(c as u8 - b'1');
                if i < pick.len() {
                    self.pick(i)
                } else {
                    Out::default()
                }
            }
            KeyCode::Enter => {
                let i = pick.cursor;
                self.pick(i)
            }
            _ => Out::default(),
        }
    }

    fn pick(&mut self, i: usize) -> Out {
        let Some(Modal::Pick(pick)) = &self.modal else {
            return Out::default();
        };
        let form = match pick.hosts.get(i) {
            Some(found) => Form::from_found(found),
            None => Form::blank(),
        };
        self.modal = Some(Modal::Form(Box::new(form)));
        Out::default()
    }

    fn key_form(&mut self, key: KeyEvent) -> Out {
        let rows = self.rows.clone();
        let Some(Modal::Form(form)) = &mut self.modal else {
            return Out::default();
        };
        if form.saving {
            return Out::default();
        }
        form.error = None;
        let field = form.focus;
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
            }
            KeyCode::Enter => return self.save_form_with(&rows),
            KeyCode::Tab | KeyCode::Down => form.shift_focus(true),
            KeyCode::BackTab | KeyCode::Up => form.shift_focus(false),
            KeyCode::Right | KeyCode::Char(' ') if Form::is_choice(field) => form.cycle(true),
            KeyCode::Left if Form::is_choice(field) => form.cycle(false),
            code => {
                if let Some(editor) = form.editor_mut(field) {
                    match code {
                        KeyCode::Char(c) => editor.insert_char(c),
                        KeyCode::Backspace => editor.backspace(),
                        KeyCode::Delete => editor.delete(),
                        KeyCode::Left => editor.left(),
                        KeyCode::Right => editor.right(),
                        KeyCode::Home => editor.home(),
                        KeyCode::End => editor.end(),
                        _ => {}
                    }
                }
            }
        }
        Out::default()
    }

    fn save_form(&mut self) -> Out {
        let rows = self.rows.clone();
        self.save_form_with(&rows)
    }

    fn save_form_with(&mut self, rows: &[SourceRow]) -> Out {
        let Some(Modal::Form(form)) = &mut self.modal else {
            return Out::default();
        };
        if form.saving {
            return Out::default();
        }
        let spec = match form.spec(rows) {
            Ok(spec) => spec,
            Err(why) => {
                form.error = Some(why);
                return Out::default();
            }
        };
        let token = form.token_secret();
        let change = match &form.mode {
            Mode::Add => Change::Add {
                spec: spec.clone(),
                token,
            },
            Mode::Edit(before) => Change::Edit {
                before: (**before).clone(),
                after: spec.clone(),
                token,
            },
        };
        form.saving = true;
        Out {
            effects: vec![Effect::Save {
                target: self.write_target.clone(),
                change,
            }],
            ..Out::default()
        }
    }

    fn key_remove(&mut self, key: KeyEvent) -> Out {
        let Some(Modal::Remove(remove)) = &mut self.modal else {
            return Out::default();
        };
        let n = remove.choices().len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                self.modal = None;
                Out::default()
            }
            KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => {
                remove.choice = (remove.choice + 1) % n;
                Out::default()
            }
            KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => {
                remove.choice = (remove.choice + n - 1) % n;
                Out::default()
            }
            KeyCode::Char('y') => {
                remove.choice = 1;
                self.confirm_remove()
            }
            KeyCode::Char('d') if n == 3 => {
                remove.choice = 2;
                self.confirm_remove()
            }
            KeyCode::Enter => self.confirm_remove(),
            _ => Out::default(),
        }
    }

    fn confirm_remove(&mut self) -> Out {
        let Some(Modal::Remove(remove)) = &self.modal else {
            return Out::default();
        };
        let choice = remove.choices()[remove.choice];
        let spec = remove.spec.clone();
        if choice == RemoveChoice::Keep {
            self.modal = None;
            return Out::default();
        }
        self.modal = None;
        Out {
            effects: vec![Effect::Save {
                target: self.write_target.clone(),
                change: Change::Remove {
                    spec,
                    forget_token: choice == RemoveChoice::RemoveAndToken,
                },
            }],
            ..Out::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, host: &str, auth: AuthKind) -> SourceSpec {
        SourceSpec {
            name: name.into(),
            kind: ForgeKind::GitHub,
            host: host.into(),
            api_url: None,
            auth,
            scope_user: false,
            owners: Vec::new(),
        }
    }

    fn row(name: &str, host: &str, auth: AuthKind) -> SourceRow {
        SourceRow {
            spec: spec(name, host, auth),
            enabled: true,
            in_all: true,
            include_drafts: false,
            tag_colour: None,
            repos: Vec::new(),
        }
    }

    fn state() -> State {
        let mut s = State::loading();
        let out = s.apply(Input::Loaded(Ok(Snapshot {
            rows: vec![
                row("work", "github.com", AuthKind::Token),
                row("lab", "gitlab.work.ca", AuthKind::Cli),
            ],
            write_target: "/c/config.toml".into(),
            origin: None,
            editable: true,
        })));
        assert!(out.effects.is_empty());
        s
    }

    fn press(s: &mut State, code: KeyCode) -> Out {
        s.apply(Input::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn type_text(s: &mut State, text: &str) {
        for c in text.chars() {
            press(s, KeyCode::Char(c));
        }
    }

    #[test]
    fn j_and_k_move_and_stay_in_range() {
        let mut s = state();
        press(&mut s, KeyCode::Char('j'));
        press(&mut s, KeyCode::Char('j'));
        assert_eq!(s.cursor, 1);
        press(&mut s, KeyCode::Char('k'));
        press(&mut s, KeyCode::Char('k'));
        assert_eq!(s.cursor, 0);
        press(&mut s, KeyCode::Char('G'));
        assert_eq!(s.cursor, 1);
        s.apply(Input::Click(Click::Row(0)));
        assert_eq!(s.cursor, 0);
        s.apply(Input::Scroll { down: true });
        assert_eq!(s.cursor, 1);
    }

    #[test]
    fn escape_goes_back_and_help_and_theme_pass_through() {
        let mut s = state();
        assert!(press(&mut s, KeyCode::Esc).back);
        assert!(press(&mut s, KeyCode::Char('?')).pass);
        assert!(press(&mut s, KeyCode::Char('T')).pass);
    }

    #[test]
    fn t_tests_the_selected_source_once() {
        let mut s = state();
        let out = press(&mut s, KeyCode::Char('t'));
        assert!(matches!(&out.effects[..], [Effect::Test { name }] if name == "work"));
        assert!(press(&mut s, KeyCode::Char('t')).effects.is_empty());
        let out = s.apply(Input::Tested {
            name: "work".into(),
            result: Ok(TestInfo {
                user: "octo".into(),
                ..TestInfo::default()
            }),
        });
        assert!(out.notice.unwrap().text.contains("signed in as octo"));
        assert!(s.testing.is_empty());
        assert!(matches!(s.checks["work"], Check::Ok(_)));
        let out = s.apply(Input::Tested {
            name: "lab".into(),
            result: Err("Not signed in. Run glab auth login.".into()),
        });
        assert_eq!(out.notice.unwrap().kind, NoticeKind::Warning);
        assert!(matches!(s.checks["lab"], Check::Failed(_)));
    }

    #[test]
    fn space_asks_to_flip_enabled_and_writes_to_the_target() {
        let mut s = state();
        let out = press(&mut s, KeyCode::Char(' '));
        match &out.effects[..] {
            [Effect::Save {
                target,
                change: Change::Toggle { name, enabled },
            }] => {
                assert_eq!(target, &PathBuf::from("/c/config.toml"));
                assert_eq!((name.as_str(), *enabled), ("work", false));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn removing_opens_on_no_and_enter_keeps_the_source() {
        let mut s = state();
        press(&mut s, KeyCode::Char('x'));
        let Some(Modal::Remove(r)) = &s.modal else {
            panic!("a confirm opens");
        };
        assert_eq!(r.choice, 0);
        assert_eq!(r.choices()[0], RemoveChoice::Keep);
        let out = press(&mut s, KeyCode::Enter);
        assert!(out.effects.is_empty() && s.modal.is_none());
    }

    #[test]
    fn escape_and_n_cancel_the_remove_confirm() {
        for code in [KeyCode::Esc, KeyCode::Char('n')] {
            let mut s = state();
            press(&mut s, KeyCode::Delete);
            assert!(press(&mut s, code).effects.is_empty());
            assert!(s.modal.is_none());
        }
    }

    #[test]
    fn y_removes_and_keeps_the_token_while_d_forgets_it_too() {
        let mut s = state();
        press(&mut s, KeyCode::Char('x'));
        let out = press(&mut s, KeyCode::Char('y'));
        assert!(matches!(
            &out.effects[..],
            [Effect::Save { change: Change::Remove { forget_token: false, spec }, .. }] if spec.name == "work"
        ));
        press(&mut s, KeyCode::Char('x'));
        let out = press(&mut s, KeyCode::Char('d'));
        assert!(matches!(
            &out.effects[..],
            [Effect::Save {
                change: Change::Remove {
                    forget_token: true,
                    ..
                },
                ..
            }]
        ));
    }

    #[test]
    fn the_token_option_is_only_offered_for_keyring_sources_that_own_their_entry() {
        let mut s = state();
        press(&mut s, KeyCode::Char('j'));
        press(&mut s, KeyCode::Char('x'));
        press(&mut s, KeyCode::Char('d'));
        let Some(Modal::Remove(r)) = &s.modal else {
            panic!("d does nothing without the option");
        };
        assert_eq!(r.choices().len(), 2);

        let mut s = state();
        s.rows.push(row("work2", "github.com", AuthKind::Token));
        press(&mut s, KeyCode::Char('x'));
        let Some(Modal::Remove(r)) = &s.modal else {
            panic!()
        };
        assert!(r.shares_token && r.choices().len() == 2);
    }

    #[test]
    fn arrows_move_between_the_confirm_buttons() {
        let mut s = state();
        press(&mut s, KeyCode::Char('x'));
        press(&mut s, KeyCode::Right);
        press(&mut s, KeyCode::Right);
        press(&mut s, KeyCode::Left);
        let out = press(&mut s, KeyCode::Enter);
        assert!(matches!(
            &out.effects[..],
            [Effect::Save {
                change: Change::Remove {
                    forget_token: false,
                    ..
                },
                ..
            }]
        ));
    }

    #[test]
    fn a_locked_state_explains_instead_of_changing() {
        let mut s = state();
        s.editable = false;
        s.origin = Some(Origin {
            path: "/etc/xdg/review-buddy/config.toml".into(),
            label: "from /etc/xdg/review-buddy/config.toml".into(),
        });
        for key in ['e', 'a', ' ', 'x'] {
            let out = press(&mut s, KeyCode::Char(key));
            let text = out.notice.expect("explains").text;
            assert!(
                text.contains("from /etc/xdg/review-buddy/config.toml"),
                "{text}"
            );
            assert!(text.contains("/c/config.toml"));
            assert!(out.effects.is_empty() && s.modal.is_none());
        }
        assert!(
            !press(&mut s, KeyCode::Char('t')).effects.is_empty(),
            "testing a token isn't a write"
        );
    }

    #[test]
    fn demo_is_read_only_and_never_tests() {
        let mut s = State::demo(&[]);
        assert!(s.demo && s.loaded);
        for key in ['a', 'e', 'x', ' ', 't'] {
            let out = press(&mut s, KeyCode::Char(key));
            assert!(out.notice.unwrap().text.contains("(demo)"));
            assert!(out.effects.is_empty());
        }
    }

    #[test]
    fn adding_asks_for_detection_then_offers_what_it_finds() {
        let mut s = state();
        let out = press(&mut s, KeyCode::Char('a'));
        assert!(matches!(out.effects[..], [Effect::Detect]));
        let found = |host: &str| FoundHost {
            host: host.into(),
            kind: ForgeKind::GitLab,
            evidence: Vec::new(),
        };
        s.apply(Input::Detected(Detection {
            hosts: vec![found("github.com"), found("gitlab.example.org")],
            notes: Vec::new(),
        }));
        let Some(Modal::Pick(p)) = &s.modal else {
            panic!()
        };
        assert_eq!(
            p.hosts.len(),
            1,
            "a host that's already a source isn't offered"
        );
        press(&mut s, KeyCode::Enter);
        let Some(Modal::Form(f)) = &s.modal else {
            panic!("a pick opens the form");
        };
        assert_eq!(f.host.text(), "gitlab.example.org");
        assert_eq!(f.name.text(), "gitlab.example.org");
        assert_eq!(f.kind, ForgeKind::GitLab);
        assert_eq!(f.auth, AuthChoice::Token);
    }

    #[test]
    fn another_host_opens_a_blank_form_on_the_host() {
        let mut s = state();
        press(&mut s, KeyCode::Char('n'));
        press(&mut s, KeyCode::Char('1'));
        let Some(Modal::Form(f)) = &s.modal else {
            panic!()
        };
        assert_eq!(f.focus, Field::Host);
        assert!(f.host.is_blank() && !f.is_edit());
    }

    fn open_blank(s: &mut State) {
        press(s, KeyCode::Char('a'));
        press(s, KeyCode::Enter);
    }

    #[test]
    fn filling_the_form_saves_an_add_with_the_hidden_token() {
        let mut s = state();
        open_blank(&mut s);
        type_text(&mut s, "ghe.test");
        press(&mut s, KeyCode::Up);
        press(&mut s, KeyCode::Up);
        type_text(&mut s, "ghe");
        press(&mut s, KeyCode::Tab);
        press(&mut s, KeyCode::Tab);
        press(&mut s, KeyCode::Tab);
        type_text(&mut s, "https://ghe.test/api/v3");
        for _ in 0..2 {
            press(&mut s, KeyCode::Tab);
        }
        type_text(&mut s, "acme, liminal-hq");
        press(&mut s, KeyCode::Tab);
        press(&mut s, KeyCode::Char(' '));
        press(&mut s, KeyCode::Tab);
        type_text(&mut s, "ghp_secret");
        let out = press(&mut s, KeyCode::Enter);
        match &out.effects[..] {
            [Effect::Save {
                change: Change::Add { spec, token },
                ..
            }] => {
                assert_eq!(spec.name, "ghe");
                assert_eq!(spec.host, "ghe.test");
                assert_eq!(spec.api_url.as_deref(), Some("https://ghe.test/api/v3"));
                assert_eq!(spec.owners, ["acme", "liminal-hq"]);
                assert!(spec.scope_user);
                assert_eq!(spec.auth, AuthKind::Token);
                assert_eq!(token.as_ref().unwrap().expose(), "ghp_secret");
            }
            other => panic!("{other:?}"),
        }
        assert!(!format!("{:?}", s.modal).contains("ghp_secret"));
    }

    #[test]
    fn a_form_that_isnt_valid_says_what_to_fix_and_keeps_open() {
        let mut s = state();
        open_blank(&mut s);
        let out = press(&mut s, KeyCode::Enter);
        assert!(out.effects.is_empty());
        let Some(Modal::Form(f)) = &s.modal else {
            panic!()
        };
        assert!(f.error.as_ref().unwrap().contains("name"));
        let mut s = state();
        open_blank(&mut s);
        type_text(&mut s, "https://github.com");
        let out = press(&mut s, KeyCode::Enter);
        assert!(out.effects.is_empty());
        let mut s = state();
        open_blank(&mut s);
        type_text(&mut s, "github.com");
        press(&mut s, KeyCode::Up);
        press(&mut s, KeyCode::Up);
        type_text(&mut s, "WORK");
        press(&mut s, KeyCode::Enter);
        let Some(Modal::Form(f)) = &s.modal else {
            panic!()
        };
        assert!(f
            .error
            .as_ref()
            .unwrap()
            .contains("already a source called WORK"));
    }

    #[test]
    fn editing_prefills_and_saves_the_difference() {
        let mut s = state();
        press(&mut s, KeyCode::Char('e'));
        let Some(Modal::Form(f)) = &s.modal else {
            panic!()
        };
        assert!(f.is_edit() && f.name.text() == "work" && f.auth == AuthChoice::Token);
        assert!(!f.visible().contains(&Field::Kind));
        type_text(&mut s, "-2");
        let out = press(&mut s, KeyCode::Enter);
        match &out.effects[..] {
            [Effect::Save {
                change:
                    Change::Edit {
                        before,
                        after,
                        token,
                    },
                ..
            }] => {
                assert_eq!(before.name, "work");
                assert_eq!(after.name, "work-2");
                assert!(token.is_none());
            }
            other => panic!("{other:?}"),
        }
        let Some(Modal::Form(f)) = &s.modal else {
            panic!()
        };
        assert!(f.saving);
        assert!(
            press(&mut s, KeyCode::Enter).effects.is_empty(),
            "no double save"
        );
    }

    #[test]
    fn a_failed_save_keeps_the_form_with_the_reason() {
        let mut s = state();
        press(&mut s, KeyCode::Char('e'));
        press(&mut s, KeyCode::Enter);
        let out = s.apply(Input::Saved(Err("That token was rejected.".into())));
        assert!(out.notice.is_none() && !out.reload);
        let Some(Modal::Form(f)) = &s.modal else {
            panic!()
        };
        assert!(!f.saving);
        assert_eq!(f.error.as_deref(), Some("That token was rejected."));
    }

    #[test]
    fn a_saved_change_closes_the_form_reloads_and_selects_the_new_source() {
        let mut s = state();
        press(&mut s, KeyCode::Char('e'));
        press(&mut s, KeyCode::Enter);
        let out = s.apply(Input::Saved(Ok(Saved {
            message: "Saved lab.".into(),
            select: Some("lab".into()),
        })));
        assert!(out.reload && s.modal.is_none());
        assert!(matches!(out.effects[..], [Effect::Load]));
        s.apply(Input::Loaded(Ok(Snapshot {
            rows: vec![row("a", "x", AuthKind::Cli), row("lab", "y", AuthKind::Cli)],
            write_target: "/c/config.toml".into(),
            origin: None,
            editable: true,
        })));
        assert_eq!(s.cursor, 1);
    }

    #[test]
    fn a_failed_toggle_is_a_calm_warning() {
        let mut s = state();
        let out = s.apply(Input::Saved(Err("Couldn't write the file.".into())));
        assert_eq!(out.notice.unwrap().kind, NoticeKind::Warning);
    }

    #[test]
    fn pasting_goes_to_the_focused_field_on_one_line() {
        let mut s = state();
        open_blank(&mut s);
        s.apply(Input::Paste("github.com\nignored".into()));
        let Some(Modal::Form(f)) = &s.modal else {
            panic!()
        };
        assert_eq!(f.host.text(), "github.com");
    }

    #[test]
    fn auth_choices_show_their_detail_field_and_validate_it() {
        let mut f = Form::blank();
        f.host = Editor::with_text("github.com");
        f.name = Editor::with_text("gh");
        f.focus = Field::Auth;
        f.cycle(true);
        assert_eq!(f.auth, AuthChoice::Env);
        assert!(f.visible().contains(&Field::Detail));
        assert!(!f.visible().contains(&Field::Token));
        assert!(f.spec(&[]).unwrap_err().contains("environment variable"));
        f.detail = Editor::with_text("GH_TOKEN");
        assert_eq!(f.spec(&[]).unwrap().auth, AuthKind::Env("GH_TOKEN".into()));
        f.detail = Editor::new();
        f.cycle(true);
        assert!(f.spec(&[]).unwrap_err().contains("command"));
        assert!(f.token_secret().is_none());
    }

    #[test]
    fn switching_to_gitlab_drops_the_github_only_user_scope() {
        let mut f = Form::blank();
        f.user = true;
        f.focus = Field::Kind;
        f.cycle(true);
        assert_eq!(f.kind, ForgeKind::GitLab);
        assert!(!f.user && !f.visible().contains(&Field::User));
    }

    #[test]
    fn scope_and_auth_read_calmly() {
        let mut r = row("work", "github.com", AuthKind::Cli);
        assert_eq!(r.scope_summary(), "everything you can see");
        assert_eq!(r.auth_label(), "gh");
        r.spec.owners = vec!["acme".into()];
        r.spec.scope_user = true;
        assert_eq!(r.scope_summary(), "acme, you");
        r.spec.kind = ForgeKind::GitLab;
        assert_eq!(r.auth_label(), "glab");
    }

    #[test]
    fn keys_do_nothing_until_the_config_has_loaded() {
        let mut s = State::loading();
        assert!(press(&mut s, KeyCode::Char('a')).effects.is_empty());
        assert!(press(&mut s, KeyCode::Esc).back);
        let out = s.apply(Input::Loaded(Err("line 3: bad".into())));
        assert!(out.notice.unwrap().text.contains("line 3"));
        assert!(s.loaded);
    }
}
