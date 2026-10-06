//! The first-run flow as line-based prompts, for pipes, `--plain` and terminals that can't draw.
//!
//! It drives the same [`Flow`] as the full-screen version: each answer becomes an [`Input`],
//! each [`Effect`] runs through [`run_effect`], and its result goes back in as another input.
//! Reading and writing are injected, and so is the way a token is read, so tests script a whole
//! run. Confirmations that could lose something default to No.

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use rb_core::ForgeKind;
use rb_theme::BUILTIN_IDS;

use super::effects::{run_effect, Services};
use super::flow::{Click, Conn, Flow, HostRow, Input, Outcome, ScopeItem, Step};

/// How a run ended.
#[derive(Debug)]
pub struct Run {
    pub flow: Flow,
    pub outcome: Outcome,
    /// Why the config couldn't be written, when that is what stopped the run.
    pub failure: Option<String>,
}

const THEME_NAMES: [&str; 4] = ["Liminal HQ", "Dusk", "Afterglow Dark", "Afterglow Light"];

/// Runs the prompts to the end. `Outcome::Skipped` also covers the input ending early.
pub fn run(
    mut flow: Flow,
    services: &Arc<Services>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    hidden_tokens: bool,
) -> io::Result<Run> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut driver = Driver {
        services,
        runtime,
        input,
        out,
        hidden_tokens,
        failure: None,
    };
    writeln!(driver.out, "Review Buddy setup")?;
    writeln!(
        driver.out,
        "Let's connect your accounts. It takes about a minute, and nothing is written until you say so."
    )?;
    writeln!(
        driver.out,
        "Type skip at any prompt to stop without changing anything.\n"
    )?;
    writeln!(driver.out, "Looking for accounts you already use…")?;
    let effects = flow.start();
    driver.pump(&mut flow, effects);
    for note in flow.notes.clone() {
        writeln!(driver.out, "  {note}")?;
    }
    flow.apply(Input::Next);
    let outcome = driver.steps(&mut flow)?;
    Ok(Run {
        flow,
        outcome,
        failure: driver.failure.take(),
    })
}

struct Driver<'a> {
    services: &'a Arc<Services>,
    runtime: tokio::runtime::Runtime,
    input: &'a mut dyn BufRead,
    out: &'a mut dyn Write,
    hidden_tokens: bool,
    failure: Option<String>,
}

/// What a prompt got back.
enum Answer {
    Line(String),
    Skip,
}

impl Driver<'_> {
    /// Applies `effects`, then everything they lead to.
    fn pump(&mut self, flow: &mut Flow, effects: Vec<super::flow::Effect>) {
        let mut queue = effects;
        while let Some(effect) = queue.pop() {
            let result = self.runtime.block_on(run_effect(self.services, effect));
            queue.extend(flow.apply(result));
        }
    }

    fn apply(&mut self, flow: &mut Flow, input: Input) {
        let effects = flow.apply(input);
        self.pump(flow, effects);
    }

    fn ask(&mut self, prompt: &str) -> io::Result<Answer> {
        write!(self.out, "{prompt} ")?;
        self.out.flush()?;
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 {
            writeln!(self.out)?;
            return Ok(Answer::Skip);
        }
        let line = line.trim().to_string();
        if matches!(line.to_ascii_lowercase().as_str(), "skip" | "q" | "quit") {
            return Ok(Answer::Skip);
        }
        Ok(Answer::Line(line))
    }

    fn read_token(&mut self, prompt: &str) -> io::Result<String> {
        if self.hidden_tokens {
            return hidden_reader(prompt);
        }
        write!(self.out, "{prompt} ")?;
        self.out.flush()?;
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 {
            return Ok("skip".to_string());
        }
        Ok(line)
    }

    fn steps(&mut self, flow: &mut Flow) -> io::Result<Outcome> {
        loop {
            let proceed = match flow.step {
                Step::Welcome | Step::Done => true,
                Step::Connect => self.connect(flow)?,
                Step::Scope => self.scope(flow)?,
                Step::Look => self.look(flow)?,
                Step::Jax => self.jax(flow)?,
                Step::Summary => self.summary(flow)?,
            };
            if !proceed {
                return Ok(Outcome::Skipped);
            }
            if let Some(outcome) = flow.outcome.clone() {
                return Ok(outcome);
            }
        }
    }

    fn connect(&mut self, flow: &mut Flow) -> io::Result<bool> {
        if flow.hosts.is_empty() {
            writeln!(
                self.out,
                "\nI didn't find a GitHub or GitLab account to connect. Sign in with `gh auth login` or `glab auth login`, then run this again, or add a [[source]] to config.toml by hand."
            )?;
            return Ok(false);
        }
        writeln!(self.out, "\nAccounts and hosts I found:")?;
        for (i, row) in flow.hosts.iter().enumerate() {
            writeln!(self.out, "  {}. {}", i + 1, describe(row))?;
        }
        let prompt = "Which should I connect? Numbers like 1,3 (enter keeps the ticked ones):";
        let Answer::Line(line) = self.ask(prompt)? else {
            return Ok(false);
        };
        if !line.is_empty() {
            let wanted = numbers(&line, flow.hosts.len());
            for i in 0..flow.hosts.len() {
                if wanted.contains(&i) != flow.hosts[i].selected {
                    self.apply(flow, Input::Click(Click::Row(i)));
                    if !self.token_prompt(flow, i)? {
                        return Ok(false);
                    }
                }
            }
        }
        for i in 0..flow.hosts.len() {
            if flow.hosts[i].selected && flow.hosts[i].needs_token() {
                flow.cursor = i;
                self.apply(flow, Input::Next);
                if !self.token_prompt(flow, i)? {
                    return Ok(false);
                }
            }
        }
        self.apply(flow, Input::Next);
        if flow.step == Step::Connect {
            let message = flow.message.take().unwrap_or_default();
            writeln!(self.out, "{message}")?;
            return self.connect_again(flow);
        }
        Ok(true)
    }

    fn connect_again(&mut self, flow: &mut Flow) -> io::Result<bool> {
        let Answer::Line(line) = self.ask("Try again? [y/N]")? else {
            return Ok(false);
        };
        Ok(matches!(line.to_ascii_lowercase().as_str(), "y" | "yes") && self.connect(flow)?)
    }

    /// Asks for a token while the flow has its field open. False means stop altogether.
    fn token_prompt(&mut self, flow: &mut Flow, i: usize) -> io::Result<bool> {
        while flow.token_open {
            let row = &flow.hosts[i];
            let scopes = super::flow::required_scopes(row.kind).join(", ");
            let prompt = format!(
                "Token for {} (needs {scopes}). Paste it, or press enter to leave this host out:",
                row.host
            );
            let token = self.read_token(&prompt)?;
            let token = token.trim();
            if token.eq_ignore_ascii_case("skip") {
                return Ok(false);
            }
            if token.is_empty() {
                self.apply(flow, Input::Skip);
                break;
            }
            self.apply(flow, Input::Paste(token.to_string()));
            self.apply(flow, Input::Next);
            let row = &flow.hosts[i];
            match &row.conn {
                Conn::Connected(_) => writeln!(self.out, "  {}", describe(row))?,
                Conn::Failed(reason) => writeln!(self.out, "  {reason}")?,
                _ => {}
            }
        }
        Ok(true)
    }

    fn scope(&mut self, flow: &mut Flow) -> io::Result<bool> {
        let items = flow.scope_items();
        if items.is_empty() {
            writeln!(
                self.out,
                "\nEverything you can see on each host will be included."
            )?;
            self.apply(flow, Input::Next);
            return Ok(true);
        }
        writeln!(self.out, "\nWhat should each account include?")?;
        for (n, item) in items.iter().enumerate() {
            writeln!(self.out, "  {}. {}", n + 1, scope_label(flow, *item))?;
        }
        writeln!(
            self.out,
            "Leave them all out to include everything you can see."
        )?;
        let Answer::Line(line) = self.ask("Numbers like 1,2 (enter for everything):")? else {
            return Ok(false);
        };
        let wanted = numbers(&line, items.len());
        for (n, _) in items.iter().enumerate() {
            if wanted.contains(&n) {
                self.apply(flow, Input::Click(Click::Row(n)));
            }
        }
        self.apply(flow, Input::Next);
        Ok(true)
    }

    fn look(&mut self, flow: &mut Flow) -> io::Result<bool> {
        writeln!(self.out, "\nPick a look:")?;
        for (i, name) in THEME_NAMES.iter().enumerate() {
            let mark = if flow.theme == i { "›" } else { " " };
            writeln!(self.out, " {mark} {}. {name}", i + 1)?;
        }
        let Answer::Line(line) = self.ask("Number (enter keeps the current one):")? else {
            return Ok(false);
        };
        if let Some(i) = numbers(&line, BUILTIN_IDS.len()).first() {
            self.apply(flow, Input::Click(Click::Theme(*i)));
        }
        self.apply(flow, Input::Next);
        Ok(true)
    }

    fn jax(&mut self, flow: &mut Flow) -> io::Result<bool> {
        let Answer::Line(line) = self.ask("\nKeep Jax the otter around? [Y/n]")? else {
            return Ok(false);
        };
        let keep = !matches!(line.to_ascii_lowercase().as_str(), "n" | "no");
        if keep != flow.jax {
            self.apply(flow, Input::Click(Click::Jax));
        }
        self.apply(flow, Input::Next);
        Ok(true)
    }

    fn summary(&mut self, flow: &mut Flow) -> io::Result<bool> {
        writeln!(self.out, "\nHere's what I'll set up:")?;
        for (_, row) in flow.chosen() {
            writeln!(self.out, "  {} · {}", row.host, scope_summary(row))?;
        }
        writeln!(
            self.out,
            "  Look: {} · Jax: {}",
            THEME_NAMES[flow.theme],
            if flow.jax { "around" } else { "away" }
        )?;
        writeln!(self.out, "  Config: {}", flow.target.display())?;
        if flow.existing {
            writeln!(
                self.out,
                "A config file is already there. If you go ahead, a copy of it is kept as config.toml.bak."
            )?;
            self.apply(flow, Input::Next);
            let Answer::Line(line) = self.ask("Replace it? [y/N]")? else {
                return Ok(false);
            };
            let yes = matches!(line.to_ascii_lowercase().as_str(), "y" | "yes");
            self.apply(flow, Input::Confirm(yes));
            if !yes {
                writeln!(self.out, "Nothing was changed.")?;
                return Ok(false);
            }
        } else {
            let Answer::Line(line) = self.ask("Save it? [Y/n]")? else {
                return Ok(false);
            };
            if matches!(line.to_ascii_lowercase().as_str(), "n" | "no") {
                writeln!(self.out, "Nothing was changed.")?;
                return Ok(false);
            }
            self.apply(flow, Input::Next);
        }
        if flow.step == Step::Summary {
            let message = flow.message.take().unwrap_or_default();
            writeln!(self.out, "{message}")?;
            self.failure = Some(message);
            return Ok(false);
        }
        Ok(true)
    }
}

fn describe(row: &HostRow) -> String {
    let tag = match row.kind {
        ForgeKind::GitHub => "GH",
        ForgeKind::GitLab => "GL",
    };
    let (glyph, text) = row.status();
    let mut out = format!(
        "{} {glyph} {tag} {}  {text}",
        if row.selected { "[x]" } else { "[ ]" },
        row.host
    );
    if let Some(info) = row.connected() {
        for hint in &info.hints {
            out.push_str(&format!("\n         {hint}"));
        }
    }
    out
}

fn scope_label(flow: &Flow, item: ScopeItem) -> String {
    match item {
        ScopeItem::User(h) => format!("{} · my own repositories", flow.hosts[h].host),
        ScopeItem::Org(h, o) => {
            let noun = match flow.hosts[h].kind {
                ForgeKind::GitHub => "organisation",
                ForgeKind::GitLab => "group",
            };
            format!(
                "{} · {noun} {}",
                flow.hosts[h].host, flow.hosts[h].orgs[o].0
            )
        }
    }
}

pub fn scope_summary(row: &HostRow) -> String {
    let mut parts = Vec::new();
    if row.user {
        parts.push("my repositories".to_string());
    }
    parts.extend(
        row.orgs
            .iter()
            .filter(|(_, on)| *on)
            .map(|(o, _)| o.clone()),
    );
    if parts.is_empty() {
        "everything you can see".to_string()
    } else {
        parts.join(", ")
    }
}

/// Zero-based indexes from `1,3` or `1 3`; anything out of range or unreadable is ignored.
fn numbers(text: &str, len: usize) -> Vec<usize> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|n| n.parse::<usize>().ok())
        .filter(|n| (1..=len).contains(n))
        .map(|n| n - 1)
        .collect()
}

/// A secret reader for a real terminal: raw mode, no echo. Falls back to a plain line when
/// raw mode isn't available.
pub fn hidden_reader(prompt: &str) -> io::Result<String> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    let mut stdout = io::stdout();
    write!(stdout, "{prompt} ")?;
    stdout.flush()?;
    if crossterm::terminal::enable_raw_mode().is_err() {
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        return Ok(line);
    }
    let mut text = String::new();
    let result = loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    text = "skip".to_string();
                    break Ok(());
                }
                KeyCode::Char(c) => text.push(c),
                _ => {}
            },
            Ok(Event::Paste(pasted)) => text.push_str(&pasted),
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };
    let _ = crossterm::terminal::disable_raw_mode();
    writeln!(stdout)?;
    result.map(|()| text)
}
