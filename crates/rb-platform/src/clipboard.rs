use std::io::Write;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::{CommandRunner, Os, PlatformError};

/// Largest base64 payload sent in one OSC 52 sequence; many terminals drop more.
pub const OSC52_MAX_ENCODED: usize = 100_000;

/// What the clipboard code needs to know about the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardContext {
    pub os: Os,
    pub in_tmux: bool,
    pub wayland: bool,
}

impl ClipboardContext {
    pub fn from_env() -> Self {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        Self {
            os: Os::current(),
            in_tmux: set("TMUX"),
            wayland: set("WAYLAND_DISPLAY"),
        }
    }
}

/// How a copy was performed. OSC 52 gives no confirmation, so `Osc52` means "sent".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyMethod {
    Osc52,
    Tool(&'static str),
}

/// Builds an OSC 52 "set clipboard" sequence, wrapped for tmux passthrough if needed.
pub fn osc52_sequence(text: &str, tmux: bool) -> Result<String, PlatformError> {
    let encoded = STANDARD.encode(text.as_bytes());
    if encoded.len() > OSC52_MAX_ENCODED {
        return Err(PlatformError::ClipboardTooLarge {
            size: encoded.len(),
            limit: OSC52_MAX_ENCODED,
        });
    }
    let seq = format!("\x1b]52;c;{encoded}\x07");
    if tmux {
        // tmux needs the inner ESC doubled inside a DCS passthrough.
        Ok(format!(
            "\x1bPtmux;{}\x1b\\",
            seq.replace('\x1b', "\x1b\x1b")
        ))
    } else {
        Ok(seq)
    }
}

/// OS clipboard tools to try, in order.
pub fn tool_candidates(ctx: &ClipboardContext) -> Vec<(&'static str, &'static [&'static str])> {
    match ctx.os {
        Os::MacOs => vec![("pbcopy", &[])],
        Os::Windows => vec![("clip", &[])],
        Os::Linux => {
            let wl = ("wl-copy", &[][..]);
            let xclip = ("xclip", &["-selection", "clipboard"][..]);
            let xsel = ("xsel", &["--clipboard", "--input"][..]);
            if ctx.wayland {
                vec![wl, xclip, xsel]
            } else {
                vec![xclip, xsel, wl]
            }
        }
    }
}

/// Copies `text`: OSC 52 to `tty` first, then an OS tool if that can't be used.
pub fn copy(
    text: &str,
    ctx: &ClipboardContext,
    tty: &mut dyn Write,
    runner: &dyn CommandRunner,
) -> Result<CopyMethod, PlatformError> {
    if let Ok(seq) = osc52_sequence(text, ctx.in_tmux) {
        if tty
            .write_all(seq.as_bytes())
            .and_then(|()| tty.flush())
            .is_ok()
        {
            return Ok(CopyMethod::Osc52);
        }
    }
    copy_with_tool(text, ctx, runner)
}

pub fn copy_with_tool(
    text: &str,
    ctx: &ClipboardContext,
    runner: &dyn CommandRunner,
) -> Result<CopyMethod, PlatformError> {
    for (program, args) in tool_candidates(ctx) {
        if let Ok(out) = runner.run(program, args, Some(text.as_bytes())) {
            if out.success {
                return Ok(CopyMethod::Tool(program));
            }
        }
    }
    Err(PlatformError::ClipboardUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::fake::FakeRunner;

    fn ctx(os: Os) -> ClipboardContext {
        ClipboardContext {
            os,
            in_tmux: false,
            wayland: false,
        }
    }

    #[test]
    fn osc52_encodes_base64() {
        assert_eq!(
            osc52_sequence("hello", false).unwrap(),
            "\x1b]52;c;aGVsbG8=\x07"
        );
    }

    #[test]
    fn osc52_tmux_passthrough_doubles_escape() {
        assert_eq!(
            osc52_sequence("hi", true).unwrap(),
            "\x1bPtmux;\x1b\x1b]52;c;aGk=\x07\x1b\\"
        );
    }

    #[test]
    fn osc52_enforces_limit() {
        let ok = "a".repeat(OSC52_MAX_ENCODED / 4 * 3);
        assert!(osc52_sequence(&ok, false).is_ok());
        let big = "a".repeat(OSC52_MAX_ENCODED);
        assert!(matches!(
            osc52_sequence(&big, false),
            Err(PlatformError::ClipboardTooLarge { .. })
        ));
    }

    #[test]
    fn copy_prefers_osc52() {
        let r = FakeRunner::default();
        let mut tty = Vec::new();
        let m = copy("x", &ctx(Os::Linux), &mut tty, &r).unwrap();
        assert_eq!(m, CopyMethod::Osc52);
        assert!(!tty.is_empty());
        assert!(r.calls.borrow().is_empty());
    }

    #[test]
    fn oversize_falls_back_to_tool_with_stdin() {
        let r = FakeRunner::default().with("pbcopy", true, "", "");
        let mut tty = Vec::new();
        let big = "a".repeat(OSC52_MAX_ENCODED);
        let m = copy(&big, &ctx(Os::MacOs), &mut tty, &r).unwrap();
        assert_eq!(m, CopyMethod::Tool("pbcopy"));
        assert!(tty.is_empty());
        assert_eq!(r.calls.borrow()[0].2.as_deref(), Some(big.as_bytes()));
    }

    #[test]
    fn linux_tool_order_and_failure() {
        let mut c = ctx(Os::Linux);
        assert_eq!(tool_candidates(&c)[0].0, "xclip");
        c.wayland = true;
        assert_eq!(tool_candidates(&c)[0].0, "wl-copy");
        let r = FakeRunner::default()
            .with("wl-copy", false, "", "")
            .with("xclip", true, "", "");
        assert_eq!(
            copy_with_tool("x", &c, &r).unwrap(),
            CopyMethod::Tool("xclip")
        );
        assert!(matches!(
            copy_with_tool("x", &c, &FakeRunner::default()),
            Err(PlatformError::ClipboardUnavailable)
        ));
    }

    #[test]
    fn broken_tty_falls_back() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let r = FakeRunner::default().with("clip", true, "", "");
        let m = copy("x", &ctx(Os::Windows), &mut Broken, &r).unwrap();
        assert_eq!(m, CopyMethod::Tool("clip"));
    }
}
