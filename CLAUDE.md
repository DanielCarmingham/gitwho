# gitwho

Pick the right git identity **and** the right credentials for a repository —
automatically, wherever that repository lives on disk — across multiple hosted
git providers and the CLIs/MCP servers that talk to them.

Read [docs/DESIGN.md](docs/DESIGN.md) before proposing anything. It holds the
R1–R15 principles the source refers to by number, and the measured evidence
each one rests on. [README.md](README.md) is the short version;
[docs/INSTALL.md](docs/INSTALL.md) is the setup path.

## Status

**Built, tested, and in use** on the author's machine since 2026-08-10 —
resolver, credential helper, secret storage, `exec` + shims, `doctor`, `sync`
and MCP wrapping all route real traffic. 161 tests, clippy clean.

**macOS is where it runs daily. Linux is now exercised, not assumed:** the full
suite (138 tests as it stood then) plus the whole `init` flow — `0700`/`0600` modes, identity
resolution, the credential helper, a shim executed with the shim dir first on
`PATH`, and the `.bashrc` branch — pass on Debian bookworm/aarch64 under
`rust:1.88`. Reproduce with `docker run --rm -v "$PWD:/src:ro" -w /work
rust:1.88-bookworm`, copying the tree in rather than building in the mount.
x86-64 Linux is still only covered by CI.

**Windows is unverified in the strong sense.** `%APPDATA%\gitwho` in
`src/paths.rs`, the `.cmd` shim and `PATHEXT` lookup in `src/shim.rs`, and the
fact that gitwho applies no ACLs there (`Protection::DirectoryInherited`, so
`doctor` warns rather than the file being closed down). All are unit-tested as
pure functions from macOS; none has ever run on Windows. **Do not describe them
as working.**

The mechanism that makes that testable is a **parameter, never a `cfg!`**:
`paths::Layout` and `shim::ShimTarget` are arguments, with `HOST` used only by
`main`. A `cfg!(windows)` branch is unreachable from a test run here, so it can
be documented as covered while nothing can reach it — which is exactly what
happened to an earlier `APPDATA` branch. The test binaries are
`#[cfg(unix)]`-gated around anything touching `std::os::unix`, so `cargo test`
still *builds* elsewhere.

## Ground rules

- **Evidence, not assumption.** Every claim in `docs/DESIGN.md` was verified
  with a command. Keep that bar: if you assert direnv/git/gh behaviour, run it
  first and record the tool version alongside the result.
- **Wrong-and-quiet is worse than broken-and-loud** (R8). Any fallback that
  produces a *working but incorrect* account is a bug, not a convenience. This
  is why `Resolved` carries a `reason` and why `Unmatched` is distinct from
  `Default`.
- **Secrets never enter this repo** (R10). Config that *names* a variable is
  tracked; values live only in the store. Never echo a token value —
  fingerprints or prefixes only, including in test output and error messages.
- **No global mutable credential state** (R9). No `gh auth switch`-style
  process-wide active account. Per-process / per-invocation only.
- **Resolution is on the hot path** (R15). It runs on every CLI invocation and
  every git transport operation; budget is single-digit-to-low-double-digit
  milliseconds. A regression test pins secret reads under 100 ms so a KDF
  cannot creep back in.
- **Nothing personal in the repo.** Fixtures and examples use `example.com`,
  `acme-*` and placeholder account names. Real accounts, orgs, emails and
  hostnames belong in `~/.config/gitwho/accounts.toml`, never here.
- YAGNI. This replaces a working-ish shell setup; scope creep is the main risk.

## Tooling

### Version control: `jj` (colocated on top of git)

This repo is a **colocated** jj repo (`.jj/` and `.git/` side by side), so git
tooling still works, but **drive it with `jj`**:

- `jj st` — working-copy status. There is no staging area; the working copy is
  always a commit.
- `jj describe -m "msg"` — set the current change's message.
- `jj new` — start a new change on top (the usual "commit and move on").
- `jj log` — history. `jj diff` — current change's diff.
- `jj file untrack <path>` — stop tracking a file that is now gitignored. It
  must be ignored first, or the command refuses.
- `jj bookmark set main -r @-` then `jj git push` — bookmarks are jj's
  branches; they do **not** move automatically. Move `main` explicitly before
  pushing.
- `main@origin` is tracked. Remote: `github.com/DanielCarmingham/gitwho`.
- Don't mix `git commit` into a jj workflow here; use `jj` and let colocation
  export to git.

**`git ls-files` lies here.** Git's index reflects the last *exported* commit,
so a file untracked in the working-copy change still appears in it until that
change is committed and exported. Use `jj file list` when the question is "what
does this repo actually track?".

### Task tracking: `dex`

Multi-step work is tracked in dex, **by default, without being asked**. Use the
`dex` / `dex-plan` skills. The store resolves from cwd — this repo has its own
`.dex/` (confirm with `dex dir`; outside a git repo dex silently falls back to
the shared global store). `.dex/` is gitignored: it is local working state, not
a project artifact.

Create the task list up front and keep it current as work lands — including
marking a task active when work on it *starts*, not only when it finishes.

## Domain model

Two independent axes. Keying either on filesystem path is the bug this project
exists to fix.

| Axis | Decides | Keyed on |
|---|---|---|
| **git identity** | author name/email, ssh key | remote URL, via `includeIf "hasconfig:remote.*.url:"` |
| **CLI / MCP credentials** | what `gh`/`tea`/`glab`/MCP authenticate as | the repository, re-resolved per invocation by `exec` |

Providers differ in **mechanism**, not just variable name:

- An https remote authenticates git through the credential helper, which needs
  a token.
- An ssh remote authenticates with a key via `core.sshcommand`, and **no token
  is involved in push/pull at all**. An ssh account must not be forced to
  invent a token (R7).
- The same account can do both, over two hostnames — which is why transport is
  a property of a *remote*, not of an account, and why there is no `gitAuth`
  field.
- Each CLI reads its own variables: `gh`→`GH_TOKEN`, `tea`→`GITEA_TOKEN` +
  `GITEA_HOST`, `glab`→`GITLAB_TOKEN`, `az devops`→its own.

The declaration schema is in
[docs/accounts.toml.example](docs/accounts.toml.example). `env` and
`gitCredential` name variables; values live in the store.

## Verified environment

Verified 2026-08-09 on the development machine. Re-measure rather than trusting
these:

- macOS (darwin 25.6.0), zsh
- git 2.50.1 (Apple Git-155) — `hasconfig:remote.*.url:` needs ≥ 2.36 ✅
- direnv 2.37.1, gh 2.97.0, jj 0.44.0
- `tea` and `az` installed; `glab` not installed

## Releasing

Four channels, three of them automated off one tag push. `release.yml` is
**generated by `dist`** — never hand-edit it; change `dist-workspace.toml` and
re-run `dist generate`. `ci.yml` is hand-written and is not generated.

1. Bump `version` in `Cargo.toml`, then commit — `cargo publish --dry-run`
   refuses a dirty tree.
2. Verify, in this order:
   ```sh
   cargo test
   cargo clippy --all-targets -- -D warnings
   cargo fmt --check
   dist plan                 # four targets, shell installer, homebrew formula
   dist generate --check     # fails if release.yml has drifted from the config
   dist build --artifacts=local --target=aarch64-apple-darwin
   cargo publish --dry-run
   ```

   That fourth line is not optional, and 0.1.0 is why. `dist plan` never
   compiles, and `--artifacts=global` builds only the installer, formula and
   checksums — so a missing `[profile.dist]` in `Cargo.toml` passed every local
   check and then failed every build job in CI. A local-artifact build is the
   first thing that actually invokes the profile.
3. `cargo publish` **first**. It is the irreversible channel — a version on
   crates.io cannot be replaced — and dist has no crates.io job.
4. Tag `vX.Y.Z` on **exactly** the commit `cargo publish` ran from, then push
   the branch and the tag. The tag push is what triggers the release.
5. `gh run watch` — roughly ten minutes for four targets.

Two prerequisites live outside this repo, and both fail only at the *last* job,
after the builds have already run:

- the `DanielCarmingham/homebrew-tap` repository must exist;
- this repository needs a `HOMEBREW_TAP_TOKEN` secret — a PAT with write access
  to that tap. That exact name is what dist's generated job reads.

Choices worth not re-litigating:

- **`install-path = "CARGO_HOME"`** so the shell installer, `cargo install` and
  `cargo binstall` all land in `~/.cargo/bin`. Two routes disagreeing about
  where the binary went is a support question we can simply not have.
- **gnu, not musl.** No `build.rs`, no C dependencies. Add
  `x86_64-unknown-linux-musl` if a `GLIBC_2.xx not found` report appears.
- **A personal tap, not homebrew-core**, which has stars/forks thresholds this
  is nowhere near.
- **`[package.metadata.binstall]` in `Cargo.toml` is load-bearing.** Without
  it `cargo binstall` silently falls back to a full source build, because its
  default filename patterns embed the version and dist's archives do not.
- **`docs/accounts.toml.example` must stay out of `exclude`.** `init::TEMPLATE`
  is an `include_str!` of it, so excluding it breaks the build.

## Checks before calling anything done

```sh
cargo test                              # 161 pass, 1 ignored
cargo clippy --all-targets -- -D warnings
gitwho doctor                           # read-only; exits non-zero on problems
```

`doctor` is the integration check. It never prints a secret value, so its
output is safe to paste.
