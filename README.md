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
for you: put your accounts in `~/.config/gitwho/accounts.toml`. To get a head
start, read the repositories you already have:

```sh
gitwho init --discover ~/src            # prints a proposal, writes nothing
```

It groups every `remote.origin.url` it finds by `host/org` and prints a config
you can pipe into a file. It **cannot** know which orgs are the same person, so
it says so rather than guessing — merging those blocks is the part left to you.
Then

```sh
gitwho secret set Work GH_TOKEN    # once per token; the value never enters argv
gitwho secret set --here          # or let the repository you are in name the account
gitwho init --write                # finishes, and ends by running doctor
```

If `gh` is already logged in as that account, you can skip storing a token
altogether — `env = [{ var = "GH_TOKEN", from = "gh", user = "..." }]` reads it
from `gh` on demand, so there is no copy to keep in sync. Declare every variable
that way and there is no secret store to set up at all. See
[Security](#security).

`init` is idempotent — re-run it whenever you add an account. Every step
reports `ok` when there was nothing to do, so a second run tells you exactly
what changed.

**[docs/INSTALL.md](docs/INSTALL.md)** covers what `init` does, how to do each
piece by hand, the checks that prove it works, and the two test commands that
produce false passes.

## Commands

```
gitwho init          set everything up; safe to re-run
gitwho init --discover <roots>   propose accounts.toml from repos on disk
gitwho doctor        report whether the wiring is coherent (read-only)
gitwho whoami        which account this repository resolves to, and why
gitwho renew         bring this repository's credentials up to date,
                     pulling from gh (or logging in) rather than asking you to
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
no longer exports a token per directory. 195 tests, clippy clean.

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

**What it does not cover yet.** `jj` takes its credentials from git, so pushing
works — but it keeps author identity in its own config and does not read
gitconfig's `includeIf` rules, so in a colocated repo `jj` and `git` can commit
as different people without saying so. Details, and what fixing it would take,
are in [docs/DESIGN.md](docs/DESIGN.md#known-limits).

## Security

The property that matters is not the encryption — it is that **a token is
fetched by the one process that needs it, at the moment it needs it**, instead
of sitting in your environment where everything you launch inherits it. `exec`
clears every managed variable before setting the resolved account's, so nothing
ambient survives into the child.

Around that: stored values live in an age-encrypted file unlocked by an identity
key, both created `0600` inside a `0700` directory — and created at that mode
rather than chmod'd afterwards, so there is no window where the file is complete
and readable. Values never enter `argv` (`secret set` reads stdin, because `ps`
is public), are never printed (only fingerprints), and never enter the
repository (`accounts.toml` names variables). `doctor` fails outright if those
permissions drift, and warns when a provider token is sitting in your
environment — the exposure this exists to remove.

A variable can also name a tool that already holds its value, in which case
gitwho stores nothing at all and reads it on demand:

```toml
env = [{ var = "GH_TOKEN", from = "gh", user = "octocat" }]
```

That is a pointer, not a copy, so it cannot go stale when you rotate the token —
and on macOS the value stays in `gh`'s keychain entry rather than gitwho's file.
It costs a process spawn (~60 ms against ~10 ms), which is why it is declared
per variable rather than switched on globally. If the tool cannot answer,
gitwho fails and says so; it never falls back to a stored copy.

**What it does not do:** the identity key sits next to the ciphertext, both
owned by you, so anything running as your user can decrypt everything. The
encryption defends the secrets at rest — a backup, a sync folder, an accidental
commit — not against local code.

Be concrete about that, because "encrypted" invites the wrong conclusion.
Anything running as you can pipe a request into `gitwho credential` and be
handed the token in plain text. gitwho is a decryption oracle for its own
store — it has to be, that is how it answers git. Getting a token out of it is
no harder than reading `GH_TOKEN` out of your `.bashrc`, or running
`gh auth token`. **Against local code running as you, this is not an
improvement on either, and does not claim to be.** What changes is exposure
over time: a token is present in one process for one invocation, instead of in
every process for the whole session.

The full threat model, including why the platform keychain is implemented but
not the default, is in
[docs/DESIGN.md](docs/DESIGN.md#what-this-protects-and-what-it-does-not).
To report a vulnerability, see [SECURITY.md](SECURITY.md) — please use private
reporting rather than a public issue.

## Reading

- **[docs/INSTALL.md](docs/INSTALL.md)** — setting it up, step by step, with a
  check after each one and the traps that produce false passes.
- **[docs/DESIGN.md](docs/DESIGN.md)** — why it is shaped this way: the
  measured evidence, the resolution algorithm, the threat model it does and
  does not cover, and the R1–R15 principles the source refers to by number.
- **[docs/accounts.toml.example](docs/accounts.toml.example)** — a commented
  starting config.
