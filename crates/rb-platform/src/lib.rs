//! Per-OS integrations: open a URL, copy to the clipboard, keep secrets.
//!
//! Everything that spawns a process goes through [`CommandRunner`] so tests
//! never touch the real system.

pub mod auth;
pub mod browser;
pub mod clipboard;
mod error;
pub mod runner;
pub mod secret;
pub mod store;

pub use error::PlatformError;
pub use runner::{CommandOutput, CommandRunner, SystemRunner};
pub use secret::Secret;
pub use store::{MemorySecretStore, SecretStore, SERVICE};

#[cfg(feature = "keyring")]
pub use store::KeyringStore;

/// Operating systems with distinct integration behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}
