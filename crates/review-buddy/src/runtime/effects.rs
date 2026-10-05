//! Effects that touch the desktop: the browser and the clipboard. Everything external goes
//! through an injected [`CommandRunner`] and terminal writer, so tests never spawn anything.

use std::io::Write;
use std::sync::{Arc, Mutex};

use rb_platform::{
    browser::open_url,
    clipboard::{copy, ClipboardContext},
    CommandRunner, PlatformError, SystemRunner,
};

use crate::app::{Notice, NoticeKind};

#[derive(Clone)]
pub struct Platform {
    runner: Arc<dyn CommandRunner + Send + Sync>,
    tty: Arc<Mutex<dyn Write + Send>>,
    ctx: ClipboardContext,
}

impl Platform {
    pub fn new(
        runner: Arc<dyn CommandRunner + Send + Sync>,
        tty: Arc<Mutex<dyn Write + Send>>,
        ctx: ClipboardContext,
    ) -> Self {
        Self { runner, tty, ctx }
    }

    /// The real runner, the controlling terminal's stdout, and the environment's clipboard setup.
    pub fn system() -> Self {
        Self::new(
            Arc::new(SystemRunner),
            Arc::new(Mutex::new(std::io::stdout())),
            ClipboardContext::from_env(),
        )
    }

    /// Opens `url` in the browser and says what happened.
    pub fn open(&self, url: &str) -> Notice {
        match open_url(url, self.runner.as_ref()) {
            Ok(()) => Notice::new(NoticeKind::Info, format!("Opened {url}")),
            Err(err) => Notice::new(NoticeKind::Warning, open_failure(&err)),
        }
    }

    /// Copies `text`, OSC 52 first and an OS tool second, and says what happened.
    pub fn copy(&self, text: &str) -> Notice {
        let Ok(mut tty) = self.tty.lock() else {
            return Notice::new(
                NoticeKind::Warning,
                copy_failure(&PlatformError::ClipboardUnavailable),
            );
        };
        match copy(text, &self.ctx, &mut *tty, self.runner.as_ref()) {
            Ok(_) => Notice::new(NoticeKind::Success, format!("Copied {text}")),
            Err(err) => Notice::new(NoticeKind::Warning, copy_failure(&err)),
        }
    }
}

/// The notice for a browser that wouldn't open: what went wrong, then what to do next.
pub fn open_failure(err: &PlatformError) -> String {
    match err {
        PlatformError::UnsupportedUrl => {
            "Only http and https links can be opened. Press y to copy it instead.".to_string()
        }
        _ => {
            "Couldn't open the browser. Press y to copy the link and paste it into one.".to_string()
        }
    }
}

pub fn copy_failure(_err: &PlatformError) -> String {
    "Couldn't copy to the clipboard. Install wl-copy, xclip or xsel, or use a terminal that supports OSC 52."
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use rb_platform::{CommandOutput, Os};

    use super::*;

    type Call = (String, Vec<String>, Option<Vec<u8>>);

    #[derive(Default)]
    struct Fake {
        works: Vec<&'static str>,
        calls: StdMutex<Vec<Call>>,
    }

    impl CommandRunner for Fake {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            self.calls.lock().unwrap().push((
                program.to_string(),
                args.iter().map(|a| a.to_string()).collect(),
                stdin.map(<[u8]>::to_vec),
            ));
            if self.works.contains(&program) {
                Ok(CommandOutput {
                    success: true,
                    ..CommandOutput::default()
                })
            } else {
                Err(PlatformError::Spawn {
                    program: program.to_string(),
                    reason: "not installed".to_string(),
                })
            }
        }
    }

    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn ctx() -> ClipboardContext {
        ClipboardContext {
            os: Os::Linux,
            in_tmux: false,
            wayland: false,
        }
    }

    fn platform(runner: &Arc<Fake>, tty: Arc<Mutex<dyn Write + Send>>) -> Platform {
        Platform::new(runner.clone(), tty, ctx())
    }

    #[test]
    fn open_runs_the_opener_and_reports_it() {
        let runner = Arc::new(Fake {
            works: vec!["xdg-open"],
            ..Fake::default()
        });
        let p = platform(&runner, Arc::new(Mutex::new(Vec::new())));
        let notice = p.open("https://github.com/a/b/pull/1");
        assert_eq!(notice.text, "Opened https://github.com/a/b/pull/1");
        assert_eq!(runner.calls.lock().unwrap()[0].0, "xdg-open");
    }

    #[test]
    fn open_failure_says_what_to_do_next() {
        let runner = Arc::new(Fake::default());
        let p = platform(&runner, Arc::new(Mutex::new(Vec::new())));
        let notice = p.open("https://x.test");
        assert_eq!(notice.kind, NoticeKind::Warning);
        assert!(notice.text.contains("Press y"));
        let notice = p.open("file:///etc/passwd");
        assert!(notice.text.starts_with("Only http and https"));
        assert_eq!(
            runner.calls.lock().unwrap().len(),
            1,
            "the bad link never ran"
        );
    }

    #[test]
    fn copy_writes_osc52_to_the_terminal_first() {
        let runner = Arc::new(Fake::default());
        let tty = Arc::new(Mutex::new(Vec::<u8>::new()));
        let p = platform(&runner, tty.clone());
        let notice = p.copy("https://x.test/1");
        assert_eq!(notice.kind, NoticeKind::Success);
        assert_eq!(notice.text, "Copied https://x.test/1");
        let written = String::from_utf8(tty.lock().unwrap().clone()).unwrap();
        assert!(written.starts_with("\x1b]52;c;"));
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn copy_falls_back_to_a_tool_when_the_terminal_write_fails() {
        let runner = Arc::new(Fake {
            works: vec!["xclip"],
            ..Fake::default()
        });
        let p = platform(&runner, Arc::new(Mutex::new(Broken)));
        assert_eq!(p.copy("hello").kind, NoticeKind::Success);
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].0, "xclip");
        assert_eq!(calls[0].2.as_deref(), Some(b"hello".as_slice()));
    }

    #[test]
    fn copy_failure_says_what_to_do_next() {
        let runner = Arc::new(Fake::default());
        let p = platform(&runner, Arc::new(Mutex::new(Broken)));
        let notice = p.copy("x");
        assert_eq!(notice.kind, NoticeKind::Warning);
        assert!(notice.text.contains("Install wl-copy"));
    }
}
