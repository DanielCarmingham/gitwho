//! Checks that a *rebuilt* binary can still read a Keychain entry an earlier
//! build wrote, without macOS prompting for access.
//!
//! This matters because resolution runs on every git transport operation: a
//! prompt there would make the tool unusable (R15). macOS keys Keychain ACLs
//! to the calling binary's designated requirement, so a changed signature
//! reintroduces prompting.
//!
//! **Result on 2026-08-09, unsigned binary: FAILS.** `write` succeeded, and
//! after `touch` + rebuild the `read` blocked indefinitely on a GUI prompt.
//! That is why `AgeFileBackend` exists. Signing the installed binary with a
//! stable identity should fix it -- this probe is how to confirm that, and it
//! has not been confirmed yet.
//!
//! Re-run after changing how the binary is built or signed:
//!
//!     cargo run --example keychain_probe -- write
//!     touch src/secrets/keychain.rs && cargo build --example keychain_probe
//!     cargo run --example keychain_probe -- read
//!     cargo run --example keychain_probe -- clean

use gitwho::secrets::{fingerprint, Backend, KeychainBackend};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let backend = KeychainBackend::with_service("gitwho-selftest");
    let (account, var) = ("RebuildProbe", "GH_TOKEN");

    match mode.as_str() {
        "write" => {
            backend.set(account, var, "probe-value").unwrap();
            println!("wrote {account}/{var}");
        }
        "read" => match backend.get(account, var).unwrap() {
            // Print the fingerprint, never the value -- even in a diagnostic.
            Some(value) => println!("read {account}/{var} fingerprint={}", fingerprint(&value)),
            None => {
                eprintln!("no entry found");
                std::process::exit(1);
            }
        },
        "clean" => {
            backend.delete(account, var).unwrap();
            println!("deleted {account}/{var}");
        }
        other => {
            eprintln!("usage: keychain_probe write|read|clean (got {other:?})");
            std::process::exit(2);
        }
    }
}
