//! The focus model for leaving the pane.
//!
//! While the pane has focus every key belongs to the child, so there has to be a way out that the
//! child doesn't use. The main way is a chord (default `Ctrl-\`) followed by a command key, `Esc` to
//! return focus to the app. Where a terminal or multiplexer swallows the chord there is a second
//! way: pressing `Esc` twice in quick succession. The first `Esc` still reaches the child, so
//! editors keep working.
//!
//! Everything here is pure; time is a tick counter the caller advances.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// How long a chord waits for its second key, in ticks.
pub const ARM_TICKS: u64 = 8;
/// How close two `Esc` presses must be to count as a double press, in ticks.
pub const DOUBLE_ESC_TICKS: u64 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordError(pub String);

impl fmt::Display for ChordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ChordError {}

/// A key with modifiers, such as `ctrl-\`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Default for Chord {
    fn default() -> Self {
        Self {
            code: KeyCode::Char('\\'),
            mods: KeyModifiers::CONTROL,
        }
    }
}

impl Chord {
    /// Parses `ctrl-\`, `ctrl-]`, `alt-x`, `ctrl-alt-space`, `f12`. A chord needs Ctrl or Alt (or
    /// be a function key), so it can never swallow ordinary typing.
    pub fn parse(text: &str) -> Result<Self, ChordError> {
        let bad = |why: &str| {
            ChordError(format!(
                "`{text}` isn't a usable escape chord: {why}. Try ctrl-\\ or ctrl-]"
            ))
        };
        let mut mods = KeyModifiers::NONE;
        let mut rest = text.trim();
        loop {
            let lower = rest.to_ascii_lowercase();
            let (flag, len) = if lower.starts_with("ctrl-") {
                (KeyModifiers::CONTROL, 5)
            } else if lower.starts_with("alt-") {
                (KeyModifiers::ALT, 4)
            } else if lower.starts_with("shift-") {
                (KeyModifiers::SHIFT, 6)
            } else {
                break;
            };
            mods |= flag;
            rest = &rest[len..];
        }
        let code = match rest.to_ascii_lowercase().as_str() {
            "" => return Err(bad("no key after the modifiers")),
            "esc" => KeyCode::Esc,
            "enter" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "space" => KeyCode::Char(' '),
            f if f.starts_with('f')
                && f.len() > 1
                && f[1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                match f[1..].parse::<u8>() {
                    Ok(n @ 1..=24) => KeyCode::F(n),
                    _ => return Err(bad("function keys run from f1 to f24")),
                }
            }
            _ => {
                let mut chars = rest.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => KeyCode::Char(c.to_ascii_lowercase()),
                    _ => return Err(bad("expected one key")),
                }
            }
        };
        let has_base = mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if !has_base && !matches!(code, KeyCode::F(_)) {
            return Err(bad("it needs ctrl or alt"));
        }
        Ok(Self { code, mods })
    }

    /// Whether `key` is this chord. A terminal that sends `Ctrl-\` as the byte 0x1c reports it as
    /// `Ctrl-4`; those spellings are treated as the same key.
    pub fn matches(&self, key: &KeyEvent) -> bool {
        let wanted = self.mods & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let got = key.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let ctrl = self.mods.contains(KeyModifiers::CONTROL);
        match (self.code, key.code) {
            (KeyCode::Char(a), KeyCode::Char(b)) if ctrl && got.contains(KeyModifiers::CONTROL) => {
                let same_key = match (control_code(a), control_code(b)) {
                    (Some(x), Some(y)) => x == y,
                    _ => a.eq_ignore_ascii_case(&b),
                };
                same_key && (wanted - KeyModifiers::SHIFT) == (got - KeyModifiers::SHIFT)
            }
            (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b) && wanted == got,
            (a, b) => a == b && wanted == got,
        }
    }

    /// The chord as people write it on screen: `⌃\`, `⌥x`.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            out.push('⌃');
        }
        if self.mods.contains(KeyModifiers::ALT) {
            out.push('⌥');
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            out.push('⇧');
        }
        match self.code {
            KeyCode::Char(' ') => out.push_str("space"),
            KeyCode::Char(c) => out.push(c),
            KeyCode::Esc => out.push_str("esc"),
            KeyCode::Enter => out.push('⏎'),
            KeyCode::Tab => out.push_str("tab"),
            KeyCode::F(n) => out.push_str(&format!("F{n}")),
            other => out.push_str(&format!("{other:?}")),
        }
        out
    }
}

fn control_code(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' => 0x1f,
        _ => return None,
    })
}

/// What a key means while the pane has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeAction {
    /// Not ours: encode it and send it to the child.
    Forward,
    /// Ours, and nothing else to do (the chord was pressed, or a command key was unknown).
    Swallow,
    /// Give focus back to the app.
    Leave,
    /// Hide the pane. The child keeps running.
    Hide,
    /// Close the pane (and stop the child).
    Close,
    /// Move the pane to the next placement.
    CyclePlacement,
    Grow,
    Shrink,
    ResetSize,
    /// Send the chord's own key to the child (the chord pressed twice).
    SendChord,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EscapeState {
    armed: Option<u64>,
    last_esc: Option<u64>,
}

impl EscapeState {
    /// Whether the chord has been pressed and a command key is awaited.
    pub fn is_armed(&self) -> bool {
        self.armed.is_some()
    }

    /// Forgets a half-finished chord that has waited too long. Call on every tick.
    pub fn tick(&mut self, now: u64) {
        if self
            .armed
            .is_some_and(|at| now.saturating_sub(at) > ARM_TICKS)
        {
            self.armed = None;
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn on_key(&mut self, key: &KeyEvent, chord: &Chord, now: u64) -> EscapeAction {
        self.tick(now);
        if key.kind == KeyEventKind::Release {
            return if chord.matches(key) {
                EscapeAction::Swallow
            } else {
                EscapeAction::Forward
            };
        }
        if self.armed.take().is_some() {
            self.last_esc = None;
            if chord.matches(key) {
                return EscapeAction::SendChord;
            }
            let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
            return match key.code {
                KeyCode::Esc => EscapeAction::Leave,
                KeyCode::Char('t') if plain => EscapeAction::Hide,
                KeyCode::Char('x') if plain => EscapeAction::Close,
                KeyCode::Char('p') if plain => EscapeAction::CyclePlacement,
                KeyCode::Char('>') | KeyCode::Char('+') => EscapeAction::Grow,
                KeyCode::Char('<') | KeyCode::Char('-') => EscapeAction::Shrink,
                KeyCode::Char('=') if plain => EscapeAction::ResetSize,
                _ => EscapeAction::Swallow,
            };
        }
        if chord.matches(key) {
            self.armed = Some(now);
            self.last_esc = None;
            return EscapeAction::Swallow;
        }
        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            if self
                .last_esc
                .is_some_and(|at| now.saturating_sub(at) <= DOUBLE_ESC_TICKS)
            {
                self.last_esc = None;
                return EscapeAction::Leave;
            }
            self.last_esc = Some(now);
            return EscapeAction::Forward;
        }
        self.last_esc = None;
        EscapeAction::Forward
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn ctrl_backslash() -> KeyEvent {
        k(KeyCode::Char('\\'), KeyModifiers::CONTROL)
    }

    fn esc() -> KeyEvent {
        k(KeyCode::Esc, KeyModifiers::NONE)
    }

    #[test]
    fn parses_chords() {
        let c = Chord::parse("ctrl-\\").unwrap();
        assert_eq!(c, Chord::default());
        assert_eq!(Chord::parse("Ctrl-]").unwrap().code, KeyCode::Char(']'));
        let alt = Chord::parse("ctrl-alt-space").unwrap();
        assert_eq!(alt.mods, KeyModifiers::CONTROL | KeyModifiers::ALT);
        assert_eq!(alt.code, KeyCode::Char(' '));
        assert_eq!(Chord::parse("f12").unwrap().code, KeyCode::F(12));
        assert_eq!(Chord::default().label(), "⌃\\");
    }

    #[test]
    fn rejects_chords_that_would_eat_typing() {
        for bad in ["", "a", "ctrl-", "ctrl-ab", "shift-a", "f99", "esc"] {
            let err = Chord::parse(bad).unwrap_err();
            assert!(err.0.contains("escape chord"), "{bad}: {err}");
        }
    }

    #[test]
    fn a_legacy_terminal_reports_ctrl_backslash_as_ctrl_4() {
        let chord = Chord::default();
        assert!(chord.matches(&k(KeyCode::Char('4'), KeyModifiers::CONTROL)));
        assert!(chord.matches(&ctrl_backslash()));
        assert!(!chord.matches(&k(KeyCode::Char('\\'), KeyModifiers::NONE)));
        assert!(!chord.matches(&k(
            KeyCode::Char('\\'),
            KeyModifiers::CONTROL | KeyModifiers::ALT
        )));
        assert!(!chord.matches(&k(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn chord_then_esc_leaves() {
        let mut s = EscapeState::default();
        let c = Chord::default();
        assert_eq!(s.on_key(&ctrl_backslash(), &c, 0), EscapeAction::Swallow);
        assert!(s.is_armed());
        assert_eq!(s.on_key(&esc(), &c, 1), EscapeAction::Leave);
        assert!(!s.is_armed());
    }

    #[test]
    fn chord_commands() {
        let c = Chord::default();
        let run = |key: KeyEvent| {
            let mut s = EscapeState::default();
            s.on_key(&ctrl_backslash(), &c, 0);
            s.on_key(&key, &c, 1)
        };
        let plain = |ch| k(KeyCode::Char(ch), KeyModifiers::NONE);
        assert_eq!(run(plain('t')), EscapeAction::Hide);
        assert_eq!(run(plain('x')), EscapeAction::Close);
        assert_eq!(run(plain('p')), EscapeAction::CyclePlacement);
        assert_eq!(run(plain('>')), EscapeAction::Grow);
        assert_eq!(run(plain('<')), EscapeAction::Shrink);
        assert_eq!(run(plain('=')), EscapeAction::ResetSize);
        assert_eq!(run(plain('q')), EscapeAction::Swallow);
        assert_eq!(run(ctrl_backslash()), EscapeAction::SendChord);
    }

    #[test]
    fn an_armed_chord_expires() {
        let mut s = EscapeState::default();
        let c = Chord::default();
        s.on_key(&ctrl_backslash(), &c, 0);
        assert_eq!(s.on_key(&esc(), &c, ARM_TICKS + 5), EscapeAction::Forward);
        assert!(!s.is_armed());
    }

    #[test]
    fn double_escape_leaves_and_the_first_still_reaches_the_child() {
        let mut s = EscapeState::default();
        let c = Chord::default();
        assert_eq!(s.on_key(&esc(), &c, 10), EscapeAction::Forward);
        assert_eq!(s.on_key(&esc(), &c, 11), EscapeAction::Leave);
        assert_eq!(
            s.on_key(&esc(), &c, 12),
            EscapeAction::Forward,
            "counting starts over"
        );
    }

    #[test]
    fn slow_escapes_and_interleaved_keys_do_not_count() {
        let mut s = EscapeState::default();
        let c = Chord::default();
        s.on_key(&esc(), &c, 10);
        assert_eq!(
            s.on_key(&esc(), &c, 10 + DOUBLE_ESC_TICKS + 1),
            EscapeAction::Forward
        );
        s.on_key(&k(KeyCode::Char('i'), KeyModifiers::NONE), &c, 20);
        s.on_key(&esc(), &c, 21);
        s.on_key(&k(KeyCode::Char('j'), KeyModifiers::NONE), &c, 21);
        assert_eq!(s.on_key(&esc(), &c, 22), EscapeAction::Forward);
    }

    #[test]
    fn ordinary_keys_pass_through() {
        let mut s = EscapeState::default();
        let c = Chord::default();
        for key in [
            k(KeyCode::Char('a'), KeyModifiers::NONE),
            k(KeyCode::Char('c'), KeyModifiers::CONTROL),
            k(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            assert_eq!(s.on_key(&key, &c, 0), EscapeAction::Forward);
        }
    }

    #[test]
    fn a_release_of_the_chord_does_not_disarm_it() {
        let mut s = EscapeState::default();
        let c = Chord::default();
        s.on_key(&ctrl_backslash(), &c, 0);
        let mut up = ctrl_backslash();
        up.kind = KeyEventKind::Release;
        assert_eq!(s.on_key(&up, &c, 0), EscapeAction::Swallow);
        assert!(s.is_armed());
    }
}
