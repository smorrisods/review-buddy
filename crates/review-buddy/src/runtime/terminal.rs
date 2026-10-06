//! Terminal setup and panic-safe restore.

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Owns the terminal modes. Dropping it restores the terminal.
pub struct TerminalGuard {
    pub terminal: Tui,
}

impl TerminalGuard {
    pub fn enter(mouse: bool) -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        ACTIVE.store(true, Ordering::SeqCst);
        let mut stdout = io::stdout();
        let setup = execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableFocusChange,
            Hide
        );
        let setup = setup.and_then(|()| {
            if mouse {
                execute!(stdout, EnableMouseCapture)
            } else {
                Ok(())
            }
        });
        if let Err(err) = setup {
            restore();
            return Err(err);
        }
        enhance_keys(&mut stdout);
        let terminal = Terminal::new(CrosstermBackend::new(stdout)).inspect_err(|_| restore())?;
        Ok(Self { terminal })
    }
}

/// Asks terminals that speak the kitty keyboard protocol to report `⌃⏎` and `⇧⏎` as such.
/// Others ignore the request. It isn't available through the Windows console API, which
/// already reports those modifiers.
#[cfg(unix)]
fn enhance_keys(out: &mut Stdout) {
    use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
    let _ = execute!(
        out,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
}

#[cfg(not(unix))]
fn enhance_keys(_out: &mut Stdout) {}

#[cfg(unix)]
fn release_keys(out: &mut Stdout) {
    let _ = execute!(out, crossterm::event::PopKeyboardEnhancementFlags);
}

#[cfg(not(unix))]
fn release_keys(_out: &mut Stdout) {}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

/// Undoes everything `enter` did. Safe to call more than once.
pub fn restore() {
    if !ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let mut stdout = io::stdout();
    release_keys(&mut stdout);
    let _ = execute!(
        stdout,
        Show,
        DisableFocusChange,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}
