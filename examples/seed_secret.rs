//! Seed a secrets file for benchmarking and manual trials.
//!
//! Takes a directory, writes `identity.key` and `secrets.age` into it, and
//! stores a placeholder token for each account named on the command line
//! (defaulting to Personal and Work). Values here are obvious dummies -- this
//! is never a path for real credentials.

use gitfriend::secrets::{AgeFileBackend, Backend};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("usage: seed_secret <dir> [account...]"));

    let accounts: Vec<String> = {
        let rest: Vec<String> = args.collect();
        if rest.is_empty() {
            vec!["Personal".to_string(), "Work".to_string()]
        } else {
            rest
        }
    };

    let key_path = dir.join("identity.key");
    if !key_path.exists() {
        AgeFileBackend::generate_identity_file(&key_path).unwrap();
    }
    let backend = AgeFileBackend::with_identity_file(dir.join("secrets.age"), &key_path).unwrap();

    for account in &accounts {
        backend
            .set(account, "GH_TOKEN", &format!("dummy-token-for-{account}"))
            .unwrap();
        println!("seeded {account}/GH_TOKEN");
    }
}
