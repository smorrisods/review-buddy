//! The pane model: an [`Emulator`] plus, in demo mode, a scripted responder.
//!
//! A pane never owns the PTY. The host spawns one with [`crate::Pty`], pushes its output through
//! [`Pane::feed`] and writes whatever [`Pane::write`] and [`Output::reply`] hand back. For a
//! scripted pane `write` is answered in-process and returns nothing for the host to send.

use crate::emulator::{Emulator, KittyFlags, Modes, Output};
use crate::screen::Screen;
use crate::script::{Script, ScriptContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// A real child in a PTY.
    Live,
    /// The in-process demo transcript.
    Scripted,
}

#[derive(Debug)]
pub struct Pane {
    emulator: Emulator,
    script: Option<Script>,
    exited: Option<Option<u32>>,
}

impl Pane {
    /// An empty pane for a real child.
    pub fn live(cols: u16, rows: u16, scrollback: usize) -> Self {
        Self {
            emulator: Emulator::new(cols, rows, scrollback),
            script: None,
            exited: None,
        }
    }

    /// A pane that replays a canned transcript and answers typing in-process.
    pub fn scripted(cols: u16, rows: u16, scrollback: usize, ctx: ScriptContext) -> Self {
        let script = Script::new(ctx);
        let mut emulator = Emulator::new(cols, rows, scrollback);
        let _ = emulator.feed(&script.greeting());
        Self {
            emulator,
            script: Some(script),
            exited: None,
        }
    }

    pub fn kind(&self) -> PaneKind {
        if self.script.is_some() {
            PaneKind::Scripted
        } else {
            PaneKind::Live
        }
    }

    pub fn emulator_mut(&mut self) -> &mut Emulator {
        &mut self.emulator
    }

    /// Feeds the child's output and returns what to send back to it.
    pub fn feed(&mut self, bytes: &[u8]) -> Output {
        self.emulator.feed(bytes)
    }

    /// Takes encoded input. Returns the bytes the host must write to the child; empty for a
    /// scripted pane, which answers itself. Typing snaps the viewport back to the live screen.
    pub fn write(&mut self, bytes: &[u8]) -> Vec<u8> {
        if bytes.is_empty() {
            return Vec::new();
        }
        self.emulator.scroll_to_bottom();
        match &mut self.script {
            Some(script) => {
                let answer = script.input(bytes);
                let _ = self.emulator.feed(&answer);
                Vec::new()
            }
            None => bytes.to_vec(),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.emulator.resize(cols, rows);
    }

    pub fn size(&self) -> (u16, u16) {
        self.emulator.size()
    }

    pub fn screen(&self) -> Screen {
        self.emulator.screen()
    }

    pub fn modes(&self) -> Modes {
        let mut modes = self.emulator.modes();
        if self.script.is_some() {
            modes.kitty = KittyFlags::default();
        }
        modes
    }

    pub fn title(&self) -> Option<&str> {
        self.emulator.title()
    }

    /// Records that the child ended and tells the person, in the pane itself.
    pub fn mark_exited(&mut self, code: Option<u32>) {
        if self.exited.is_some() {
            return;
        }
        self.exited = Some(code);
        let note = match code {
            Some(0) => "\r\n\x1b[2m[process finished]\x1b[0m\r\n".to_string(),
            Some(n) => format!("\r\n\x1b[2m[process exited with status {n}]\x1b[0m\r\n"),
            None => "\r\n\x1b[2m[process ended]\x1b[0m\r\n".to_string(),
        };
        let _ = self.emulator.feed(note.as_bytes());
    }

    /// `Some(status)` once the child has ended.
    pub fn exited(&self) -> Option<Option<u32>> {
        self.exited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ScriptContext {
        ScriptContext {
            source: "s".into(),
            repo: "a/b".into(),
            number: 1,
            url: "u".into(),
        }
    }

    #[test]
    fn a_live_pane_hands_input_to_the_host_untouched() {
        let mut p = Pane::live(20, 5, 10);
        assert_eq!(p.kind(), PaneKind::Live);
        assert_eq!(p.write(b"ls\r"), b"ls\r");
        assert!(p.write(b"").is_empty());
    }

    #[test]
    fn a_scripted_pane_answers_itself_and_sends_nothing() {
        let mut p = Pane::scripted(60, 12, 10, ctx());
        assert_eq!(p.kind(), PaneKind::Scripted);
        assert!(p.write(b"env\r").is_empty());
        assert!(p.screen().text().contains("RB_REPO=a/b"));
    }

    #[test]
    fn typing_snaps_the_viewport_back_to_the_bottom() {
        let mut p = Pane::live(10, 3, 50);
        for n in 0..12 {
            p.feed(format!("l{n}\r\n").as_bytes());
        }
        p.emulator_mut().scroll(5);
        assert_eq!(p.screen().scrolled, 5);
        p.write(b"x");
        assert_eq!(p.screen().scrolled, 0);
    }

    #[test]
    fn exit_is_noted_once_in_the_pane() {
        let mut p = Pane::live(40, 4, 10);
        p.mark_exited(Some(2));
        p.mark_exited(Some(0));
        assert_eq!(p.exited(), Some(Some(2)));
        assert!(p.screen().text().contains("process exited with status 2"));
    }

    #[test]
    fn a_scripted_pane_never_enables_kitty_keys() {
        let mut p = Pane::scripted(40, 6, 10, ctx());
        p.feed(b"\x1b[>1u");
        assert!(!p.modes().kitty.any());
    }
}
