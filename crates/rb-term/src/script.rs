//! The scripted pane demo mode uses: a canned, shell-like transcript that answers a few commands.
//!
//! It runs in-process. It never spawns a process, reads a file or opens a network connection, so
//! demo mode stays offline and untouched even when the pane is open.

/// The change the pane was opened for, shown in the transcript and echoed back by `env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptContext {
    pub source: String,
    pub repo: String,
    pub number: u64,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Script {
    ctx: ScriptContext,
    line: String,
    skipping: Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Skip {
    None,
    Escape,
    Csi,
}

const CRLF: &str = "\r\n";

impl Script {
    pub fn new(ctx: ScriptContext) -> Self {
        Self {
            ctx,
            line: String::new(),
            skipping: Skip::None,
        }
    }

    fn prompt(&self) -> String {
        let repo = self.ctx.repo.rsplit('/').next().unwrap_or("repo");
        format!("\x1b[32mdemo\x1b[0m:\x1b[34m~/review/{repo}\x1b[0m$ ")
    }

    /// The opening transcript, ending at a fresh prompt.
    pub fn greeting(&self) -> Vec<u8> {
        let mut out = String::new();
        out.push_str("\x1b[2m(demo) This pane is scripted. No shell is running and nothing here touches your files.\x1b[0m");
        out.push_str(CRLF);
        out.push_str(&self.prompt());
        out.push_str("git status -sb");
        out.push_str(CRLF);
        out.push_str(&self.run("git status -sb"));
        out.push_str(&self.prompt());
        out.push_str("echo $RB_REPO#$RB_NUMBER");
        out.push_str(CRLF);
        out.push_str(&self.run("echo $RB_REPO#$RB_NUMBER"));
        out.push_str(&self.prompt());
        out.into_bytes()
    }

    /// Takes what the person typed and returns what the "shell" prints in answer, to be fed to the
    /// emulator.
    pub fn input(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = String::new();
        let text = String::from_utf8_lossy(bytes);
        for ch in text.chars() {
            match self.skipping {
                Skip::Escape => {
                    self.skipping = if ch == '[' || ch == 'O' {
                        Skip::Csi
                    } else {
                        Skip::None
                    };
                    continue;
                }
                Skip::Csi => {
                    if ('@'..='~').contains(&ch) {
                        self.skipping = Skip::None;
                    }
                    continue;
                }
                Skip::None => {}
            }
            match ch {
                '\x1b' => self.skipping = Skip::Escape,
                '\r' | '\n' => {
                    out.push_str(CRLF);
                    let line = std::mem::take(&mut self.line);
                    out.push_str(&self.run(line.trim()));
                    out.push_str(&self.prompt());
                }
                '\x7f' | '\x08' => {
                    if self.line.pop().is_some() {
                        out.push_str("\x08 \x08");
                    }
                }
                '\x03' => {
                    self.line.clear();
                    out.push_str("^C");
                    out.push_str(CRLF);
                    out.push_str(&self.prompt());
                }
                '\x0c' => {
                    out.push_str("\x1b[2J\x1b[H");
                    out.push_str(&self.prompt());
                    out.push_str(&self.line);
                }
                '\x15' => {
                    for _ in 0..self.line.chars().count() {
                        out.push_str("\x08 \x08");
                    }
                    self.line.clear();
                }
                c if !c.is_control() => {
                    self.line.push(c);
                    out.push(c);
                }
                _ => {}
            }
        }
        out.into_bytes()
    }

    fn run(&self, line: &str) -> String {
        let c = &self.ctx;
        let lines: Vec<String> = match line {
            "" => Vec::new(),
            "help" => vec![
                "(demo) A few commands are scripted: help, ls, git status, git log, env, echo $RB_URL, clear.".into(),
                "The real pane starts your shell here, in a checkout of the change.".into(),
            ],
            "ls" => vec!["Cargo.toml  README.md  docs  src  tests".into()],
            "git status" | "git status -sb" => vec![
                format!("## review/{}-{}...origin/main", c.number, "head"),
                " M src/lib.rs".into(),
            ],
            "git log" | "git log --oneline" => vec![
                "\x1b[33me3b0c44\x1b[0m Tighten the retry loop".into(),
                "\x1b[33m9f2a7d1\x1b[0m Add a test for the empty case".into(),
                "\x1b[33m41c6b02\x1b[0m Start on the retry changes".into(),
            ],
            "env" | "env | grep RB_" => vec![
                format!("RB_SOURCE={}", c.source),
                format!("RB_REPO={}", c.repo),
                format!("RB_NUMBER={}", c.number),
                format!("RB_URL={}", c.url),
            ],
            "echo $RB_URL" => vec![c.url.clone()],
            "echo $RB_REPO#$RB_NUMBER" => vec![format!("{}#{}", c.repo, c.number)],
            "clear" => return "\x1b[2J\x1b[H".to_string(),
            "exit" | "logout" => vec![
                "(demo) There is nothing to exit here. Press the escape chord, then esc, to go back to the app.".into(),
            ],
            other => {
                let name = other.split_whitespace().next().unwrap_or(other);
                match name {
                    "claude" | "opencode" | "lazygit" | "vim" | "nvim" | "less" => vec![format!(
                        "(demo) {name} isn't run in demo mode. Outside demo it starts here, next to the change."
                    )],
                    _ => vec![format!(
                        "{name}: command not found (demo: only a few commands are scripted; try help)"
                    )],
                }
            }
        };
        let mut out = String::new();
        for l in lines {
            out.push_str(&l);
            out.push_str(CRLF);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::Emulator;

    fn ctx() -> ScriptContext {
        ScriptContext {
            source: "demo-gh".into(),
            repo: "acme/widgets".into(),
            number: 214,
            url: "https://example.test/acme/widgets/pull/214".into(),
        }
    }

    fn run(script: &mut Script, typed: &str) -> String {
        let mut e = Emulator::new(80, 24, 100);
        e.feed(&script.input(typed.as_bytes()));
        e.screen().text()
    }

    #[test]
    fn the_greeting_ends_at_a_prompt_and_says_it_is_scripted() {
        let s = Script::new(ctx());
        let mut e = Emulator::new(100, 24, 100);
        e.feed(&s.greeting());
        let text = e.screen().text();
        assert!(text.contains("(demo) This pane is scripted"));
        assert!(text.contains("acme/widgets#214"));
        assert!(text.contains("demo:~/review/widgets$"));
    }

    #[test]
    fn typing_echoes_and_enter_runs_the_command() {
        let mut s = Script::new(ctx());
        let text = run(&mut s, "env\r");
        assert!(text.contains("env"));
        assert!(text.contains("RB_SOURCE=demo-gh"));
        assert!(text.contains("RB_NUMBER=214"));
        assert!(text.contains("RB_URL=https://example.test/acme/widgets/pull/214"));
    }

    #[test]
    fn backspace_edits_the_line_and_arrows_are_ignored() {
        let mut s = Script::new(ctx());
        let mut e = Emulator::new(80, 24, 100);
        e.feed(&s.input(b"lx\x7fs\x1b[A\x1b[D"));
        e.feed(&s.input(b"\r"));
        assert!(e.screen().text().contains("Cargo.toml"));
    }

    #[test]
    fn unknown_and_assistant_commands_explain_themselves() {
        let mut s = Script::new(ctx());
        assert!(run(&mut s, "frobnicate\r").contains("frobnicate: command not found (demo"));
        assert!(run(&mut s, "claude\r").contains("claude isn't run in demo mode"));
    }

    #[test]
    fn ctrl_c_and_ctrl_l_behave_like_a_shell() {
        let mut s = Script::new(ctx());
        let mut e = Emulator::new(80, 24, 100);
        e.feed(&s.input(b"partial\x03"));
        assert!(e.screen().text().contains("^C"));
        e.feed(&s.input(b"\x0c"));
        let text = e.screen().text();
        assert!(!text.contains("partial"));
        assert!(text.starts_with("demo:~/review/widgets$"));
    }

    #[test]
    fn exit_is_explained_and_does_not_close_anything() {
        let mut s = Script::new(ctx());
        assert!(run(&mut s, "exit\r").contains("nothing to exit"));
    }
}
