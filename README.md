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

macOS and Linux, on x86-64 and arm64. Every route puts a `gitwho` binary in
`~/.cargo/bin`, so whichever you pick, that directory needs to be on your
`PATH`. You also need **git ≥ 2.36** — `includeIf "hasconfig:remote.*.url:"`
landed there.

**Homebrew**

```sh
brew install DanielCarmingham/tap/gitwho
```

**One line, no package manager:**

```sh
curl -LsSf https://github.com/DanielCarmingham/gitwho/releases/latest/download/gitwho-installer.sh | sh
```

**Cargo**, if you already have a Rust toolchain:

```sh
cargo install gitwho              # compiles it, a couple of minutes
cargo binstall gitwho             # or grab the same prebuilt binary, seconds
```

**From source**, which is what you want if you are going to change it:

```sh
git clone https://github.com/DanielCarmingham/gitwho
cd gitwho
cargo install --path . --locked
```

The first three need no Rust toolchain at all. Whichever you use, **pick the
location once and leave it alone**: `init`, `sync` and `shim install` bake the
binary's absolute path into what they generate, so moving it later breaks that
config silently.

## Quick start

```sh
gitwho init             # dry run: lists every step, writes nothing
gitwho init --write     # creates the store, scaffolds accounts.toml, stops
```

It stops there on purpose, because the next part is the one thing it cannot do
for you: put your accounts in `~/.config/gitwho/accounts.toml`. Then

```sh
gitwho secret set Work GH_TOKEN    # once per token; the value never enters argv
gitwho init --write                # finishes, and ends by running doctor
```

`init` is idempotent — re-run it whenever you add an account. Every step
reports `ok` when there was nothing to do, so a second run tells you exactly
what changed.

**[docs/INSTALL.md](docs/INSTALL.md)** covers what `init` does, how to do each
piece by hand, the checks that prove it works, and the two test commands that
produce false passes.

## Commands

```
gitwho init          set everything up; safe to re-run
gitwho doctor        report whether the wiring is coherent (read-only)
gitwho sync          regenerate the identity and credential rules
gitwho credential    git credential helper
gitwho exec -- cmd   run a command with exactly one account's credentials
gitwho shim install  wrapper scripts for gh / tea
gitwho secret …      store tokens; only fingerprints are ever printed
gitwho mcp sync      route provider MCP servers through exec
```

`doctor` is the one to run after any change. It is read-only, exits non-zero on
problems, and never prints a secret value.

## Status

In use on the author's machine since 2026-08-10: git identity, git credentials,
the `gh`/`tea` shims and a wrapped MCP server all route through it, and direnv
no longer exports a token per directory. 139 tests, clippy clean.

One gap remains there, and `doctor` reports it rather than hiding it: a shell
rc file still exports `GITEA_TOKEN`, so interactive shells carry a copy that
nothing reads. That `[ambient]` warning *is* the exposure this tool exists to
remove — it is left visible on purpose.

**Platform reality**, stated precisely because the gap between these rows is
easy to paper over:

| | |
|---|---|
| macOS | verified — this is where it runs every day |
| Linux | the test suite passes on x86-64 in CI. On Debian/aarch64 the whole `init` flow has also been driven by hand: store modes, identity, the credential helper, a shim executed for real, and the `.bashrc` PATH line |
| Windows | the `%APPDATA%` path, the `.cmd` shim and the `PATHEXT` lookup are unit-tested as pure functions **from macOS, and have never run on Windows**. Do not treat them as working |

## Reading

- **[docs/INSTALL.md](docs/INSTALL.md)** — setting it up, step by step, with a
  check after each one and the traps that produce false passes.
- **[docs/DESIGN.md](docs/DESIGN.md)** — why it is shaped this way: the
  measured evidence, the resolution algorithm, the threat model it does and
  does not cover, and the R1–R15 principles the source refers to by number.
- **[docs/accounts.toml.example](docs/accounts.toml.example)** — a commented
  starting config.
