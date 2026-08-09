use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use age::secrecy::ExposeSecret;
use age::x25519;

use super::{Backend, SecretError};

/// Every secret in one age-encrypted file, unlocked by an x25519 identity.
///
/// This exists because the platform keychain is unusable during development:
/// macOS keys a Keychain ACL to the calling binary's code signature, so every
/// rebuild presents as a new application and blocks on a GUI prompt -- fatal
/// for something invoked on each git transport operation (R15).
///
/// It is a real implementation rather than a test double, so the code the
/// suite exercises is the code that runs.
///
/// **An identity key, not a passphrase.** age's passphrase mode derives a key
/// with scrypt, which is deliberately slow: measured at ~1.5s per read on this
/// machine, against an R15 budget in milliseconds. Asymmetric identities skip
/// the KDF entirely. The trade is honest and worth stating: anything able to
/// read the key file can decrypt the secrets, so this protects the secrets at
/// rest -- in a backup, a sync folder, an accidental commit -- rather than
/// against a local process.
pub struct AgeFileBackend {
    path: PathBuf,
    identity: x25519::Identity,
}

/// The decrypted contents: `Account/VAR` to value.
type Entries = BTreeMap<String, String>;

impl AgeFileBackend {
    pub fn new(path: impl Into<PathBuf>, identity: x25519::Identity) -> Self {
        Self {
            path: path.into(),
            identity,
        }
    }

    /// Load the identity from a key file written by
    /// [`generate_identity_file`](Self::generate_identity_file).
    pub fn with_identity_file(
        secrets_path: impl Into<PathBuf>,
        key_path: &Path,
    ) -> Result<Self, SecretError> {
        let contents = std::fs::read_to_string(key_path)
            .map_err(|e| at(key_path, format!("cannot read identity: {e}")))?;

        let identity = contents
            .lines()
            .find(|line| !line.trim_start().starts_with('#') && !line.trim().is_empty())
            .ok_or_else(|| at(key_path, "identity file contains no key".to_string()))?;

        let identity = x25519::Identity::from_str(identity.trim())
            .map_err(|e| at(key_path, format!("invalid identity: {e}")))?;

        Ok(Self::new(secrets_path, identity))
    }

    /// Create a new identity and write it with owner-only permissions.
    ///
    /// Refuses to overwrite: replacing the key would strand every secret
    /// already encrypted to the old one.
    pub fn generate_identity_file(key_path: &Path) -> Result<(), SecretError> {
        if key_path.exists() {
            return Err(at(
                key_path,
                "identity already exists; refusing to overwrite and strand existing secrets"
                    .to_string(),
            ));
        }

        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| at(key_path, e.to_string()))?;
        }

        let identity = x25519::Identity::generate();
        let contents = format!(
            "# gitfriend identity. Anything able to read this file can decrypt\n\
             # the secrets file. Keep it out of version control and backups.\n\
             {}\n",
            identity.to_string().expose_secret()
        );
        std::fs::write(key_path, contents).map_err(|e| at(key_path, e.to_string()))?;
        restrict_permissions(key_path)
    }

    fn read_entries(&self) -> Result<Entries, SecretError> {
        if !self.path.exists() {
            return Ok(Entries::new());
        }

        let ciphertext = std::fs::read(&self.path).map_err(|e| at(&self.path, e.to_string()))?;

        let decryptor =
            age::Decryptor::new(&ciphertext[..]).map_err(|e| at(&self.path, e.to_string()))?;
        let mut reader = decryptor
            .decrypt(std::iter::once(&self.identity as &dyn age::Identity))
            .map_err(|e| at(&self.path, e.to_string()))?;

        let mut plaintext = String::new();
        reader
            .read_to_string(&mut plaintext)
            .map_err(|e| at(&self.path, e.to_string()))?;

        toml::from_str(&plaintext).map_err(|e| at(&self.path, e.to_string()))
    }

    fn write_entries(&self, entries: &Entries) -> Result<(), SecretError> {
        let plaintext = toml::to_string(entries).map_err(|e| at(&self.path, e.to_string()))?;

        let recipient = self.identity.to_public();
        let encryptor = age::Encryptor::with_recipients(std::iter::once(&recipient as _))
            .map_err(|e| at(&self.path, e.to_string()))?;

        let mut ciphertext = Vec::new();
        let mut writer = encryptor
            .wrap_output(&mut ciphertext)
            .map_err(|e| at(&self.path, e.to_string()))?;
        writer
            .write_all(plaintext.as_bytes())
            .map_err(|e| at(&self.path, e.to_string()))?;
        writer.finish().map_err(|e| at(&self.path, e.to_string()))?;

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| at(&self.path, e.to_string()))?;
        }
        std::fs::write(&self.path, &ciphertext).map_err(|e| at(&self.path, e.to_string()))?;
        restrict_permissions(&self.path)
    }
}

/// Errors describe the failure and the file, never a value.
fn at(path: &Path, message: String) -> SecretError {
    SecretError::Backend {
        account: "-".to_string(),
        var: path.display().to_string(),
        message,
    }
}

/// Encryption is the protection; owner-only permissions are a second layer so
/// a stray backup or a shared machine does not hand over the ciphertext too.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), SecretError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| at(path, e.to_string()))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), SecretError> {
    Ok(())
}

fn key(account: &str, var: &str) -> String {
    format!("{account}/{var}")
}

impl Backend for AgeFileBackend {
    fn get(&self, account: &str, var: &str) -> Result<Option<String>, SecretError> {
        Ok(self.read_entries()?.get(&key(account, var)).cloned())
    }

    fn set(&self, account: &str, var: &str, value: &str) -> Result<(), SecretError> {
        let mut entries = self.read_entries()?;
        entries.insert(key(account, var), value.to_string());
        self.write_entries(&entries)
    }

    fn delete(&self, account: &str, var: &str) -> Result<(), SecretError> {
        let mut entries = self.read_entries()?;
        entries.remove(&key(account, var));
        self.write_entries(&entries)
    }
}
