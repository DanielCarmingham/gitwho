//! Rewriting `.mcp.json` so provider MCP servers launch through `exec`.
//!
//! An MCP server is a long-lived process started by an editor or agent, and it
//! inherits that launcher's environment wholesale. `Digilope/one-drop-visuals`
//! runs `gitea-mcp` with no `env` block at all today, which means it holds
//! whatever credential happened to be ambient when Claude Code started.
//!
//! Wrapping the command rather than writing an `env` block is deliberate:
//! these files are usually committed, and a token in one would be a secret in
//! version control (R10).

use serde_json::{Map, Value};

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Command fragments that identify a server as talking to a git provider.
///
/// Matched against the command and its arguments, so both a packaged binary
/// (`gitea-mcp`) and an npx invocation (`@modelcontextprotocol/server-github`)
/// are caught. Deliberately a small explicit list: wrapping something that
/// needs no credential is harmless but confusing, and silently missing one is
/// the failure that matters, so `doctor` reports what is still unwrapped.
const PROVIDER_MARKERS: &[&str] = &[
    "gitea-mcp",
    "forgejo-mcp",
    "github-mcp",
    "gitlab-mcp",
    "server-github",
    "server-gitlab",
    "server-gitea",
];

fn is_provider_server(server: &Map<String, Value>) -> bool {
    let mut haystack = String::new();
    if let Some(Value::String(command)) = server.get("command") {
        haystack.push_str(command);
    }
    if let Some(Value::Array(args)) = server.get("args") {
        for arg in args {
            if let Value::String(arg) = arg {
                haystack.push(' ');
                haystack.push_str(arg);
            }
        }
    }

    PROVIDER_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
}

/// Already launching through gitfriend?
fn is_wrapped(server: &Map<String, Value>) -> bool {
    let Some(Value::Array(args)) = server.get("args") else {
        return false;
    };
    // The command path varies by install location, so the reliable signal is
    // the argument shape gitfriend itself writes.
    matches!(args.first(), Some(Value::String(first)) if first == "exec")
}

/// Rewrite provider servers to launch through `gitfriend exec`.
///
/// Returns the new document and the names of the servers changed. Servers that
/// touch no provider, and servers already wrapped, are left exactly as they
/// were.
pub fn wrap(json: &str, gitfriend_path: &str) -> Result<(String, Vec<String>), McpError> {
    let mut doc: Value = serde_json::from_str(json)?;
    let mut changed = Vec::new();

    if let Some(Value::Object(servers)) = doc.get_mut("mcpServers") {
        for (name, server) in servers.iter_mut() {
            let Value::Object(server) = server else {
                continue;
            };
            if !is_provider_server(server) || is_wrapped(server) {
                continue;
            }

            let Some(Value::String(original_command)) = server.get("command").cloned() else {
                continue;
            };
            let original_args = match server.get("args") {
                Some(Value::Array(args)) => args.clone(),
                _ => Vec::new(),
            };

            let mut new_args = vec![
                Value::String("exec".to_string()),
                Value::String("--".to_string()),
                Value::String(original_command),
            ];
            new_args.extend(original_args);

            server.insert(
                "command".to_string(),
                Value::String(gitfriend_path.to_string()),
            );
            server.insert("args".to_string(), Value::Array(new_args));
            changed.push(name.clone());
        }
    }

    Ok((serde_json::to_string_pretty(&doc)?, changed))
}

/// Provider servers still launching without `exec` -- the ones inheriting
/// whatever credential is ambient. Read-only; for `doctor`.
pub fn unwrapped_servers(json: &str) -> Result<Vec<String>, McpError> {
    let doc: Value = serde_json::from_str(json)?;
    let mut found = Vec::new();

    if let Some(Value::Object(servers)) = doc.get("mcpServers") {
        for (name, server) in servers {
            let Value::Object(server) = server else {
                continue;
            };
            if is_provider_server(server) && !is_wrapped(server) {
                found.push(name.clone());
            }
        }
    }

    Ok(found)
}
