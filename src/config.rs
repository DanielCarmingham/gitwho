//! Parsing of `accounts.toml` -- the single place an account is declared.
//!
//! Meant to be committed to a dotfiles repo, so it names variables and never
//! holds their values (R10).

use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("accounts.toml is not valid: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("cannot read {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub defaults: Defaults,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// The account used when nothing else matches. Declared explicitly so the
    /// fallback is stated rather than emergent (R4).
    pub account: String,
    /// The author name for every account that does not override it. Kept here
    /// because it is the same person throughout; only the address differs.
    #[serde(rename = "gitName", default)]
    pub git_name: Option<String>,
    /// Which secret store holds the values on this machine: `age` or
    /// `keychain`. Unset means the built-in default.
    ///
    /// Kept as a plain string. Which names are legal is
    /// `secrets::select`'s business -- a parser that knew them would reject a
    /// typo with a message about TOML rather than about backends, and would
    /// have to be edited to add one.
    #[serde(rename = "secretBackend", default)]
    pub secret_backend: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub name: String,
    pub provider: String,
    pub email: String,
    /// Which declared variable holds the token git should authenticate with.
    ///
    /// Named explicitly rather than inferred from the provider: an account may
    /// hold several credentials, and guessing which one is the git password is
    /// the kind of implicit behaviour that goes wrong quietly.
    ///
    /// Absent when the account never uses https, which is how R7 is honoured:
    /// a key-authenticated account is not made to invent a token. Transport is
    /// deliberately NOT declared per account -- git chooses it per remote, and
    /// a credential helper is only ever consulted for https, so an account
    /// that uses both -- ssh to one hostname and the API over https on another,
    /// which is the ordinary shape of a self-hosted Gitea or Forgejo -- needs
    /// no special case.
    #[serde(rename = "gitCredential", default)]
    pub git_credential: Option<String>,
    /// The ssh key for remotes that use it, written into `core.sshcommand`.
    ///
    /// Independent of `gitCredential`: an account may have both, either, or
    /// neither.
    #[serde(rename = "sshKey", default)]
    pub ssh_key: Option<String>,
    /// Author name, when this account differs from `defaults.gitName`.
    #[serde(rename = "gitName", default)]
    pub git_name: Option<String>,
    /// Glob patterns matched against `host/path` of a remote URL.
    #[serde(rename = "match", default)]
    pub match_patterns: Vec<String>,
    /// Variables this account's CLIs and MCP servers need. A bare `VAR` names
    /// a secret to fetch; `VAR=value` is a literal, for non-secret settings
    /// such as an API host; a table names a value to read from another tool
    /// that already holds it.
    #[serde(default)]
    pub env: Vec<EnvSpec>,
    /// Directory prefixes claimed by this account, consulted **only** for a
    /// repo that has no remote yet. Everything else resolves by URL, so a
    /// relocated clone is unaffected by these.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// A variable whose value is read from another tool rather than from gitwho's
/// own store.
///
/// The point is that there is no second copy. A token copied out of `gh` goes
/// stale the moment the original is rotated, and a stale credential is present,
/// decryptable and wrong -- the failure R8 exists to refuse. A reference cannot
/// drift, because there is only ever one value.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcedVar {
    /// The variable this populates.
    pub var: String,
    /// Which tool holds the value.
    ///
    /// Kept a plain string for the same reason as `secretBackend`: which names
    /// are legal is the source registry's business, and a parser that knew them
    /// would reject a typo with a message about TOML rather than about sources.
    pub from: String,
    /// The account to ask that tool for. Absent means "whichever it considers
    /// active", which is only ever right on a single-account machine.
    #[serde(default)]
    pub user: Option<String>,
    /// The host to ask about, for tools that serve more than one.
    #[serde(default)]
    pub host: Option<String>,
}

/// One entry in an account's `env` list.
///
/// Three forms, because they are three genuinely different things: a name to
/// look up, a literal that is not a secret at all, and a pointer at a value
/// somebody else is already maintaining.
#[derive(Debug, Clone)]
pub enum EnvSpec {
    /// `"VAR"` -- fetch from the secret store. `"VAR=value"` -- a literal.
    Simple(String),
    /// `{ var = "GH_TOKEN", from = "gh", user = "octocat" }`.
    Sourced(SourcedVar),
}

/// Dispatches on the shape of the value rather than deriving `untagged`.
///
/// `untagged` reports a mistyped field as "data did not match any variant of
/// untagged enum EnvSpec", which names neither the field nor the fix. Choosing
/// the variant from the TOML type first means a table is *committed* to being a
/// `SourcedVar`, so `deny_unknown_fields` gets to say `unknown field 'form',
/// expected one of 'var', 'from', 'user', 'host'` -- which is the whole
/// difference between a config typo you can see and one you cannot.
impl<'de> Deserialize<'de> for EnvSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SpecVisitor;

        impl<'de> serde::de::Visitor<'de> for SpecVisitor {
            type Value = EnvSpec;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(
                    r#"a variable name like "GH_TOKEN", a literal like "GITEA_HOST=https://...", or a table like { var = "GH_TOKEN", from = "gh", user = "octocat" }"#,
                )
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<EnvSpec, E> {
                Ok(EnvSpec::Simple(value.to_string()))
            }

            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<EnvSpec, E> {
                Ok(EnvSpec::Simple(value))
            }

            fn visit_map<A>(self, map: A) -> Result<EnvSpec, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                SourcedVar::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(EnvSpec::Sourced)
            }
        }

        deserializer.deserialize_any(SpecVisitor)
    }
}

impl EnvSpec {
    /// The variable name this entry sets.
    pub fn name(&self) -> &str {
        match self {
            EnvSpec::Simple(spec) => spec.split_once('=').map_or(spec.as_str(), |(name, _)| name),
            EnvSpec::Sourced(sourced) => &sourced.var,
        }
    }

    /// The value written literally in the config, for `VAR=value` entries.
    ///
    /// Only ever non-secret settings such as an API host -- a secret written
    /// here would be a secret in `accounts.toml`, which R10 forbids.
    pub fn literal(&self) -> Option<&str> {
        match self {
            EnvSpec::Simple(spec) => spec.split_once('=').map(|(_, value)| value),
            EnvSpec::Sourced(_) => None,
        }
    }

    /// Where the value comes from, when it is not gitwho's own store.
    pub fn sourced(&self) -> Option<&SourcedVar> {
        match self {
            EnvSpec::Sourced(sourced) => Some(sourced),
            EnvSpec::Simple(_) => None,
        }
    }
}

impl Account {
    /// Where `var` is read from, when the account declares somewhere other than
    /// the store.
    ///
    /// Looked up by name rather than carried on the declaration, because
    /// `gitCredential` names a variable too: a referenced token has to work as
    /// git's password, not only as an environment variable.
    pub fn source_for(&self, var: &str) -> Option<&SourcedVar> {
        self.env
            .iter()
            .filter_map(EnvSpec::sourced)
            .find(|sourced| sourced.var == var)
    }

    /// Every variable this account reads from another tool.
    pub fn sourced_vars(&self) -> Vec<&SourcedVar> {
        self.env.iter().filter_map(EnvSpec::sourced).collect()
    }

    /// The variables this account needs a stored value for.
    ///
    /// Literal `VAR=value` entries are excluded -- they carry their own value
    /// and are not secrets. Sourced entries are excluded too: their value lives
    /// in another tool, so asking the store for one and finding nothing is the
    /// expected state, not a fault to report. The git credential variable is
    /// included even if it is not repeated in `env`, since it still needs a
    /// value to exist. Deduplicated, because declaring it in both places is
    /// natural.
    pub fn secret_vars(&self) -> Vec<&str> {
        let mut vars: Vec<&str> = Vec::new();

        for spec in &self.env {
            if spec.literal().is_some() || spec.sourced().is_some() {
                continue;
            }
            let name = spec.name();
            if !vars.contains(&name) {
                vars.push(name);
            }
        }

        if let Some(var) = &self.git_credential {
            if self.source_for(var).is_none() && !vars.contains(&var.as_str()) {
                vars.push(var);
            }
        }

        vars
    }
}

impl Config {
    pub fn parse(toml_str: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(toml_str)?)
    }

    pub fn account(&self, name: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.name == name)
    }

    pub fn load(path: &std::path::Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text)
    }
}
