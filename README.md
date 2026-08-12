# gitwho

Pick the right git identity and credentials for a repository, automatically,
wherever that repository lives on disk.

## The problem

Working across several accounts — personal, two employers, a self-hosted Gitea —
means every repo needs a different author identity *and* different credentials
for whatever tooling touches it (`gh`, `tea`, an IDE, an MCP server). Getting it
wrong is quiet: you commit as the wrong person, or authenticate as the wrong
account, and nothing tells you.

The usual approaches key off **filesystem path** — `includeIf "gitdir:"` rules
plus per-directory environment loading. That breaks as soon as a repo isn't
where the rule expects: a clone made somewhere else, or a worktree placed in a
tool's own root. It breaks *silently*, falling back to whichever account is the
default.

## The approach

Identity should follow the **repository**, not its location:

- **git identity** resolves from the remote URL (`includeIf
  "hasconfig:remote.*.url:"`), so it is correct in a worktree, a relocated
  clone, or a temp directory.
- **git credentials** come from a credential helper, which git hands the URL it
  is about to contact — so the right account is chosen even for a clone of a
  repository that does not exist on disk yet.
- **CLI credentials** are injected per invocation, into that process only.
  `gh` and `tea` run through shims that resolve from the current repository and
  scrub every other account's variables first, so nothing sits in the ambient
  environment for an unrelated process to pick up.

One file declares your accounts. Nothing else needs editing when you add one.

```toml
[[accounts]]
name          = "Work"
email         = "you@example-corp.com"
gitCredential = "GH_TOKEN"          # names the variable; the value is in the store
match         = ["github.com/example-corp/**"]
env           = ["GH_TOKEN"]        # what `exec` injects
```

`match` runs against `host/path`, which is why several GitHub accounts on one
host can be told apart — by organisation, with no directory layout implied.

## Install

Needs a Rust toolchain and git ≥ 2.36 (for `includeIf
"hasconfig:remote.*.url:"`).

```sh
git clone https://github.com/DanielCarmingham/gitwho
cd gitwho
cargo install --path . --locked
```

The binary alone does nothing — the account rules, the secret store and the git
wiring are all setup. **[docs/INSTALL.md](docs/INSTALL.md)** walks through it
with a verification at each step, plus what a second machine can and cannot
restore from your dotfiles.

## Commands

```
gitwho doctor        report whether the wiring is coherent (read-only)
gitwho sync          generate the identity rules
gitwho credential    git credential helper
gitwho exec -- cmd   run a command with exactly one account's credentials
gitwho shim install  wrapper scripts for gh / tea
gitwho secret …      store tokens; only fingerprints are ever printed
gitwho mcp sync      route provider MCP servers through exec
```

`doctor` is the one to run first and after any change. It is read-only, exits
non-zero on problems, and never prints a secret value.

## Status

In use on the author's machine since 2026-08-10: git identity, git credentials,
the `gh`/`tea` shims and a wrapped MCP server all route through it, and direnv
no longer exports a token per directory. 118 tests, clippy clean.

One gap remains there, and `doctor` reports it rather than hiding it: a shell
rc file still exports `GITEA_TOKEN`, so interactive shells carry a copy that
nothing reads. That `[ambient]` warning *is* the exposure this tool exists to
remove — it is left visible on purpose.

**Platform reality**, stated precisely because the gap between these rows is
easy to paper over:

| | |
|---|---|
| macOS | verified — everything below has actually been run |
| Linux | plausible, untried. The default secret store is an age-encrypted file, not the Keychain, and the shims are plain `#!/bin/sh` |
| Windows | the `%APPDATA%` path, the `.cmd` shim and the `PATHEXT` lookup are unit-tested as pure functions **from macOS, and have never run on Windows**. Do not treat them as working |

## Reading

- **[docs/INSTALL.md](docs/INSTALL.md)** — setting it up, step by step, with a
  check after each one and the traps that produce false passes.
- **[docs/DESIGN.md](docs/DESIGN.md)** — why it is shaped this way: the
  measured evidence, the resolution algorithm, the threat model it does and
  does not cover, and the R1–R15 principles the source refers to by number.
- **[docs/accounts.toml.example](docs/accounts.toml.example)** — a commented
  starting config.
