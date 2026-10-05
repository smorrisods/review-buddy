/// Errors from platform integrations. Messages never contain secret values.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("couldn't run `{program}`: {reason}")]
    Spawn { program: String, reason: String },
    #[error("`{program}` exited unsuccessfully")]
    CommandFailed { program: String },
    #[error("only http and https links can be opened")]
    UnsupportedUrl,
    #[error("the text is too large to copy with OSC 52 ({size} bytes encoded, limit {limit})")]
    ClipboardTooLarge { size: usize, limit: usize },
    #[error("no clipboard method worked; install wl-copy, xclip, xsel or a terminal that supports OSC 52")]
    ClipboardUnavailable,
    #[error("no secure store is available here: {0}. Use auth = \"cli\", \"env:VAR\" or a token_command")]
    StoreUnavailable(String),
    #[error("the secure store failed: {0}")]
    Store(String),
    #[error("environment variable `{0}` is not set or is empty")]
    EnvMissing(String),
    #[error("no token was produced by {0}")]
    EmptyToken(String),
    #[error("invalid auth setting `{0}`")]
    InvalidAuth(String),
}
