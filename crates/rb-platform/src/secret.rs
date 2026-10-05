use std::fmt;

/// A secret string that never appears in `Debug` or `Display` output.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Reveals the value. Call only where it is actually sent to a forge.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_redact() {
        let s = Secret::new("ghp_abc123");
        assert!(!format!("{s:?}").contains("abc123"));
        assert!(!format!("{s}").contains("abc123"));
        assert!(!format!("{:?}", Some(&s)).contains("abc123"));
        assert_eq!(s.expose(), "ghp_abc123");
    }
}
