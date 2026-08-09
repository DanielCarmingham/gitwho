use gitfriend::mcp;

/// The shape of the real file in `Digilope/one-drop-visuals`: a provider MCP
/// with no `env` block at all, so it inherits whatever the shell had.
const REAL_SHAPE: &str = r#"{
  "mcpServers": {
    "gitea": {
      "type": "stdio",
      "command": "/Users/daniel/Developer/claude/gitea-mcp_Darwin_arm64/gitea-mcp",
      "args": ["-d"]
    },
    "sequential-thinking": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-sequential-thinking"],
      "env": {}
    }
  }
}"#;

#[test]
fn wraps_a_provider_server_so_it_launches_through_exec() {
    let (rewritten, changed) = mcp::wrap(REAL_SHAPE, "/usr/local/bin/gitfriend").unwrap();

    assert_eq!(changed, vec!["gitea".to_string()]);

    let parsed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
    let gitea = &parsed["mcpServers"]["gitea"];
    assert_eq!(gitea["command"], "/usr/local/bin/gitfriend");
    assert_eq!(
        gitea["args"],
        serde_json::json!([
            "exec",
            "--",
            "/Users/daniel/Developer/claude/gitea-mcp_Darwin_arm64/gitea-mcp",
            "-d"
        ])
    );
}

#[test]
fn leaves_servers_that_touch_no_provider_alone() {
    let (rewritten, _) = mcp::wrap(REAL_SHAPE, "/usr/local/bin/gitfriend").unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
    let other = &parsed["mcpServers"]["sequential-thinking"];
    assert_eq!(other["command"], "npx");
    assert_eq!(
        other["args"],
        serde_json::json!(["-y", "@modelcontextprotocol/server-sequential-thinking"])
    );
}

#[test]
fn wrapping_twice_does_not_double_wrap() {
    // `mcp sync` will be run again whenever a server is added. A second pass
    // must be a no-op, or the command grows a new `gitfriend exec --` prefix
    // every time.
    let (once, _) = mcp::wrap(REAL_SHAPE, "/usr/local/bin/gitfriend").unwrap();
    let (twice, changed) = mcp::wrap(&once, "/usr/local/bin/gitfriend").unwrap();

    assert!(changed.is_empty(), "second pass reported changes: {changed:?}");
    assert_eq!(once, twice, "second pass altered the file");
}

#[test]
fn unrelated_keys_and_their_order_survive() {
    // These files are committed. Reordering keys would turn a one-line change
    // into a whole-file diff.
    let input = r#"{
  "mcpServers": {
    "zzz-last": { "command": "npx", "args": ["thing"] },
    "gitea": { "command": "gitea-mcp", "args": ["-d"] },
    "aaa-first": { "command": "npx", "args": ["other"] }
  }
}"#;

    let (rewritten, _) = mcp::wrap(input, "/usr/local/bin/gitfriend").unwrap();

    let order: Vec<&str> = rewritten
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            ["\"zzz-last\"", "\"gitea\"", "\"aaa-first\""]
                .iter()
                .find(|k| t.starts_with(*k))
                .copied()
        })
        .collect();
    assert_eq!(order, vec!["\"zzz-last\"", "\"gitea\"", "\"aaa-first\""]);
}

#[test]
fn a_server_with_no_provider_marker_is_not_touched() {
    let input = r#"{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/Users/daniel"]
    }
  }
}"#;

    let (_, changed) = mcp::wrap(input, "/usr/local/bin/gitfriend").unwrap();

    assert!(changed.is_empty(), "a non-provider server was wrapped: {changed:?}");
}

#[test]
fn reports_which_servers_still_run_unwrapped() {
    // What `doctor` needs: naming the exposure without changing anything.
    let unwrapped = mcp::unwrapped_servers(REAL_SHAPE).unwrap();

    assert_eq!(unwrapped, vec!["gitea".to_string()]);
}
