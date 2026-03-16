use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn setup_fixtures() -> TempDir {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    std::fs::create_dir_all(root.join("backup")).unwrap();
    std::fs::create_dir_all(root.join("deploy")).unwrap();

    std::fs::write(
        root.join("backup/db-full.sh"),
        "#!/bin/bash\necho 'backup done'",
    )
    .unwrap();

    std::fs::write(
        root.join("backup/mysql-daily.yaml"),
        r#"name: MySQL Daily
steps:
  - id: dump
    cmd: echo "dumping"
  - id: compress
    cmd: echo "compressing"
    needs: [dump]
"#,
    )
    .unwrap();

    std::fs::write(
        root.join("deploy/staging.yaml"),
        r#"name: Deploy Staging
steps:
  - id: build
    cmd: echo "building"
  - id: deploy
    cmd: echo "deploying"
    needs: [build]
"#,
    )
    .unwrap();

    dir
}

#[test]
fn test_list_command() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap(), "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backup"))
        .stdout(predicate::str::contains("db-full"))
        .stdout(predicate::str::contains("deploy"));
}

#[test]
fn test_list_json() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap(), "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""));
}

#[test]
fn test_run_shell_script() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap(), "run", "backup/db-full"])
        .assert()
        .success()
        .stderr(predicate::str::contains("success"));
}

#[test]
fn test_run_yaml_workflow() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args([
            "--dir",
            dir.path().to_str().unwrap(),
            "run",
            "backup/mysql-daily",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("success"));
}

#[test]
fn test_run_dry_run() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args([
            "--dir",
            dir.path().to_str().unwrap(),
            "run",
            "deploy/staging",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("[dry-run]"));
}

#[test]
fn test_run_dot_notation() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args([
            "--dir",
            dir.path().to_str().unwrap(),
            "run",
            "backup.db-full",
        ])
        .assert()
        .success();
}

#[test]
fn test_run_nonexistent_task() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args([
            "--dir",
            dir.path().to_str().unwrap(),
            "run",
            "nonexistent/task",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_status_no_history() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args([
            "--dir",
            dir.path().to_str().unwrap(),
            "status",
            "backup/db-full",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("No run history"));
}

#[test]
fn test_status_after_run() {
    let dir = setup_fixtures();
    let dir_str = dir.path().to_str().unwrap();

    // Run first
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir_str, "run", "backup/db-full"])
        .assert()
        .success();

    // Check status
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir_str, "status", "backup/db-full"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Last success"));
}

#[test]
fn test_logs_empty() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args([
            "--dir",
            dir.path().to_str().unwrap(),
            "logs",
            "backup/db-full",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("No logs found"));
}

#[test]
fn test_version_flag() {
    Command::cargo_bin("workflow")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("workflow"));
}

#[test]
fn test_snapshot_set_get_delete_roundtrip() {
    let dir = setup_fixtures();
    // set
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap()])
        .args(["snapshot", "set", "test/task", "baseline", "--value", r#"{"status":"200"}"#])
        .assert()
        .success()
        .stderr(predicate::str::contains("Snapshot stored"));
    // get
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap()])
        .args(["snapshot", "get", "test/task", "baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"{"status":"200"}"#));
    // list
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap()])
        .args(["snapshot", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test/task"));
    // delete
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap()])
        .args(["snapshot", "delete", "test/task", "baseline"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Snapshot deleted"));
    // get after delete → exit 1
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap()])
        .args(["snapshot", "get", "test/task", "baseline"])
        .assert()
        .code(1);
}

#[test]
fn test_snapshot_list_json() {
    let dir = setup_fixtures();
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap()])
        .args(["snapshot", "set", "a/b", "key1", "--value", "val1"])
        .assert()
        .success();
    Command::cargo_bin("workflow")
        .unwrap()
        .args(["--dir", dir.path().to_str().unwrap()])
        .args(["snapshot", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""task_ref": "a/b""#));
}
