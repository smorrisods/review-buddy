//! The pager hook for long output on a terminal.

use std::io::Write;
use std::process::{Command, Stdio};

/// Picks the pager command: `$REVIEW_BUDDY_PAGER`, else `$PAGER`, else `less -FRX`.
/// `cat` (or an unusable value) means no pager.
pub fn resolve_pager(review_buddy_pager: Option<&str>, pager: Option<&str>) -> Option<Vec<String>> {
    let chosen = [review_buddy_pager, pager]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("less -FRX");
    let words: Vec<String> = chosen.split_whitespace().map(str::to_string).collect();
    let program = words.first()?;
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    (name != "cat").then_some(words)
}

/// Sends `text` through `pager`, or straight to stdout if there is no pager or it can't start.
pub fn page(text: &str, pager: Option<&[String]>) -> std::io::Result<()> {
    if let Some([program, args @ ..]) = pager {
        let spawned = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .spawn();
        if let Ok(mut child) = spawned {
            if let Some(mut stdin) = child.stdin.take() {
                if let Err(e) = stdin.write_all(text.as_bytes()) {
                    if e.kind() != std::io::ErrorKind::BrokenPipe {
                        return Err(e);
                    }
                }
            }
            child.wait()?;
            return Ok(());
        }
    }
    std::io::stdout().lock().write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[&str]) -> Option<Vec<String>> {
        Some(items.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn review_buddy_pager_beats_pager_beats_less() {
        assert_eq!(
            resolve_pager(Some("bat -p"), Some("more")),
            words(&["bat", "-p"])
        );
        assert_eq!(resolve_pager(None, Some("more")), words(&["more"]));
        assert_eq!(resolve_pager(None, None), words(&["less", "-FRX"]));
        assert_eq!(
            resolve_pager(Some(""), Some("  ")),
            words(&["less", "-FRX"])
        );
    }

    #[test]
    fn cat_turns_the_pager_off() {
        assert_eq!(resolve_pager(Some("cat"), Some("less")), None);
        assert_eq!(resolve_pager(Some("/bin/cat"), None), None);
    }

    #[test]
    fn a_missing_pager_falls_back_to_stdout() {
        let bogus = vec!["review-buddy-no-such-pager".to_string()];
        assert!(page("", Some(&bogus)).is_ok());
    }
}
