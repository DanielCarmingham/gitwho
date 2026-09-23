use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let version = env::var("CARGO_PKG_VERSION").expect("set by cargo");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    let root = Path::new(&manifest_dir);

    println!("cargo:rerun-if-env-changed=GITHUB_REF");

    // Release builds are the two that do not compile from a checkout: the
    // crates.io tarball, which carries no `.git`, and dist's jobs on a tag push.
    // `.git` is looked for beside Cargo.toml only -- asking git would walk up
    // and let any enclosing repository claim a registry build as its own.
    let from_checkout = root.join(".git").exists();
    let tag_build = env::var("GITHUB_REF").is_ok_and(|r| r == format!("refs/tags/v{version}"));

    // `cargo package` verifies in the same target dir, under the same unit
    // fingerprint as a checkout build, so without this the next plain `cargo
    // build` inherits this run's output and reports a release. A path that does
    // not exist is permanently stale, which forces that build to rerun us.
    if !from_checkout {
        println!("cargo:rerun-if-changed={}", root.join(".git").display());
    }

    let full = if !from_checkout || tag_build {
        version
    } else {
        watch_head(root);
        match git(root, &["rev-parse", "--short=7", "HEAD"]) {
            Some(commit) => format!("{version}-dev+{commit}"),
            None => format!("{version}-dev"),
        }
    };
    println!("cargo:rustc-env=GITWHO_VERSION={full}");
}

fn watch_head(root: &Path) {
    for path in ["HEAD", "logs/HEAD"] {
        if let Some(resolved) = git(root, &["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={}", root.join(resolved).display());
        }
    }
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_owned())
}
