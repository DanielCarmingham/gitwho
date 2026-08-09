use std::collections::HashMap;

use super::{Backend, SecretError};

/// Reads `<VAR>_<Account>` from a set of environment variables.
///
/// This is the scheme `~/.zshrc.local` already uses, so it is what makes
/// migrating an existing machine a copy rather than a token re-issue. It is a
/// migration and CI path, not the default: values held this way are readable
/// by every process in the shell.
pub struct EnvBackend {
    vars: HashMap<String, String>,
}

impl EnvBackend {
    pub fn from_map(vars: HashMap<String, String>) -> Self {
        Self { vars }
    }
}

impl Backend for EnvBackend {
    fn get(&self, account: &str, var: &str) -> Result<Option<String>, SecretError> {
        Ok(self.vars.get(&format!("{var}_{account}")).cloned())
    }

    fn set(&self, account: &str, var: &str, _value: &str) -> Result<(), SecretError> {
        Err(read_only(account, var))
    }

    fn delete(&self, account: &str, var: &str) -> Result<(), SecretError> {
        Err(read_only(account, var))
    }
}

/// Writing would mean editing a shell startup file, which is exactly the
/// arrangement this backend exists to migrate away from. Refuse loudly rather
/// than pretend to store something.
fn read_only(account: &str, var: &str) -> SecretError {
    SecretError::Backend {
        account: account.to_string(),
        var: var.to_string(),
        message: "the env backend is read-only; store this in the keychain instead".to_string(),
    }
}
