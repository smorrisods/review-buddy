use std::collections::HashMap;
use std::sync::Mutex;

use crate::{PlatformError, Secret};

/// Keyring service name; the account is the host, e.g. `github.com`.
pub const SERVICE: &str = "review-buddy";

/// Secure storage for per-host tokens.
pub trait SecretStore: Send + Sync {
    fn get(&self, host: &str) -> Result<Option<Secret>, PlatformError>;
    fn set(&self, host: &str, secret: &Secret) -> Result<(), PlatformError>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, host: &str) -> Result<(), PlatformError>;
}

/// In-memory store for tests and demo mode.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<String, Secret>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Secret>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, host: &str) -> Result<Option<Secret>, PlatformError> {
        Ok(self.lock().get(host).cloned())
    }

    fn set(&self, host: &str, secret: &Secret) -> Result<(), PlatformError> {
        self.lock().insert(host.to_string(), secret.clone());
        Ok(())
    }

    fn delete(&self, host: &str) -> Result<(), PlatformError> {
        self.lock().remove(host);
        Ok(())
    }
}

/// OS keyring: Secret Service (pure-Rust zbus) on Linux, Keychain on macOS,
/// Credential Manager on Windows.
#[cfg(feature = "keyring")]
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

#[cfg(feature = "keyring")]
impl KeyringStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(host: &str) -> Result<keyring::Entry, PlatformError> {
        keyring::Entry::new(SERVICE, host).map_err(map_err)
    }
}

#[cfg(feature = "keyring")]
fn map_err(e: keyring::Error) -> PlatformError {
    match e {
        keyring::Error::NoStorageAccess(inner) => {
            PlatformError::StoreUnavailable(inner.to_string())
        }
        keyring::Error::PlatformFailure(inner) => {
            PlatformError::StoreUnavailable(inner.to_string())
        }
        // The Display of other variants can echo attribute data but not the secret.
        other => PlatformError::Store(other.to_string()),
    }
}

#[cfg(feature = "keyring")]
impl SecretStore for KeyringStore {
    fn get(&self, host: &str) -> Result<Option<Secret>, PlatformError> {
        match Self::entry(host)?.get_password() {
            Ok(p) => Ok(Some(Secret::new(p))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }

    fn set(&self, host: &str, secret: &Secret) -> Result<(), PlatformError> {
        Self::entry(host)?
            .set_password(secret.expose())
            .map_err(map_err)
    }

    fn delete(&self, host: &str) -> Result<(), PlatformError> {
        match Self::entry(host)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let s = MemorySecretStore::new();
        assert!(s.get("github.com").unwrap().is_none());
        s.set("github.com", &Secret::new("tok")).unwrap();
        assert_eq!(s.get("github.com").unwrap().unwrap().expose(), "tok");
        s.delete("github.com").unwrap();
        s.delete("github.com").unwrap();
        assert!(s.get("github.com").unwrap().is_none());
        assert!(!format!("{s:?}").contains("tok\""));
    }
}
