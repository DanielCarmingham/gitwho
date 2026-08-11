//! Storage for token values.
//!
//! Values live here and nowhere else: never in `accounts.toml` (R10), never in
//! the ambient environment, and never in this crate's output. Only
//! [`fingerprint`] may be printed.

mod age_file;
mod env;
mod keychain;
pub mod select;

pub use age_file::{AgeFileBackend, Protection};
pub use env::EnvBackend;
pub use keychain::KeychainBackend;
pub use select::{choose, BackendKind, Choice, Platform, SelectError, Source};

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("the value was empty; nothing stored")]
    Empty,
    #[error("cannot read the value: {0}")]
    Read(#[from] std::io::Error),
    #[error("secret store failed for {account}/{var}: {message}")]
    Backend {
        account: String,
        var: String,
        message: String,
    },
}

/// Read and write token values, keyed by the account that owns them and the
/// variable they populate.
///
/// Keying on both means one account can hold several distinct credentials, and
/// a checker can say precisely which variable is missing a value.
pub trait Backend {
    /// The stored value, or `None` when nothing is stored.
    ///
    /// Absence is deliberately not an error: a caller must be able to tell
    /// "no value stored" from "the store is broken", because conflating them
    /// is how a missing credential quietly becomes a fallback.
    fn get(&self, account: &str, var: &str) -> Result<Option<String>, SecretError>;

    fn set(&self, account: &str, var: &str, value: &str) -> Result<(), SecretError>;

    /// Remove the value. Deleting something already absent succeeds.
    fn delete(&self, account: &str, var: &str) -> Result<(), SecretError>;
}

/// A short, stable identifier for a value that reveals nothing about it.
///
/// This is the only representation of a secret that may appear in output,
/// logs, or error messages. It exists so a checker can say "these two are the
/// same" or "this one changed" without ever printing token material.
pub fn fingerprint(value: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(value.as_bytes());
    // 12 hex characters -- enough to distinguish a handful of tokens by eye,
    // far too little to attack the preimage.
    digest[..6].iter().map(|b| format!("{b:02x}")).collect()
}

/// Read a secret value from non-interactive input.
///
/// Surrounding whitespace is stripped, because every ordinary way of supplying
/// a value adds some: `echo` appends a newline, a heredoc appends a newline, a
/// paste ends with Enter. Sent as part of the token that whitespace is
/// rejected by the server with an error that mentions nothing about
/// whitespace, which is a genuinely hard afternoon.
///
/// An empty result is an error rather than an empty secret: a stored empty
/// string would satisfy every "is it present?" check while authenticating as
/// nobody.
pub fn read_value_from(reader: &mut impl std::io::Read) -> Result<String, SecretError> {
    let mut raw = String::new();
    reader.read_to_string(&mut raw)?;

    let value = raw.trim();
    if value.is_empty() {
        return Err(SecretError::Empty);
    }
    Ok(value.to_string())
}
