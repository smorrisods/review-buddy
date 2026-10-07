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
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Owns the terminal modes. Dropping it restores the terminal.
pub struct TerminalGuard {
    pub terminal: Tui,
    mouse: bool,
}

impl TerminalGuard {
    pub fn enter(mouse: bool) -> io::Result<Self> {
        install_panic_hook();
        let stdout = setup(mouse)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout)).inspect_err(|_| restore())?;
        Ok(Self { terminal, mouse })
    }

    /// Hands the terminal back to the shell, as before a stop. Drop is still safe afterwards.
    pub fn suspend(&mut self) {
        restore();
    }

    /// Takes the terminal again after a stop and forces a full repaint.
    pub fn resume(&mut self) -> io::Result<()> {
        let mut stdout = setup(self.mouse)?;
        execute!(stdout, Clear(ClearType::All))?;
        self.terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(())
    }
}

fn setup(mouse: bool) -> io::Result<Stdout> {
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
    Ok(stdout)
}

/// Stops the process the way a shell expects (`SIGTSTP`'s default action) and returns once it
/// is continued. The terminal must already be restored.
#[cfg(unix)]
pub fn stop_process() {
    let _ = signal_hook::low_level::emulate_default_handler(signal_hook::consts::SIGTSTP);
}

#[cfg(not(unix))]
pub fn stop_process() {}

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

/// Whether the terminal answered the kitty keyboard protocol query, so `⌃⏎` arrives as itself.
/// Windows reports modifiers on its own. Anything that doesn't answer is treated as legacy.
/// `REVIEW_BUDDY_KITTY_KEYS=1` or `0` skips the query, which a terminal that never answers
/// would otherwise make wait.
#[cfg(unix)]
pub fn keys_enhanced() -> bool {
    match std::env::var("REVIEW_BUDDY_KITTY_KEYS").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
    }
}

#[cfg(not(unix))]
pub fn keys_enhanced() -> bool {
    true
}

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
    // ConPTY can leave win32-input-mode on after a pane child dies badly; this is empty elsewhere.
    let _ = io::Write::write_all(&mut stdout, rb_term::host_cleanup());
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
