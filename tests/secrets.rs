use std::collections::HashMap;

use gitfriend::secrets::{Backend, EnvBackend};

/// The env backend exists to read the scheme already on this machine:
/// `~/.zshrc.local` exports `GH_TOKEN_DanielAtProfound` and friends. Keeping
/// it readable is what makes migration a copy rather than a re-issue.
#[test]
fn the_env_backend_reads_the_var_suffixed_with_the_account_name() {
    let env = HashMap::from([(
        "GH_TOKEN_DanielAtProfound".to_string(),
        "token-value".to_string(),
    )]);
    let backend = EnvBackend::from_map(env);

    let found = backend.get("DanielAtProfound", "GH_TOKEN").unwrap();

    assert_eq!(found.as_deref(), Some("token-value"));
}

#[test]
fn a_fingerprint_does_not_contain_the_value_it_identifies() {
    let secret = "ghp_exampletokenmaterial1234567890";

    let fp = gitfriend::secrets::fingerprint(secret);

    assert!(
        !fp.contains("exampletokenmaterial"),
        "fingerprint leaked the secret: {fp}"
    );
    assert!(
        !secret.contains(&fp),
        "fingerprint is a substring of the secret: {fp}"
    );
}

#[test]
fn fingerprints_distinguish_different_values_and_are_stable() {
    let fp = gitfriend::secrets::fingerprint("token-a");

    assert_eq!(fp, gitfriend::secrets::fingerprint("token-a"));
    assert_ne!(fp, gitfriend::secrets::fingerprint("token-b"));
}

#[test]
fn a_missing_secret_is_absence_not_an_error() {
    // Callers must be able to tell "no value stored" from "the store broke".
    // Conflating them is how a missing credential turns into a fallback.
    let backend = EnvBackend::from_map(HashMap::new());

    let found = backend.get("NoSuchAccount", "GH_TOKEN").unwrap();

    assert_eq!(found, None);
}

#[test]
#[ignore = "touches the real login keychain; run explicitly with --ignored"]
fn the_keychain_backend_round_trips_and_deletes() {
    use gitfriend::secrets::KeychainBackend;

    let backend = KeychainBackend::with_service("gitfriend-selftest");
    let _ = backend.delete("SelfTest", "GH_TOKEN");

    backend.set("SelfTest", "GH_TOKEN", "value-one").unwrap();
    assert_eq!(
        backend.get("SelfTest", "GH_TOKEN").unwrap().as_deref(),
        Some("value-one")
    );

    backend.delete("SelfTest", "GH_TOKEN").unwrap();
    assert_eq!(backend.get("SelfTest", "GH_TOKEN").unwrap(), None);
}

// --- encrypted file backend -------------------------------------------------
//
// The keychain is impractical for development and tests: macOS keys its ACL to
// the calling binary, so every rebuild blocks on a GUI prompt. This backend is
// the substitute -- a real age-encrypted file, not a stub, so the code under
// test is the code that ships.

fn test_backend(dir: &tempfile::TempDir) -> gitfriend::secrets::AgeFileBackend {
    let key_path = dir.path().join("identity.key");
    gitfriend::secrets::AgeFileBackend::generate_identity_file(&key_path).unwrap();
    gitfriend::secrets::AgeFileBackend::with_identity_file(dir.path().join("secrets.age"), &key_path)
        .unwrap()
}

#[test]
fn the_age_backend_round_trips_a_value() {
    let dir = tempfile::tempdir().unwrap();
    let backend = test_backend(&dir);

    backend.set("Work", "GH_TOKEN", "token-value").unwrap();

    assert_eq!(
        backend.get("Work", "GH_TOKEN").unwrap().as_deref(),
        Some("token-value")
    );
}

#[test]
fn the_age_backend_never_writes_plaintext_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.age");
    let key_path = dir.path().join("identity.key");
    gitfriend::secrets::AgeFileBackend::generate_identity_file(&key_path).unwrap();
    let backend =
        gitfriend::secrets::AgeFileBackend::with_identity_file(path.clone(), &key_path).unwrap();

    backend
        .set("Work", "GH_TOKEN", "supersecrettokenvalue")
        .unwrap();

    let on_disk = std::fs::read(&path).unwrap();
    let as_text = String::from_utf8_lossy(&on_disk);
    assert!(
        !as_text.contains("supersecrettokenvalue"),
        "the token value appeared verbatim in the encrypted file"
    );
    // The variable name is not secret, but leaking it would still tell an
    // attacker what to go looking for.
    assert!(
        !as_text.contains("GH_TOKEN"),
        "the variable name appeared verbatim in the encrypted file"
    );
}

#[test]
fn reading_a_secret_is_cheap_enough_for_the_git_hot_path() {
    // R15: resolution runs on every git transport operation, so a read must
    // stay in the millisecond range. A passphrase-derived key costs ~1.5s here
    // because scrypt is deliberately slow -- this test is what stops that being
    // reintroduced. The bound is loose so it catches a KDF, not jitter.
    let dir = tempfile::tempdir().unwrap();
    let backend = test_backend(&dir);
    backend.set("Work", "GH_TOKEN", "token-value").unwrap();

    let start = std::time::Instant::now();
    backend.get("Work", "GH_TOKEN").unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_millis(100),
        "a secret read took {elapsed:?}, too slow for the git hot path"
    );
}
