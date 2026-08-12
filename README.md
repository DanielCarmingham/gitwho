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

See [REQUIREMENTS.md](REQUIREMENTS.md) for the full requirements, the evidence
behind them (all measured, not assumed), and the open questions.

```
gitwho doctor        report whether the wiring is coherent (read-only)
gitwho sync          generate the identity rules
gitwho credential    git credential helper
gitwho exec -- cmd   run a command with exactly one account's credentials
gitwho shim install  wrapper scripts for gh / tea
gitwho secret …      store tokens; only fingerprints are ever printed
gitwho mcp sync      route provider MCP servers through exec
```

## Install

There are no prebuilt binaries; build it. Needs a Rust toolchain and git ≥ 2.36
(for `includeIf "hasconfig:remote.*.url:"`).

```sh
cargo install --path . --locked
```

The binary alone does nothing — the account rules, the store and the git wiring
are all setup. **[docs/INSTALL.md](docs/INSTALL.md)** walks through it, with a
verification at each step, plus what a second machine can and cannot restore
from your dotfiles.

## Status

**In use** on the author's machine since 2026-08-10: git identity, git
credentials, the `gh`/`tea` shims and one wrapped MCP server all route through
it. direnv no longer exports a token per directory. 117 tests, clippy clean.

One gap remains there: `~/.zshrc.local` still exports the old `GH_TOKEN_*`
values, so interactive shells carry a copy nothing reads. `doctor` reports it
as an `[ambient]` warning.

Verified on macOS only. The Linux path is plausible and untried; the Windows
paths and `.cmd` shim are unit-tested from macOS and **have never run on
Windows** — see CLAUDE.md.

[docs/CUTOVER.md](docs/CUTOVER.md) is the migration runbook for that machine —
step by step, with a check and an undo for each. It is a record of dismantling
one particular setup, not an install guide.

Design and the measurements behind it:
[docs/superpowers/specs/2026-08-09-gitwho-design.md](docs/superpowers/specs/2026-08-09-gitwho-design.md).
Requirements: [REQUIREMENTS.md](REQUIREMENTS.md) — note its claim that Digilope
needs no token in the push/pull path is wrong; that host serves https too.
