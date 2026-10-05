use crate::{CommandRunner, Os, PlatformError};

/// Opens `url` in the system browser using the current OS's opener.
pub fn open_url(url: &str, runner: &dyn CommandRunner) -> Result<(), PlatformError> {
    open_url_for(Os::current(), url, runner)
}

/// Program and arguments used to open `url` on `os`.
pub fn open_command(os: Os, url: &str) -> (&'static str, Vec<String>) {
    match os {
        Os::Linux => ("xdg-open", vec![url.to_string()]),
        Os::MacOs => ("open", vec![url.to_string()]),
        // rundll32 avoids cmd.exe re-parsing `&` in query strings.
        Os::Windows => (
            "rundll32",
            vec!["url.dll,FileProtocolHandler".to_string(), url.to_string()],
        ),
    }
}

pub fn open_url_for(os: Os, url: &str, runner: &dyn CommandRunner) -> Result<(), PlatformError> {
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.contains(char::is_control)
    {
        return Err(PlatformError::UnsupportedUrl);
    }
    let (program, args) = open_command(os, url);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = runner.run(program, &args, None)?;
    if out.success {
        Ok(())
    } else {
        Err(PlatformError::CommandFailed {
            program: program.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::fake::FakeRunner;

    #[test]
    fn picks_opener_per_os() {
        for (os, prog) in [
            (Os::Linux, "xdg-open"),
            (Os::MacOs, "open"),
            (Os::Windows, "rundll32"),
        ] {
            let r = FakeRunner::default().with(prog, true, "", "");
            open_url_for(os, "https://github.com/a/b/pull/1?x=1&y=2", &r).unwrap();
            let calls = r.calls.borrow();
            assert_eq!(calls[0].0, prog);
            assert!(calls[0].1.last().unwrap().contains("x=1&y=2"));
        }
    }

    #[test]
    fn rejects_non_http_urls_without_running() {
        let r = FakeRunner::default();
        for u in ["file:///etc/passwd", "javascript:alert(1)", "https://a\nb"] {
            assert!(matches!(
                open_url_for(Os::Linux, u, &r),
                Err(PlatformError::UnsupportedUrl)
            ));
        }
        assert!(r.calls.borrow().is_empty());
    }

    #[test]
    fn reports_failure() {
        let r = FakeRunner::default().with("xdg-open", false, "", "");
        assert!(matches!(
            open_url_for(Os::Linux, "https://x.test", &r),
            Err(PlatformError::CommandFailed { .. })
        ));
        let r = FakeRunner::default();
        assert!(matches!(
            open_url_for(Os::Linux, "https://x.test", &r),
            Err(PlatformError::Spawn { .. })
        ));
    }
}
