//! Mouse reports for children that turned mouse reporting on.
//!
//! The host decides whether an event goes to the child at all (it doesn't while Shift is held,
//! so Shift always reaches the host terminal's own selection). This module only encodes.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::emulator::{Modes, MouseMode};

/// The direction of a wheel turn that isn't being reported to the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    Up,
    Down,
}

/// The bytes for `event` at cell (`col`, `row`) of the pane (zero-based), or `None` when the
/// child's mouse mode doesn't include this kind of event or the position can't be encoded.
pub fn encode_mouse(event: &MouseEvent, col: u16, row: u16, modes: &Modes) -> Option<Vec<u8>> {
    let (button, motion, release) = match event.kind {
        MouseEventKind::Down(b) => (button_code(b), false, false),
        MouseEventKind::Up(b) => (button_code(b), false, true),
        MouseEventKind::Drag(b) => {
            if !matches!(modes.mouse, MouseMode::Drag | MouseMode::Motion) {
                return None;
            }
            (button_code(b), true, false)
        }
        MouseEventKind::Moved => {
            if modes.mouse != MouseMode::Motion {
                return None;
            }
            (3, true, false)
        }
        MouseEventKind::ScrollUp => (64, false, false),
        MouseEventKind::ScrollDown => (65, false, false),
        MouseEventKind::ScrollLeft => (66, false, false),
        MouseEventKind::ScrollRight => (67, false, false),
    };
    if modes.mouse == MouseMode::Off {
        return None;
    }
    let mut code = button;
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        code += 4;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        code += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        code += 16;
    }
    if motion {
        code += 32;
    }
    let (x, y) = (u32::from(col) + 1, u32::from(row) + 1);
    if modes.sgr_mouse {
        let end = if release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    let code = if release { (code & !3) | 3 } else { code };
    let mut out = b"\x1b[M".to_vec();
    let mut push = |value: u32| -> Option<()> {
        let value = value + 32;
        if modes.utf8_mouse {
            let ch = char::from_u32(value)?;
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        } else {
            out.push(u8::try_from(value).ok().filter(|v| *v < 255)?);
        }
        Some(())
    };
    push(code)?;
    push(x)?;
    push(y)?;
    Some(out)
}

fn button_code(b: MouseButton) -> u32 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// What a wheel turn sends to an alternate-screen child that isn't reporting the mouse: arrow
/// keys, three per notch, the way terminals do for `less` and friends.
pub fn wheel_as_arrows(wheel: Wheel, app_cursor: bool) -> Vec<u8> {
    let key: &[u8] = match (wheel, app_cursor) {
        (Wheel::Up, false) => b"\x1b[A",
        (Wheel::Down, false) => b"\x1b[B",
        (Wheel::Up, true) => b"\x1bOA",
        (Wheel::Down, true) => b"\x1bOB",
    };
    key.repeat(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: MouseEventKind, mods: KeyModifiers) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: mods,
        }
    }

    fn modes(mouse: MouseMode, sgr: bool) -> Modes {
        Modes {
            mouse,
            sgr_mouse: sgr,
            ..Modes::default()
        }
    }

    const NONE: KeyModifiers = KeyModifiers::NONE;

    #[test]
    fn nothing_is_reported_when_the_child_did_not_ask() {
        let e = ev(MouseEventKind::Down(MouseButton::Left), NONE);
        assert_eq!(encode_mouse(&e, 0, 0, &Modes::default()), None);
    }

    #[test]
    fn sgr_press_release_and_wheel() {
        let m = modes(MouseMode::Click, true);
        let down = ev(MouseEventKind::Down(MouseButton::Left), NONE);
        assert_eq!(encode_mouse(&down, 4, 9, &m).unwrap(), b"\x1b[<0;5;10M");
        let up = ev(MouseEventKind::Up(MouseButton::Left), NONE);
        assert_eq!(encode_mouse(&up, 4, 9, &m).unwrap(), b"\x1b[<0;5;10m");
        let right = ev(
            MouseEventKind::Down(MouseButton::Right),
            KeyModifiers::CONTROL,
        );
        assert_eq!(encode_mouse(&right, 0, 0, &m).unwrap(), b"\x1b[<18;1;1M");
        let wheel = ev(MouseEventKind::ScrollDown, NONE);
        assert_eq!(encode_mouse(&wheel, 1, 1, &m).unwrap(), b"\x1b[<65;2;2M");
    }

    #[test]
    fn drag_and_motion_need_their_modes() {
        let drag = ev(MouseEventKind::Drag(MouseButton::Left), NONE);
        let moved = ev(MouseEventKind::Moved, NONE);
        let click = modes(MouseMode::Click, true);
        assert_eq!(encode_mouse(&drag, 0, 0, &click), None);
        let d = modes(MouseMode::Drag, true);
        assert_eq!(encode_mouse(&drag, 2, 3, &d).unwrap(), b"\x1b[<32;3;4M");
        assert_eq!(encode_mouse(&moved, 0, 0, &d), None);
        let all = modes(MouseMode::Motion, true);
        assert_eq!(encode_mouse(&moved, 0, 0, &all).unwrap(), b"\x1b[<35;1;1M");
    }

    #[test]
    fn legacy_reports_use_offset_bytes_and_a_release_button() {
        let m = modes(MouseMode::Click, false);
        let down = ev(MouseEventKind::Down(MouseButton::Left), NONE);
        assert_eq!(encode_mouse(&down, 0, 0, &m).unwrap(), b"\x1b[M !!");
        let up = ev(MouseEventKind::Up(MouseButton::Left), NONE);
        assert_eq!(encode_mouse(&up, 0, 0, &m).unwrap(), b"\x1b[M#!!");
    }

    #[test]
    fn legacy_reports_far_right_are_dropped_unless_utf8() {
        let m = modes(MouseMode::Click, false);
        let down = ev(MouseEventKind::Down(MouseButton::Left), NONE);
        assert_eq!(encode_mouse(&down, 300, 0, &m), None);
        let utf8 = Modes {
            utf8_mouse: true,
            ..m
        };
        let bytes = encode_mouse(&down, 300, 0, &utf8).unwrap();
        assert!(String::from_utf8(bytes).unwrap().contains('\u{14d}'));
    }

    #[test]
    fn wheel_as_arrows_sends_three() {
        assert_eq!(wheel_as_arrows(Wheel::Up, false), b"\x1b[A\x1b[A\x1b[A");
        assert_eq!(wheel_as_arrows(Wheel::Down, true), b"\x1bOB\x1bOB\x1bOB");
    }
}
