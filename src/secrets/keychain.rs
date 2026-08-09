use super::{Backend, SecretError};

/// The default macOS Keychain service name. One service, many entries -- the
/// entry's "account" field carries `<Account>/<VAR>`.
const DEFAULT_SERVICE: &str = "gitfriend";

/// Stores values in the platform credential store: macOS Keychain here,
/// Credential Manager on Windows, Secret Service on Linux.
///
/// Unlike an environment variable, a value here is fetched on demand by the
/// one process that needs it, so it is never visible to unrelated tooling
/// launched from the same shell (R11).
///
/// **Not usable for development, measured 2026-08-09.** macOS keys a Keychain
/// ACL to the calling binary's designated requirement. For an unsigned binary
/// that is its code hash, so *every rebuild* presents as a new application and
/// the read blocks on a GUI prompt -- verified by writing an entry with one
/// build and reading it with the next, which hung until killed. Since the
/// credential helper runs on every git transport operation, that is fatal
/// (R15).
///
/// Making this viable needs the installed binary signed with a stable identity
/// so its designated requirement survives rebuilds. Until that is set up and
/// re-verified, use [`AgeFileBackend`](super::AgeFileBackend).
pub struct KeychainBackend {
    service: String,
}

impl KeychainBackend {
    pub fn new() -> Self {
        Self::with_service(DEFAULT_SERVICE)
    }

    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str, var: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, &key(account, var)).map_err(|e| SecretError::Backend {
            account: account.to_string(),
            var: var.to_string(),
            message: e.to_string(),
        })
    }
}

impl Default for KeychainBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Entries are keyed by account *and* variable, so one account can hold
/// several distinct credentials and a checker can name exactly which one is
/// missing.
fn key(account: &str, var: &str) -> String {
    format!("{account}/{var}")
}

fn backend_error(account: &str, var: &str, e: keyring::Error) -> SecretError {
    SecretError::Backend {
        account: account.to_string(),
        var: var.to_string(),
        // `e` describes the failure, never the value.
        message: e.to_string(),
    }
}

impl Backend for KeychainBackend {
    fn get(&self, account: &str, var: &str) -> Result<Option<String>, SecretError> {
        match self.entry(account, var)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(backend_error(account, var, e)),
        }
    }

    fn set(&self, account: &str, var: &str, value: &str) -> Result<(), SecretError> {
        self.entry(account, var)?
            .set_password(value)
            .map_err(|e| backend_error(account, var, e))
    }

    fn delete(&self, account: &str, var: &str) -> Result<(), SecretError> {
        match self.entry(account, var)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(backend_error(account, var, e)),
        }
    }
}
