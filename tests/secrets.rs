use std::collections::HashMap;

use gitwho::secrets::{Backend, EnvBackend};

/// The env backend exists to read the scheme people already have: a shell
/// rc file exporting `GH_TOKEN_<Account>` and friends. Keeping it readable is
/// what makes migration a copy rather than a re-issue of every token.
#[test]
fn the_env_backend_reads_the_var_suffixed_with_the_account_name() {
    let env = HashMap::from([("GH_TOKEN_Work".to_string(), "token-value".to_string())]);
    let backend = EnvBackend::from_map(env);

    let found = backend.get("Work", "GH_TOKEN").unwrap();

    assert_eq!(found.as_deref(), Some("token-value"));
}

#[test]
fn a_fingerprint_does_not_contain_the_value_it_identifies() {
    let secret = "ghp_exampletokenmaterial1234567890";

    let fp = gitwho::secrets::fingerprint(secret);

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
    let fp = gitwho::secrets::fingerprint("token-a");

    assert_eq!(fp, gitwho::secrets::fingerprint("token-a"));
    assert_ne!(fp, gitwho::secrets::fingerprint("token-b"));
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
    use gitwho::secrets::KeychainBackend;

    let backend = KeychainBackend::with_service("gitwho-selftest");
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

fn test_backend(dir: &tempfile::TempDir) -> gitwho::secrets::AgeFileBackend {
    let key_path = dir.path().join("identity.key");
    gitwho::secrets::AgeFileBackend::generate_identity_file(&key_path).unwrap();
    gitwho::secrets::AgeFileBackend::with_identity_file(dir.path().join("secrets.age"), &key_path)
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
    gitwho::secrets::AgeFileBackend::generate_identity_file(&key_path).unwrap();
    let backend =
        gitwho::secrets::AgeFileBackend::with_identity_file(path.clone(), &key_path).unwrap();

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

/// The claim the backend makes about permissions has to be checkable, because
/// on a platform where it cannot be kept the silence is indistinguishable from
/// success. Here the claim is tied to the mode actually on disk; where it
/// cannot be made, `doctor` says so instead.
#[test]
fn the_backend_states_what_protection_it_could_actually_apply() {
    use gitwho::secrets::Protection;

    let dir = tempfile::tempdir().unwrap();
    let backend = test_backend(&dir);
    backend.set("Work", "GH_TOKEN", "token-value").unwrap();

    let owner_only = gitwho::secrets::AgeFileBackend::protection() == Protection::OwnerOnly;

    // The claim must track the platform. Flipping the cfg behind
    // `Protection::HOST` without meaning to would silently turn the check below
    // off; this is what stops that.
    assert_eq!(
        owner_only,
        cfg!(unix),
        "unix can apply owner-only permissions and nothing else here can"
    );

    if owner_only {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["identity.key", "secrets.age"] {
                let mode = std::fs::metadata(dir.path().join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(mode, 0o600, "{name} claims owner-only but is {mode:o}");
            }
        }
    }
    // Where it cannot be claimed there is nothing on disk to check; the promise
    // on that platform is that `doctor` reports the gap, which tests/doctor.rs
    // covers. Unverified: there is no Windows machine here.
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

// --- reading a value from non-interactive input -----------------------------

#[test]
fn a_pasted_value_loses_its_trailing_newline() {
    // Piping through `echo`, a heredoc, or a paste that ends in Enter all add
    // one. Sent as part of the token it is rejected by the server, with an
    // error that says nothing about whitespace.
    let value = gitwho::secrets::read_value_from(&mut "tok-value\n".as_bytes()).unwrap();

    assert_eq!(value, "tok-value");
}

#[test]
fn surrounding_whitespace_from_a_paste_is_removed() {
    let value = gitwho::secrets::read_value_from(&mut "  tok-value \r\n".as_bytes()).unwrap();

    assert_eq!(value, "tok-value");
}

#[test]
fn an_empty_value_is_refused_rather_than_stored() {
    // Storing an empty string would satisfy every "is it present?" check while
    // authenticating as nobody.
    let error = gitwho::secrets::read_value_from(&mut "   \n".as_bytes())
        .expect_err("an empty value must be refused");

    assert!(error.to_string().contains("empty"), "got: {error}");
}

// --- a store that is never asked for anything --------------------------------

/// An account whose variables all come from another tool never touches the
/// store. Refusing to run because there is no `identity.key` would make such a
/// machine set up a secret store it will never read -- which contradicts the
/// point of referencing a token rather than copying it.
#[test]
fn a_store_that_failed_to_open_costs_nothing_until_it_is_used() {
    let broken =
        gitwho::secrets::DeferredBackend::new(Err(gitwho::secrets::SecretError::Backend {
            account: "-".to_string(),
            var: "/nowhere/identity.key".to_string(),
            message: "cannot read identity: No such file or directory".to_string(),
        }));

    // Constructing it is not an error, and nothing has been read.
    let error = broken
        .get("Personal", "GH_TOKEN")
        .expect_err("asking a store that could not open must fail");

    let message = error.to_string();

    // It fails where the account and variable are known, so the message can
    // name them -- which the eager version could not.
    assert!(message.contains("Personal/GH_TOKEN"), "{message}");
    assert!(message.contains("/nowhere/identity.key"), "{message}");
    assert!(message.contains("cannot read identity"), "{message}");

    // And it says it once. Re-wrapping a Backend error whole produced
    // "secret store failed for X: secret store failed for Y".
    assert_eq!(
        message.matches("secret store failed").count(),
        1,
        "the failure is reported twice: {message}"
    );
}

/// Deferring the failure must not swallow it. Writing is the case where a
/// silent success would be worst.
#[test]
fn a_store_that_failed_to_open_still_refuses_to_write() {
    let broken =
        gitwho::secrets::DeferredBackend::new(Err(gitwho::secrets::SecretError::Backend {
            account: "-".to_string(),
            var: "/nowhere/identity.key".to_string(),
            message: "cannot read identity".to_string(),
        }));

    assert!(broken.set("Personal", "GH_TOKEN", "value").is_err());
    assert!(broken.delete("Personal", "GH_TOKEN").is_err());
}

/// A store that opened fine is passed through untouched.
#[test]
fn a_store_that_opened_behaves_exactly_as_itself() {
    let dir = tempfile::tempdir().unwrap();
    let deferred = gitwho::secrets::DeferredBackend::new(Ok(Box::new(test_backend(&dir))));

    deferred.set("Work", "GH_TOKEN", "stored-value").unwrap();

    assert_eq!(
        deferred.get("Work", "GH_TOKEN").unwrap().as_deref(),
        Some("stored-value")
    );
    // Absence is still absence, not a deferred failure.
    assert_eq!(deferred.get("Work", "MISSING").unwrap(), None);
}
