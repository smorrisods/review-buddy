//! Choosing how images are drawn, once, at startup.

use std::time::Duration;

use ratatui_image::picker::{cap_parser::QueryStdioOptions, Picker, ProtocolType};
use rb_theme::ColourDepth;

use crate::config::Images;

/// How long the terminal gets to answer the capability query. A terminal that doesn't answer
/// costs this much at startup, once, and then gets the fallback.
pub const QUERY_TIMEOUT: Duration = Duration::from_millis(400);

/// Reads `REVIEW_BUDDY_IMAGES`; a value that isn't one of the four settings is ignored.
pub fn mode_from_env(value: Option<&str>) -> Option<Images> {
    let value = value?.trim().to_ascii_lowercase();
    [
        Images::Auto,
        Images::Off,
        Images::Halfblocks,
        Images::ForgeOnly,
    ]
    .into_iter()
    .find(|m| m.as_str() == value)
}

/// The renderer for this terminal, or `None` when images should be notes only.
///
/// Call after entering the alternate screen and before reading terminal events: the query
/// writes to the terminal and reads its answer from stdin.
///
/// Halfblocks paint with colour, so without truecolour (or with `NO_COLOR`) they are skipped.
/// Real graphics protocols found by the query are used wherever they are found, except under
/// `NO_COLOR`, which turns every picture off.
pub fn detect(mode: Images, depth: ColourDepth, no_color: bool) -> Option<Picker> {
    if no_color {
        return None;
    }
    let truecolour = depth == ColourDepth::TrueColour;
    match mode {
        Images::Off => None,
        Images::Halfblocks => Some(Picker::halfblocks()),
        Images::Auto | Images::ForgeOnly => {
            if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
                return None;
            }
            let options = QueryStdioOptions {
                timeout: QUERY_TIMEOUT,
                ..QueryStdioOptions::default()
            };
            match query(options) {
                Some(picker) if picker.protocol_type() != ProtocolType::Halfblocks => Some(picker),
                Some(picker) => truecolour.then_some(picker),
                None => truecolour.then(Picker::halfblocks),
            }
        }
    }
}

#[cfg(unix)]
use ratatui_image::{
    picker::cap_parser::{Parser, Response},
    FontSize,
};

/// What the terminal said about graphics, as far as it said anything.
#[cfg(unix)]
#[derive(Debug, Default, PartialEq, Eq)]
struct Answer {
    protocol: Option<ProtocolType>,
    cell: Option<(u16, u16)>,
}

/// Reads a terminal's replies a few bytes at a time and stops at the final status report, which
/// every terminal sends.
#[cfg(unix)]
#[derive(Default)]
struct Collector {
    parser: Option<Parser>,
    answer: Answer,
}

#[cfg(unix)]
impl Collector {
    /// Takes bytes from the terminal; returns whether the reply is complete.
    fn push(&mut self, bytes: &[u8]) -> bool {
        let parser = self.parser.get_or_insert_with(Parser::new);
        for byte in bytes {
            for response in parser.push(char::from(*byte)) {
                match response {
                    Response::Kitty => self.answer.protocol = Some(ProtocolType::Kitty),
                    Response::Sixel if self.answer.protocol.is_none() => {
                        self.answer.protocol = Some(ProtocolType::Sixel);
                    }
                    Response::CellSize(Some(cell)) => self.answer.cell = Some(cell),
                    Response::Status => return true,
                    _ => {}
                }
            }
        }
        false
    }
}

/// Terminals that answer the graphics queries wrongly, so those protocols are not asked about.
#[cfg(unix)]
fn blacklist() -> Vec<ProtocolType> {
    let set = |name: &str| std::env::var(name).is_ok_and(|v| !v.is_empty());
    if set("WEZTERM_EXECUTABLE") || set("KONSOLE_VERSION") {
        vec![ProtocolType::Kitty, ProtocolType::Sixel]
    } else {
        Vec::new()
    }
}

#[cfg(unix)]
fn picker_from(answer: &Answer, fallback_cell: Option<(u16, u16)>) -> Picker {
    let (w, h) = answer
        .cell
        .or(fallback_cell)
        .filter(|(w, h)| *w > 0 && *h > 0)
        .unwrap_or((10, 20));
    #[allow(deprecated)]
    let mut picker = Picker::from_fontsize(FontSize::new(w, h));
    if let Some(protocol) = answer.protocol {
        picker.set_protocol_type(protocol);
    }
    picker
}

/// Asks the terminal what it can draw. On Unix this waits on stdin with a deadline, so a terminal
/// that never answers costs `options.timeout` and leaves nothing behind reading the keyboard.
#[cfg(unix)]
fn query(options: QueryStdioOptions) -> Option<Picker> {
    use std::io::Write;
    use std::time::Instant;

    use rustix::event::{poll, PollFd, PollFlags, Timespec};

    let timeout = options.timeout;
    let is_tmux = Picker::halfblocks().tmux_detected();
    let options = QueryStdioOptions {
        blacklist_protocols: blacklist(),
        ..options
    };
    let mut out = std::io::stdout();
    out.write_all(Parser::query(is_tmux, options).as_bytes())
        .ok()?;
    out.flush().ok()?;

    let stdin = std::io::stdin();
    let deadline = Instant::now() + timeout;
    let mut collector = Collector::default();
    let mut buf = [0u8; 64];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        let wait = Timespec {
            tv_sec: i64::try_from(left.as_secs()).unwrap_or(0),
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [PollFd::new(&stdin, PollFlags::IN)];
        match poll(&mut fds, Some(&wait)) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => return None,
        }
        let n = rustix::io::read(&stdin, &mut buf[..])
            .ok()
            .filter(|n| *n > 0)?;
        if collector.push(&buf[..n]) {
            let size = rustix::termios::tcgetwinsize(&out).ok();
            let cell = size.and_then(|s| {
                (s.ws_col > 0 && s.ws_row > 0 && s.ws_xpixel > 0 && s.ws_ypixel > 0)
                    .then(|| (s.ws_xpixel / s.ws_col, s.ws_ypixel / s.ws_row))
            });
            return Some(picker_from(&collector.answer, cell));
        }
    }
}

/// Elsewhere the library's own query runs, which gives up after the timeout but may leave its
/// reader waiting for one more key.
#[cfg(not(unix))]
fn query(options: QueryStdioOptions) -> Option<Picker> {
    Picker::from_query_stdio_with_options(options).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_takes_the_four_settings_and_ignores_the_rest() {
        assert_eq!(mode_from_env(Some("off")), Some(Images::Off));
        assert_eq!(
            mode_from_env(Some(" Halfblocks ")),
            Some(Images::Halfblocks)
        );
        assert_eq!(mode_from_env(Some("forge-only")), Some(Images::ForgeOnly));
        assert_eq!(mode_from_env(Some("auto")), Some(Images::Auto));
        assert_eq!(mode_from_env(Some("sixel")), None);
        assert_eq!(mode_from_env(None), None);
    }

    #[test]
    fn off_and_no_color_never_draw() {
        assert!(detect(Images::Off, ColourDepth::TrueColour, false).is_none());
        assert!(detect(Images::Halfblocks, ColourDepth::TrueColour, true).is_none());
        assert!(detect(Images::Auto, ColourDepth::TrueColour, true).is_none());
    }

    #[cfg(unix)]
    fn collected(reply: &str) -> (bool, Answer) {
        let mut collector = Collector::default();
        let done = collector.push(reply.as_bytes());
        (done, collector.answer)
    }

    #[cfg(unix)]
    #[test]
    fn a_kitty_reply_with_a_cell_size_picks_kitty() {
        let (done, answer) = collected("\x1b_Gi=31;OK\x1b\\\x1b[?62;4c\x1b[6;20;10t\x1b[0n");
        assert!(done);
        assert_eq!(answer.protocol, Some(ProtocolType::Kitty));
        assert_eq!(answer.cell, Some((10, 20)));
    }

    #[cfg(unix)]
    #[test]
    fn a_sixel_reply_picks_sixel_and_a_plain_one_picks_nothing() {
        let (_, sixel) = collected("\x1b[?64;4;6c\x1b[0n");
        assert_eq!(sixel.protocol, Some(ProtocolType::Sixel));
        let (done, plain) = collected("\x1b[?64;6c\x1b[0n");
        assert!(done);
        assert_eq!(plain, Answer::default());
    }

    #[cfg(unix)]
    #[test]
    fn replies_that_arrive_in_pieces_are_assembled_and_garbage_is_skipped() {
        let mut collector = Collector::default();
        assert!(!collector.push(b"\x1b[?64;4"));
        assert!(!collector.push(b";6c\x1bgarbage"));
        assert!(collector.push(b"\x1b[0n"));
        assert_eq!(collector.answer.protocol, Some(ProtocolType::Sixel));
        assert!(!Collector::default().push(b"no answer yet"));
    }

    #[cfg(unix)]
    #[test]
    fn the_picker_uses_the_reported_cell_then_the_window_then_a_default() {
        let answer = Answer {
            protocol: Some(ProtocolType::Sixel),
            cell: Some((8, 16)),
        };
        let picker = picker_from(&answer, Some((9, 18)));
        assert_eq!(picker.protocol_type(), ProtocolType::Sixel);
        assert_eq!(
            (picker.font_size().width, picker.font_size().height),
            (8, 16)
        );
        let window = picker_from(&Answer::default(), Some((9, 18)));
        assert_eq!(window.font_size().width, 9);
        let default = picker_from(&Answer::default(), Some((0, 0)));
        assert_eq!(
            (default.font_size().width, default.font_size().height),
            (10, 20)
        );
    }

    #[test]
    fn halfblocks_can_be_asked_for_outright() {
        let picker = detect(Images::Halfblocks, ColourDepth::Ansi256, false).unwrap();
        assert_eq!(picker.protocol_type(), ProtocolType::Halfblocks);
    }
}
