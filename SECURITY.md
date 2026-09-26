# Security

gitwho holds provider tokens and decides which one to hand to which process, so
a bug here is a bug about credentials. This document says how to report one,
and — just as important — what the design does and does not claim, so you can
tell a vulnerability from a documented limit.

## Reporting a vulnerability

**Use [GitHub's private vulnerability
reporting](https://github.com/DanielCarmingham/gitwho/security/advisories/new).**
It is enabled on this repository. Please do not open a public issue for
anything that would expose or misdirect someone's credentials — a public issue
is a disclosure.

This is one person's project, maintained alongside other work. Expect an
acknowledgement within about a week rather than within hours. If a report is
valid I will fix it, credit you unless you would rather I did not, and say
plainly in the release notes what was wrong.

Include, if you can: the version (`gitwho --version`), the platform, and a
`gitwho doctor` transcript. `doctor` never prints a token value — only
fingerprints — so its output is safe to paste. **Never send a real token**, in
a report or anywhere else; a fingerprint or the first few characters is enough
to identify one.

## Supported versions

The latest release only. This is pre-1.0 software with a single maintainer;
fixes go into a new version rather than being backported.

## What counts as a vulnerability here

These are the properties gitwho is built to hold. A reproducible break in any
of them is a security bug, not a feature request:

- **A working-but-incorrect account.** Resolution returning an account that
  authenticates successfully but is the *wrong* one. This is the failure the
  whole project exists to prevent: it means committing or acting as someone you
  are not, with nothing to signal it. Refusing is correct; guessing is not.
- **A token crossing accounts.** Entering one account's repository handing a
  different account's token to any tool.
- **A token value leaving the process.** Appearing in output, an error message,
  a log, a generated file, or `argv` — anywhere `ps`, a shoulder, or a pasted
  transcript could pick it up. Only fingerprints are ever printed.
- **Config permissions widening.** `~/.config/gitwho` is `0700` and
  `accounts.toml` is `0600`. That is what keeps `accounts.toml` out of another
  local account's reach — a writable config is a redirect vector: whoever can
  write it can add a `match` pattern for a host they control and be handed a
  token.
- **A credential served for a host no account claims.** The helper declines
  when resolution is not confident; serving anyway would be a redirect.

## What is *not* a vulnerability

These are known, documented, and explained in
[docs/DESIGN.md](docs/DESIGN.md#what-this-protects-and-what-it-does-not).
Reports about them are welcome as ideas, but they are not undisclosed holes:

- **A local process running as you can read everything.** gitwho holds no
  secret of its own: a token lives in `gh`'s keychain entry, or in `tea`'s own
  `credentials.json.enc` with its key in the keychain — both owned by you, both
  readable by anything running as your user.

  It is worth being blunt about how low that bar is, because "gitwho decides
  who gets logged in" reads like a stronger claim than it is. Code running as
  you does not need to go through gitwho at all; it can just ask:

  ```sh
  gh auth token --hostname github.com --user octocat
  ```

  and get the same token back. This is no harder than running that command
  directly, and gitwho is **not** an improvement on it against local code.
  Reports demonstrating this are not vulnerabilities; it is the documented
  design.

  What actually improves is *exposure over time*: a token lives in one process
  for one invocation instead of in every process you launch for the whole
  session. That narrows the window and the blast radius. It does not stop
  anything running as you.
- **A token is in the environment of the process gitwho launches.** That is how
  `gh` and `tea` read it, so during `gitwho exec` the value is visible in that
  child's environment to your own user. The gain is scope, not absence: one
  process for one invocation, instead of every process for the whole session.
- **A dead token looks like a live one.** `doctor` reports that the CLI answers
  for an account, not that the provider still accepts the token it hands back.
- **Anything on Windows.** The Windows paths and shim are unit-tested as pure
  functions and have never been executed on Windows. Treat that platform as
  unverified rather than as broken or as working.

## If you think a token has been exposed

Rotate it at the provider first — that is the only step that actually revokes
anything — then log the CLI back in as that login (`gh auth login`, or
`tea login add --url <url>`). gitwho keeps no copy to invalidate: the next
`gitwho exec` reads whatever `gh`/`tea` now hold.
