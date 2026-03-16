#![cfg(feature = "mcp")]

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Returns the command string to spawn the echo MCP server.
fn echo_server_cmd() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("bash {}/tests/fixtures/echo_mcp_server.sh", manifest_dir)
}

#[test]
fn test_mcp_list_tools_echo_server() {
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["mcp", "list-tools", &echo_server_cmd()])
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"))
        .stdout(predicate::str::contains("1 tool(s) available"));
}

#[test]
fn test_mcp_list_tools_json() {
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["mcp", "list-tools", &echo_server_cmd(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("echo"));
}

#[test]
fn test_mcp_call_echo_server() {
    Command::cargo_bin("workflow")
        .unwrap()
        .args([
            "mcp",
            "call",
            &echo_server_cmd(),
            "echo",
            "--arg",
            "message=hello",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("echo: hello"));
}

#[test]
fn test_mcp_check_echo_server() {
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["mcp", "check", &echo_server_cmd()])
        .assert()
        .success()
        .stdout(predicate::str::contains("healthy"))
        .stdout(predicate::str::contains("echo"));
}

#[test]
fn test_mcp_workflow_execution() {
    let dir = TempDir::new().unwrap();
    let server_cmd = echo_server_cmd();

    std::fs::create_dir_all(dir.path().join("test")).unwrap();
    std::fs::write(
        dir.path().join("test/mcp-echo.yaml"),
        format!(
            r#"name: MCP Echo Test
steps:
  - id: echo_step
    mcp:
      server:
        command: "{server_cmd}"
      tool: echo
      args:
        message: "hello world"
"#
        ),
    )
    .unwrap();

    Command::cargo_bin("workflow")
        .unwrap()
        .args([
            "--dir",
            dir.path().to_str().unwrap(),
            "run",
            "test/mcp-echo",
        ])
        .assert()
        .success();
}
