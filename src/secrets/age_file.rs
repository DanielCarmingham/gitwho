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

/// How far this platform lets gitwho close the file down.
///
/// Named rather than left implicit because the weaker answer must be *reported*
/// (`doctor`), not silently accepted: a no-op that looks like success is how a
/// file ends up readable by everyone who can reach the directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    /// `0600`, applied by gitwho.
    OwnerOnly,
    /// Whatever the containing directory grants. On a default Windows profile
    /// that is owner plus SYSTEM plus Administrators -- weaker than `0600`, not
    /// world-readable. Unverified: there is no Windows machine here.
    DirectoryInherited,
}

impl Protection {
    /// What [`restrict_permissions`] can achieve on the platform this was built
    /// for. Stated once, so the two `cfg` arms below cannot drift from it.
    pub const HOST: Protection = if cfg!(unix) {
        Protection::OwnerOnly
    } else {
        Protection::DirectoryInherited
    };
}

impl AgeFileBackend {
    /// What the files this backend writes are actually protected by.
    pub fn protection() -> Protection {
        Protection::HOST
    }

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
    /// already encrypted to the old one. The refusal is the `open` itself
    /// rather than a preceding `exists`, so two `secret init`s racing cannot
    /// both decide the file was absent.
    pub fn generate_identity_file(key_path: &Path) -> Result<(), SecretError> {
        if let Some(parent) = key_path.parent() {
            create_store_dir(parent).map_err(|e| at(key_path, e.to_string()))?;
        }

        let identity = x25519::Identity::generate();
        let contents = format!(
            "# gitwho identity. Anything able to read this file can decrypt\n\
             # the secrets file. Keep it out of version control and backups.\n\
             {}\n",
            identity.to_string().expose_secret()
        );

        create_owner_only(key_path, contents.as_bytes()).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                at(
                    key_path,
                    "identity already exists; refusing to overwrite and strand existing secrets"
                        .to_string(),
                )
            } else {
                at(key_path, e.to_string())
            }
        })
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
            create_store_dir(parent).map_err(|e| at(&self.path, e.to_string()))?;
        }
        write_owner_only(&self.path, &ciphertext).map_err(|e| at(&self.path, e.to_string()))?;
        // The mode above only applies to a file this call created; one that was
        // already there keeps whatever it had, so it is narrowed here.
        restrict_permissions(&self.path)
    }
}

/// Create the directory the store lives in, owner-only.
///
/// `create_dir_all` applies the umask, so the ubiquitous `022` left this `0755`
/// -- and the directory is the whole reason anything below it is out of reach,
/// which is why `doctor` fails on anything but `0700`. A fresh install used to
/// fail that check on permissions gitwho itself had set.
///
/// Only a directory this call actually creates is closed down. `GITWHO_*`
/// can point the store at a directory that already exists and belongs to
/// something else -- chmodding *that* would be a side effect nobody asked for,
/// and `doctor` is what reports one that has since drifted.
#[cfg(unix)]
fn create_store_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    // `mode` would apply to every directory a recursive create makes, so the
    // parents are made at the default and only the leaf is narrowed.
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_store_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Ask for owner-only *at creation*, where the platform can express it.
///
/// The point is the ordering: `fs::write` then chmod leaves the file complete
/// on disk at whatever the umask allows for the width of the chmod, and these
/// are the identity key and every stored token. A descriptor opened in that
/// window stays open afterwards.
#[cfg(unix)]
fn owner_only(options: &mut std::fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;

    options.mode(0o600);
}

/// See [`restrict_permissions`]: nothing here can express it, and the gap is
/// carried in [`Protection::HOST`] rather than papered over.
#[cfg(not(unix))]
fn owner_only(_options: &mut std::fs::OpenOptions) {}

/// Write `contents` to a file that must not already exist.
fn create_owner_only(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    owner_only(&mut options);
    options.open(path)?.write_all(contents)
}

/// Write `contents`, replacing whatever was there.
fn write_owner_only(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    owner_only(&mut options);
    options.open(path)?.write_all(contents)
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

/// Nothing to do, and -- unlike `shim::make_executable`'s no-op, where a `.cmd`
/// genuinely needs no bit -- that is a real loss, not an irrelevance. Writing
/// Windows ACLs here would be unverifiable from this machine, and refusing to
/// write at all would make the only usable backend there unusable. So the gap
/// is carried in [`Protection::HOST`] and reported by `doctor` instead of being
/// closed with code nobody has run.
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
