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

Requirements and evidence gathered. Nothing built here yet — the current
working implementation still lives in dotfiles and is path-based.
