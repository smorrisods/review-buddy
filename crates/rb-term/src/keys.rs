//! Turns crossterm key events into the bytes a child expects.
//!
//! Two encodings exist. The legacy one is what every terminal child understands. The kitty keyboard
//! protocol is used only when both sides want it: the child switched it on (its flags are in
//! [`KeyContext::kitty`]) and the host terminal reported that it can tell the keys apart
//! ([`KeyContext::host_kitty`]). A host that can't, such as a plain xterm, would hand us the same
//! event for `Ctrl-I` and `Tab`, so claiming kitty keys there would only lose information.
//!
//! Supported kitty flags: disambiguate (1), event types (2), all keys as escape codes (8) and
//! associated text (16). Alternate-key reporting (4) is accepted but not sent, because crossterm
//! doesn't carry the unshifted or base-layout key.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::emulator::{KittyFlags, Modes};

/// What the encoder needs to know about the child and the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyContext {
    /// Cursor keys send `ESC O A` instead of `ESC [ A` (DECCKM).
    pub app_cursor: bool,
    pub app_keypad: bool,
    /// The kitty flags the child has enabled.
    pub kitty: KittyFlags,
    /// The host terminal reports keys with enough detail for the kitty protocol.
    pub host_kitty: bool,
}

impl KeyContext {
    pub fn new(modes: &Modes, host_kitty: bool) -> Self {
        Self {
            app_cursor: modes.app_cursor,
            app_keypad: modes.app_keypad,
            kitty: modes.kitty,
            host_kitty,
        }
    }

    fn kitty_on(&self) -> bool {
        self.host_kitty && self.kitty.any()
    }
}

/// The bytes for one key event. Empty when the key sends nothing (releases in the legacy
/// encoding, bare modifier keys).
pub fn encode_key(key: &KeyEvent, ctx: &KeyContext) -> Vec<u8> {
    if ctx.kitty_on() {
        if let Some(bytes) = kitty(key, ctx) {
            return bytes;
        }
    }
    if key.kind == KeyEventKind::Release {
        return Vec::new();
    }
    legacy(key, ctx)
}

/// A paste, wrapped in bracketed-paste markers when the child asked for them. Without them,
/// newlines become carriage returns like typed Enter. A stray end marker inside the text is removed
/// so pasted text can't end the paste early.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let clean = text.replace("\x1b[201~", "");
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(clean.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// Focus reports, for children that asked for them (mode 1004).
pub fn encode_focus(gained: bool) -> &'static [u8] {
    if gained {
        b"\x1b[I"
    } else {
        b"\x1b[O"
    }
}

fn xterm_mod(m: KeyModifiers) -> u8 {
    1 + u8::from(m.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(m.contains(KeyModifiers::ALT))
        + 4 * u8::from(m.contains(KeyModifiers::CONTROL))
}

fn csi_letter(letter: char, m: KeyModifiers, ctx: &KeyContext) -> Vec<u8> {
    let p = xterm_mod(m);
    if p > 1 {
        format!("\x1b[1;{p}{letter}").into_bytes()
    } else if ctx.app_cursor && matches!(letter, 'A'..='D' | 'H' | 'F') {
        format!("\x1bO{letter}").into_bytes()
    } else {
        format!("\x1b[{letter}").into_bytes()
    }
}

fn csi_tilde(n: u8, m: KeyModifiers) -> Vec<u8> {
    let p = xterm_mod(m);
    if p > 1 {
        format!("\x1b[{n};{p}~").into_bytes()
    } else {
        format!("\x1b[{n}~").into_bytes()
    }
}

fn function_key(n: u8, m: KeyModifiers) -> Vec<u8> {
    let p = xterm_mod(m);
    match n {
        1..=4 => {
            let letter = ['P', 'Q', 'R', 'S'][usize::from(n) - 1];
            if p > 1 {
                format!("\x1b[1;{p}{letter}").into_bytes()
            } else {
                format!("\x1bO{letter}").into_bytes()
            }
        }
        5..=20 => {
            let code = [
                15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34,
            ][usize::from(n) - 5];
            csi_tilde(code, m)
        }
        _ => Vec::new(),
    }
}

fn control_byte(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

fn legacy(key: &KeyEvent, ctx: &KeyContext) -> Vec<u8> {
    let m = key.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let alt_prefix = |mut bytes: Vec<u8>| {
        if alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    };
    match key.code {
        KeyCode::Char(c) => {
            let bytes = match (ctrl, control_byte(c)) {
                (true, Some(b)) => vec![b],
                _ => c.to_string().into_bytes(),
            };
            alt_prefix(bytes)
        }
        // Enter is a plain carriage return whatever else is held: terminals without the kitty
        // protocol (and Windows consoles) can't tell Shift-Enter from Enter, so the pane doesn't
        // pretend to either. Ctrl-Enter is the line feed it has always been.
        KeyCode::Enter => alt_prefix(vec![if ctrl { b'\n' } else { b'\r' }]),
        KeyCode::Tab if m.contains(KeyModifiers::SHIFT) => alt_prefix(b"\x1b[Z".to_vec()),
        KeyCode::Tab => alt_prefix(vec![b'\t']),
        KeyCode::BackTab => alt_prefix(b"\x1b[Z".to_vec()),
        KeyCode::Backspace => alt_prefix(vec![if ctrl { 0x08 } else { 0x7f }]),
        KeyCode::Esc => alt_prefix(vec![0x1b]),
        KeyCode::Up => csi_letter('A', m, ctx),
        KeyCode::Down => csi_letter('B', m, ctx),
        KeyCode::Right => csi_letter('C', m, ctx),
        KeyCode::Left => csi_letter('D', m, ctx),
        KeyCode::Home => csi_letter('H', m, ctx),
        KeyCode::End => csi_letter('F', m, ctx),
        KeyCode::Insert => csi_tilde(2, m),
        KeyCode::Delete => csi_tilde(3, m),
        KeyCode::PageUp => csi_tilde(5, m),
        KeyCode::PageDown => csi_tilde(6, m),
        KeyCode::F(n) => function_key(n, m),
        KeyCode::Null => vec![0],
        _ => Vec::new(),
    }
}

fn kitty_mod(m: KeyModifiers) -> u8 {
    1 + u8::from(m.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(m.contains(KeyModifiers::ALT))
        + 4 * u8::from(m.contains(KeyModifiers::CONTROL))
        + 8 * u8::from(m.contains(KeyModifiers::SUPER))
        + 16 * u8::from(m.contains(KeyModifiers::HYPER))
        + 32 * u8::from(m.contains(KeyModifiers::META))
}

/// `;mods[:event]`, or nothing when both are the defaults.
fn kitty_params(m: u8, event: u8, with_events: bool) -> String {
    let event = if with_events && event != 1 {
        format!(":{event}")
    } else {
        String::new()
    };
    if m == 1 && event.is_empty() {
        String::new()
    } else {
        format!(";{m}{event}")
    }
}

fn kitty(key: &KeyEvent, ctx: &KeyContext) -> Option<Vec<u8>> {
    let flags = ctx.kitty;
    let with_events = flags.has(KittyFlags::EVENT_TYPES);
    let all_keys = flags.has(KittyFlags::ALL_KEYS);
    let event = match key.kind {
        KeyEventKind::Press => 1,
        KeyEventKind::Repeat => 2,
        KeyEventKind::Release => 3,
    };
    let m = kitty_mod(key.modifiers);
    let params = kitty_params(m, event, with_events);
    let released = event == 3;

    let functional = |code: &str, tail: char| -> Option<Vec<u8>> {
        if released && !with_events {
            return Some(Vec::new());
        }
        let body = match (code, params.is_empty()) {
            ("1", true) => String::new(),
            (code, _) => format!("{code}{params}"),
        };
        Some(format!("\x1b[{body}{tail}").into_bytes())
    };

    let csi_u = |code: u32, text: Option<char>| -> Vec<u8> {
        let mut out = format!("\x1b[{code}");
        let has_text = text.is_some();
        if !params.is_empty() {
            out.push_str(&params);
        } else if has_text {
            out.push_str(";1");
        }
        if let Some(t) = text {
            out.push_str(&format!(";{}", t as u32));
        }
        out.push('u');
        out.into_bytes()
    };

    match key.code {
        KeyCode::Up => functional("1", 'A'),
        KeyCode::Down => functional("1", 'B'),
        KeyCode::Right => functional("1", 'C'),
        KeyCode::Left => functional("1", 'D'),
        KeyCode::Home => functional("1", 'H'),
        KeyCode::End => functional("1", 'F'),
        KeyCode::Insert => functional("2", '~'),
        KeyCode::Delete => functional("3", '~'),
        KeyCode::PageUp => functional("5", '~'),
        KeyCode::PageDown => functional("6", '~'),
        KeyCode::F(n @ 1..=4) => functional("1", ['P', 'Q', 'R', 'S'][usize::from(n) - 1]),
        KeyCode::F(n @ 5..=20) => {
            let code = [
                15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34,
            ][usize::from(n) - 5];
            functional(&code.to_string(), '~')
        }
        KeyCode::Esc => Some(csi_u(27, None)),
        KeyCode::Enter | KeyCode::Tab | KeyCode::Backspace | KeyCode::BackTab => {
            let (code, plain): (u32, u8) = match key.code {
                KeyCode::Enter => (13, b'\r'),
                KeyCode::Backspace => (127, 0x7f),
                _ => (9, b'\t'),
            };
            let (code, m) = if key.code == KeyCode::BackTab {
                (
                    9,
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        m
                    } else {
                        m + 1
                    },
                )
            } else {
                (code, m)
            };
            if released && !(with_events && all_keys) {
                return Some(Vec::new());
            }
            if m == 1 && !all_keys && key.code != KeyCode::BackTab {
                return Some(vec![plain]);
            }
            let params = kitty_params(m, event, with_events);
            Some(format!("\x1b[{code}{params}u").into_bytes())
        }
        KeyCode::Char(c) => {
            let beyond_shift = key.modifiers.intersects(
                KeyModifiers::ALT
                    | KeyModifiers::CONTROL
                    | KeyModifiers::SUPER
                    | KeyModifiers::HYPER
                    | KeyModifiers::META,
            );
            if !beyond_shift && !all_keys {
                return None;
            }
            if released && !(with_events && (all_keys || beyond_shift)) {
                return Some(Vec::new());
            }
            let base = c.to_lowercase().next().unwrap_or(c);
            let text =
                (flags.has(KittyFlags::ASSOCIATED_TEXT) && !beyond_shift && !released).then_some(c);
            Some(csi_u(base as u32, text))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use KeyCode::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn legacy_bytes(code: KeyCode, mods: KeyModifiers) -> Vec<u8> {
        encode_key(&key(code, mods), &KeyContext::default())
    }

    const NONE: KeyModifiers = KeyModifiers::NONE;
    const CTRL: KeyModifiers = KeyModifiers::CONTROL;
    const ALT: KeyModifiers = KeyModifiers::ALT;
    const SHIFT: KeyModifiers = KeyModifiers::SHIFT;

    #[test]
    fn legacy_table() {
        let cases: &[(KeyCode, KeyModifiers, &[u8])] = &[
            (Char('a'), NONE, b"a"),
            (Char('A'), SHIFT, b"A"),
            (Char('é'), NONE, "é".as_bytes()),
            (Char('c'), CTRL, &[3]),
            (Char('C'), CTRL | SHIFT, &[3]),
            (Char('z'), CTRL, &[26]),
            (Char('\\'), CTRL, &[0x1c]),
            (Char(' '), CTRL, &[0]),
            (Char('['), CTRL, &[0x1b]),
            (Char('x'), ALT, b"\x1bx"),
            (Char('c'), CTRL | ALT, b"\x1b\x03"),
            (Enter, NONE, b"\r"),
            (Enter, SHIFT, b"\r"),
            (Enter, ALT, b"\x1b\r"),
            (Enter, CTRL, b"\n"),
            (Tab, NONE, b"\t"),
            (Tab, SHIFT, b"\x1b[Z"),
            (BackTab, SHIFT, b"\x1b[Z"),
            (Backspace, NONE, &[0x7f]),
            (Backspace, CTRL, &[0x08]),
            (Backspace, ALT, b"\x1b\x7f"),
            (Esc, NONE, &[0x1b]),
            (Esc, ALT, b"\x1b\x1b"),
            (Up, NONE, b"\x1b[A"),
            (Down, NONE, b"\x1b[B"),
            (Right, NONE, b"\x1b[C"),
            (Left, NONE, b"\x1b[D"),
            (Up, CTRL, b"\x1b[1;5A"),
            (Left, ALT, b"\x1b[1;3D"),
            (Right, SHIFT | CTRL, b"\x1b[1;6C"),
            (Home, NONE, b"\x1b[H"),
            (End, NONE, b"\x1b[F"),
            (Insert, NONE, b"\x1b[2~"),
            (Delete, NONE, b"\x1b[3~"),
            (Delete, SHIFT, b"\x1b[3;2~"),
            (PageUp, NONE, b"\x1b[5~"),
            (PageDown, CTRL, b"\x1b[6;5~"),
            (F(1), NONE, b"\x1bOP"),
            (F(4), NONE, b"\x1bOS"),
            (F(1), SHIFT, b"\x1b[1;2P"),
            (F(5), NONE, b"\x1b[15~"),
            (F(12), NONE, b"\x1b[24~"),
            (F(5), CTRL, b"\x1b[15;5~"),
            (F(21), NONE, b""),
            (Null, NONE, &[0]),
            (CapsLock, NONE, b""),
        ];
        for (code, mods, want) in cases {
            assert_eq!(legacy_bytes(*code, *mods), *want, "{code:?} with {mods:?}");
        }
    }

    #[test]
    fn application_cursor_mode_uses_ss3() {
        let ctx = KeyContext {
            app_cursor: true,
            ..KeyContext::default()
        };
        let enc = |c, m| encode_key(&key(c, m), &ctx);
        assert_eq!(enc(Up, NONE), b"\x1bOA");
        assert_eq!(enc(End, NONE), b"\x1bOF");
        assert_eq!(
            enc(Up, CTRL),
            b"\x1b[1;5A",
            "modified keys keep the CSI form"
        );
        assert_eq!(enc(PageUp, NONE), b"\x1b[5~");
    }

    #[test]
    fn releases_send_nothing_in_the_legacy_encoding() {
        let mut k = key(Char('a'), NONE);
        k.kind = KeyEventKind::Release;
        assert!(encode_key(&k, &KeyContext::default()).is_empty());
        k.kind = KeyEventKind::Repeat;
        assert_eq!(encode_key(&k, &KeyContext::default()), b"a");
    }

    fn kctx(flags: u8) -> KeyContext {
        KeyContext {
            kitty: KittyFlags(flags),
            host_kitty: true,
            ..KeyContext::default()
        }
    }

    fn kitty_bytes(flags: u8, code: KeyCode, mods: KeyModifiers) -> String {
        String::from_utf8(encode_key(&key(code, mods), &kctx(flags))).unwrap()
    }

    #[test]
    fn kitty_disambiguate_table() {
        let d = KittyFlags::DISAMBIGUATE;
        let cases: &[(KeyCode, KeyModifiers, &str)] = &[
            (Char('a'), NONE, "a"),
            (Char('A'), SHIFT, "A"),
            (Char('c'), CTRL, "\x1b[99;5u"),
            (Char('C'), CTRL | SHIFT, "\x1b[99;6u"),
            (Char('x'), ALT, "\x1b[120;3u"),
            (Char('i'), CTRL, "\x1b[105;5u"),
            (Esc, NONE, "\x1b[27u"),
            (Enter, NONE, "\r"),
            (Enter, SHIFT, "\x1b[13;2u"),
            (Enter, CTRL, "\x1b[13;5u"),
            (Tab, NONE, "\t"),
            (Tab, CTRL, "\x1b[9;5u"),
            (BackTab, SHIFT, "\x1b[9;2u"),
            (Backspace, NONE, "\x7f"),
            (Backspace, ALT, "\x1b[127;3u"),
            (Up, NONE, "\x1b[A"),
            (Up, SHIFT, "\x1b[1;2A"),
            (Home, CTRL, "\x1b[1;5H"),
            (Delete, NONE, "\x1b[3~"),
            (PageDown, ALT, "\x1b[6;3~"),
            (F(1), NONE, "\x1b[P"),
            (F(3), CTRL, "\x1b[1;5R"),
            (F(5), NONE, "\x1b[15~"),
            (Char('a'), KeyModifiers::SUPER, "\x1b[97;9u"),
        ];
        for (code, mods, want) in cases {
            assert_eq!(kitty_bytes(d, *code, *mods), *want, "{code:?} {mods:?}");
        }
    }

    #[test]
    fn kitty_all_keys_sends_everything_as_escape_codes() {
        let f = KittyFlags::DISAMBIGUATE | KittyFlags::ALL_KEYS;
        assert_eq!(kitty_bytes(f, Char('a'), NONE), "\x1b[97u");
        assert_eq!(kitty_bytes(f, Char('A'), SHIFT), "\x1b[97;2u");
        assert_eq!(kitty_bytes(f, Enter, NONE), "\x1b[13u");
        assert_eq!(kitty_bytes(f, Tab, NONE), "\x1b[9u");
        assert_eq!(kitty_bytes(f, Backspace, NONE), "\x1b[127u");
    }

    #[test]
    fn kitty_associated_text_follows_the_key() {
        let f = KittyFlags::DISAMBIGUATE | KittyFlags::ALL_KEYS | KittyFlags::ASSOCIATED_TEXT;
        assert_eq!(kitty_bytes(f, Char('a'), NONE), "\x1b[97;1;97u");
        assert_eq!(kitty_bytes(f, Char('A'), SHIFT), "\x1b[97;2;65u");
    }

    #[test]
    fn kitty_event_types_mark_repeats_and_releases() {
        let f = KittyFlags::DISAMBIGUATE | KittyFlags::EVENT_TYPES | KittyFlags::ALL_KEYS;
        let mut k = key(Char('a'), NONE);
        k.kind = KeyEventKind::Release;
        assert_eq!(
            String::from_utf8(encode_key(&k, &kctx(f))).unwrap(),
            "\x1b[97;1:3u"
        );
        k.kind = KeyEventKind::Repeat;
        assert_eq!(
            String::from_utf8(encode_key(&k, &kctx(f))).unwrap(),
            "\x1b[97;1:2u"
        );
        let mut up = key(Up, CTRL);
        up.kind = KeyEventKind::Release;
        assert_eq!(
            String::from_utf8(encode_key(&up, &kctx(f))).unwrap(),
            "\x1b[1;5:3A"
        );
    }

    #[test]
    fn kitty_releases_are_dropped_unless_asked_for() {
        let mut k = key(Char('c'), CTRL);
        k.kind = KeyEventKind::Release;
        assert!(encode_key(&k, &kctx(KittyFlags::DISAMBIGUATE)).is_empty());
    }

    #[test]
    fn a_host_without_kitty_keys_gets_the_legacy_encoding() {
        let ctx = KeyContext {
            kitty: KittyFlags(KittyFlags::DISAMBIGUATE),
            host_kitty: false,
            ..KeyContext::default()
        };
        assert_eq!(encode_key(&key(Char('c'), CTRL), &ctx), vec![3]);
        assert_eq!(encode_key(&key(Enter, SHIFT), &ctx), b"\r");
    }

    #[test]
    fn a_child_that_did_not_ask_gets_legacy_even_on_a_kitty_host() {
        let ctx = KeyContext {
            host_kitty: true,
            ..KeyContext::default()
        };
        assert_eq!(encode_key(&key(Char('c'), CTRL), &ctx), vec![3]);
    }

    #[test]
    fn paste_is_bracketed_only_when_asked() {
        assert_eq!(encode_paste("a\nb", false), b"a\rb");
        assert_eq!(encode_paste("a\r\nb", false), b"a\rb");
        assert_eq!(encode_paste("hi", true), b"\x1b[200~hi\x1b[201~");
        assert_eq!(
            encode_paste("x\x1b[201~y", true),
            b"\x1b[200~xy\x1b[201~",
            "an embedded end marker can't end the paste early"
        );
    }

    #[test]
    fn focus_reports() {
        assert_eq!(encode_focus(true), b"\x1b[I");
        assert_eq!(encode_focus(false), b"\x1b[O");
    }
}
