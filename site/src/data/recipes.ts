/**
 * Four complete, paste-ready `accounts.toml` configurations.
 *
 * Every key used here must exist in `../docs/accounts.toml.example` — that is
 * what `tests/recipes.test.ts` checks, and why. This file and that file live
 * in the same repo now, so a schema change that isn't reflected here fails the
 * site build instead of shipping a recipe nobody can paste in.
 */
export interface Recipe {
  id: string;
  title: string;
  /** What situation this solves, in plain terms — read before the config. */
  problem: string;
  /** A complete accounts.toml, ready to paste into ~/.config/gitwho/accounts.toml. */
  toml: string;
  /** The exact `gitwho secret set` commands this config's tokens need. */
  secrets: string[];
}

export const RECIPES: Recipe[] = [
  {
    id: 'two-github',
    title: 'Two GitHub accounts: personal and work',
    problem:
      'A personal GitHub login and a separate work login on the same machine need different ' +
      'commit identities and different tokens, chosen by which repository you are in rather than ' +
      'by whichever account you happened to log into last.',
    toml: `[defaults]
account = "Personal"
gitName = "Your Name"

[[accounts]]
name = "Personal"
provider = "github"
email = "you@example.com"
gitCredential = "GH_TOKEN"
sshKey = "~/.ssh/id_ed25519_personal"
match = ["github.com/your-personal-username/**"]
env = ["GH_TOKEN"]
paths = ["~/src/personal/"]

[[accounts]]
name = "Work"
provider = "github"
email = "you@acme.example.com"
gitName = "Your Name (Acme)"
gitCredential = "GH_TOKEN"
sshKey = "~/.ssh/id_ed25519_work"
match = ["github.com/your-work-username/**"]
env = ["GH_TOKEN"]
paths = ["~/src/work/"]
`,
    secrets: ['gitwho secret set Personal GH_TOKEN', 'gitwho secret set Work GH_TOKEN'],
  },
  {
    id: 'github-org',
    title: 'One GitHub host, two organisations',
    problem:
      'A personal account plus a work organisation, both on github.com. This is the case ' +
      '`includeIf "gitdir:"` loses most often: both remotes are github.com, so only the path ' +
      'distinguishes them — and path-based rules break exactly when a repo moves, since nothing ' +
      'ties the rule to the remote it was written for. `match` runs against `host/path`, so ' +
      'listing the organisations is enough; no directory layout is implied or required.',
    toml: `[defaults]
account = "Personal"
gitName = "Your Name"

[[accounts]]
name = "Personal"
provider = "github"
email = "you@example.com"
gitCredential = "GH_TOKEN"
sshKey = "~/.ssh/id_ed25519_personal"
match = ["github.com/your-personal-username/**"]
env = ["GH_TOKEN"]
paths = ["~/src/personal/"]

[[accounts]]
name = "Work"
provider = "github"
email = "you@acme.example.com"
gitName = "Your Name (Acme)"
gitCredential = "GH_TOKEN"
match = [
    "github.com/acme-corp/**",
    "github.com/acme-labs/**",
]
env = ["GH_TOKEN"]
paths = ["~/src/work/"]
`,
    secrets: ['gitwho secret set Personal GH_TOKEN', 'gitwho secret set Work GH_TOKEN'],
  },
  {
    id: 'github-and-gitea',
    title: 'GitHub plus a self-hosted Gitea or Forgejo',
    problem:
      'One account over https, one over ssh. An https remote authenticates through the ' +
      'credential helper, which needs a token — but an ssh remote authenticates with a key and ' +
      'involves no token at all for push or pull. The self-hosted account below must not be made ' +
      'to invent one: it carries `sshKey` and nothing else, and that is a complete, working ' +
      'account, not an unfinished one.',
    toml: `[defaults]
account = "Personal"
gitName = "Your Name"

[[accounts]]
name = "Personal"
provider = "github"
email = "you@example.com"
gitCredential = "GH_TOKEN"
match = ["github.com/your-personal-username/**"]
env = ["GH_TOKEN"]
paths = ["~/src/github/"]

[[accounts]]
name = "SelfHosted"
provider = "gitea"
email = "you@example.net"
sshKey = "~/.ssh/id_ed25519_selfhosted"
match = ["ssh.git.example.net/**"]
paths = ["~/src/selfhosted/"]
`,
    secrets: ['gitwho secret set Personal GH_TOKEN'],
  },
  {
    id: 'adding-a-third',
    title: 'Adding a third account',
    problem:
      'Starting from the two-account config above, here is what changes to add a third: one new ' +
      '`[[accounts]]` block, nothing else touched. Run `gitwho init --write` again afterwards — ' +
      'it is idempotent and safe to re-run, and reports `ok` for every step that already matches, ' +
      'so a second run tells you exactly what changed.',
    toml: `[defaults]
account = "Personal"
gitName = "Your Name"

[[accounts]]
name = "Personal"
provider = "github"
email = "you@example.com"
gitCredential = "GH_TOKEN"
match = ["github.com/your-personal-username/**"]
env = ["GH_TOKEN"]
paths = ["~/src/personal/"]

[[accounts]]
name = "Work"
provider = "github"
email = "you@acme.example.com"
gitName = "Your Name (Acme)"
gitCredential = "GH_TOKEN"
match = ["github.com/acme-corp/**"]
env = ["GH_TOKEN"]
paths = ["~/src/work/"]

# --- New: a third account, self-hosted, ssh only -----------------------------
[[accounts]]
name = "SelfHosted"
provider = "gitea"
email = "you@example.net"
sshKey = "~/.ssh/id_ed25519_selfhosted"
match = ["ssh.git.example.net/**"]
paths = ["~/src/selfhosted/"]
`,
    secrets: ['gitwho secret set Personal GH_TOKEN', 'gitwho secret set Work GH_TOKEN'],
  },
];
