# gitfriend

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
- **credentials** are injected into the environment, re-resolved on every
  directory change rather than only at directory-tree boundaries — so anything
  launched from that shell inherits the right ones, not just wrapped commands.

See [REQUIREMENTS.md](REQUIREMENTS.md) for the full requirements, the evidence
behind them (all measured, not assumed), and the open questions.

## Status

**Built and tested; not yet adopted.** The machine still runs the old
path-based dotfiles setup — nothing in `$HOME` has been changed.

```
gitfriend doctor        report whether the wiring is coherent (read-only)
gitfriend sync          generate the identity rules
gitfriend credential    git credential helper
gitfriend exec -- cmd   run a command with exactly one account's credentials
gitfriend shim install  wrapper scripts for gh / tea
gitfriend secret …      store tokens; only fingerprints are ever printed
gitfriend mcp sync      route provider MCP servers through exec
```

**Picking this up?** Read [docs/CUTOVER.md](docs/CUTOVER.md) — the remaining
work is switching this machine over, step by step, with a check and an undo
for each.

Design and the measurements behind it:
[docs/superpowers/specs/2026-08-09-gitfriend-design.md](docs/superpowers/specs/2026-08-09-gitfriend-design.md).
Requirements: [REQUIREMENTS.md](REQUIREMENTS.md) — note its claim that Digilope
needs no token in the push/pull path is wrong; that host serves https too.
