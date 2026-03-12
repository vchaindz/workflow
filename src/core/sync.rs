use std::path::Path;
use std::process::Command;

use crate::error::{DzError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    NotInitialized,
    Clean,
    Dirty(usize),
    Ahead(usize),
    Behind(usize),
    Diverged(usize, usize),
    NoRemote,
    Offline,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PullResult {
    UpToDate,
    Updated(usize),
    Conflict(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct SyncInfo {
    pub status: SyncStatus,
    pub branch: String,
    pub remote_url: Option<String>,
    pub last_sync: Option<String>,
    pub changed_files: Vec<String>,
}

/// Check if `git` is on PATH.
pub fn detect_git() -> bool {
    which("git")
}

/// Check if `gh` (GitHub CLI) is on PATH.
pub fn detect_gh() -> bool {
    which("gh")
}

fn which(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if a directory is a git repository.
pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Initialize a git repo in `dir` with .gitignore and initial commit.
pub fn init_repo(dir: &Path) -> Result<()> {
    run_git(dir, &["init"])?;
    create_gitignore(dir)?;
    run_git(dir, &["add", "-A"])?;
    run_git(dir, &["commit", "-m", "Initial workflow sync"])?;
    Ok(())
}

/// Create a .gitignore with sensible defaults for the workflows directory.
pub fn create_gitignore(dir: &Path) -> Result<()> {
    let content = "logs/\nhistory.db\nhistory.db-journal\n*.log\nconfig.local.toml\n.DS_Store\n";
    std::fs::write(dir.join(".gitignore"), content)?;
    Ok(())
}

/// Add a remote origin URL.
pub fn setup_remote(dir: &Path, url: &str) -> Result<()> {
    // Check if remote already exists
    let out = run_git(dir, &["remote"])?;
    if out.trim().lines().any(|l| l.trim() == "origin") {
        run_git(dir, &["remote", "set-url", "origin", url])?;
    } else {
        run_git(dir, &["remote", "add", "origin", url])?;
    }
    Ok(())
}

/// Use `gh` to create a private repo and push. Returns the repo URL.
pub fn create_private_repo(dir: &Path) -> Result<String> {
    let name = "workflow-app-sync-repo";
    let output = Command::new("gh")
        .args(["repo", "create", name, "--private", "--source=.", "--push",
               "--description", "Workflow task definitions synced via workflow CLI"])
        .current_dir(dir)
        .output()
        .map_err(|e| DzError::Sync(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DzError::Sync(format!("gh repo create failed: {stderr}")));
    }

    // Extract URL from gh output or construct it
    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = stdout
        .lines()
        .find(|l| l.contains("github.com"))
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| format!("https://github.com/{name}"));

    Ok(url)
}

/// Get current sync status information.
pub fn get_status(dir: &Path) -> Result<SyncInfo> {
    if !is_repo(dir) {
        return Ok(SyncInfo {
            status: SyncStatus::NotInitialized,
            branch: String::new(),
            remote_url: None,
            last_sync: None,
            changed_files: Vec::new(),
        });
    }

    let branch = run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_default()
        .trim()
        .to_string();

    let remote_url = run_git(dir, &["remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let porcelain = run_git(dir, &["status", "--porcelain"]).unwrap_or_default();
    let changed_files: Vec<String> = porcelain
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    let last_sync = run_git(dir, &["log", "-1", "--format=%cr", "origin/HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let status = if remote_url.is_none() {
        if changed_files.is_empty() {
            SyncStatus::NoRemote
        } else {
            SyncStatus::Dirty(changed_files.len())
        }
    } else {
        // Fetch to update tracking refs (silently ignore failures = offline)
        let fetch_ok = run_git(dir, &["fetch", "--quiet"]).is_ok();

        if !fetch_ok {
            SyncStatus::Offline
        } else {
            let ahead = run_git(
                dir,
                &["rev-list", "--count", &format!("origin/{branch}..HEAD")],
            )
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);

            let behind = run_git(
                dir,
                &["rev-list", "--count", &format!("HEAD..origin/{branch}")],
            )
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);

            if !changed_files.is_empty() {
                SyncStatus::Dirty(changed_files.len())
            } else if ahead > 0 && behind > 0 {
                SyncStatus::Diverged(ahead, behind)
            } else if ahead > 0 {
                SyncStatus::Ahead(ahead)
            } else if behind > 0 {
                SyncStatus::Behind(behind)
            } else {
                SyncStatus::Clean
            }
        }
    };

    Ok(SyncInfo {
        status,
        branch,
        remote_url,
        last_sync,
        changed_files,
    })
}

/// Get the git diff output.
pub fn get_diff(dir: &Path) -> Result<String> {
    run_git(dir, &["diff"])
}

/// Auto-commit all changes with a smart commit message. Returns None if clean.
pub fn auto_commit(dir: &Path) -> Result<Option<String>> {
    if !is_repo(dir) {
        return Ok(None);
    }

    run_git(dir, &["add", "-A"])?;

    // Check if there's anything to commit
    let status = run_git(dir, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(None);
    }

    let msg = generate_commit_msg(dir)?;
    run_git(dir, &["commit", "-m", &msg])?;
    Ok(Some(msg))
}

/// Generate a smart commit message from staged changes.
fn generate_commit_msg(dir: &Path) -> Result<String> {
    let diff = run_git(dir, &["diff", "--name-status", "--cached"])?;
    let changes: Vec<(&str, &str)> = diff
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let status = parts.next()?.trim();
            let file = parts.next()?.trim();
            Some((status, file))
        })
        .collect();

    if changes.is_empty() {
        return Ok("Sync workflow changes".to_string());
    }

    if changes.len() == 1 {
        let (status, file) = changes[0];
        let action = match status {
            "A" => "Add",
            "D" => "Delete",
            _ => "Update",
        };
        // Strip extension for nicer message
        let name = file.strip_suffix(".yaml")
            .or_else(|| file.strip_suffix(".yml"))
            .or_else(|| file.strip_suffix(".sh"))
            .unwrap_or(file);
        return Ok(format!("{action} {name} task"));
    }

    if changes.len() <= 3 {
        let names: Vec<&str> = changes
            .iter()
            .map(|(_, f)| {
                f.strip_suffix(".yaml")
                    .or_else(|| f.strip_suffix(".yml"))
                    .or_else(|| f.strip_suffix(".sh"))
                    .unwrap_or(f)
            })
            .collect();
        return Ok(format!("Update {}", names.join(", ")));
    }

    Ok(format!("Sync {} workflow changes", changes.len()))
}

/// Push to remote.
pub fn push(dir: &Path, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["push", "-u", "origin", branch])
        .current_dir(dir)
        .output()
        .map_err(|e| DzError::Sync(format!("failed to run git push: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Could not resolve host") || stderr.contains("unable to access") {
            return Err(DzError::Sync(
                "Cannot reach remote — are you offline? Local changes are safe.".to_string(),
            ));
        }
        return Err(DzError::Sync(format!("git push failed: {stderr}")));
    }
    Ok(())
}

/// Pull from remote. Returns the result.
pub fn pull(dir: &Path, branch: &str) -> Result<PullResult> {
    let output = Command::new("git")
        .args(["pull", "origin", branch])
        .current_dir(dir)
        .output()
        .map_err(|e| DzError::Sync(format!("failed to run git pull: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if stdout.contains("Already up to date") {
        return Ok(PullResult::UpToDate);
    }

    if !output.status.success() {
        if stderr.contains("Could not resolve host") || stderr.contains("unable to access") {
            return Err(DzError::Sync(
                "Cannot reach remote — are you offline?".to_string(),
            ));
        }

        // Check for merge conflicts
        if stderr.contains("CONFLICT") || stderr.contains("Merge conflict") {
            let conflicts: Vec<String> = stderr
                .lines()
                .filter(|l| l.contains("CONFLICT"))
                .map(|l| l.to_string())
                .collect();
            return Ok(PullResult::Conflict(conflicts));
        }

        return Err(DzError::Sync(format!("git pull failed: {stderr}")));
    }

    // Count files changed
    let count = stdout
        .lines()
        .filter(|l| {
            l.contains("file changed")
                || l.contains("files changed")
                || l.contains("insertion")
                || l.contains("deletion")
        })
        .count()
        .max(1);

    Ok(PullResult::Updated(count))
}

/// Clone a repo to target path.
pub fn clone_repo(url: &str, target: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["clone", url])
        .arg(target)
        .output()
        .map_err(|e| DzError::Sync(format!("failed to run git clone: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DzError::Sync(format!("git clone failed: {stderr}")));
    }
    Ok(())
}

/// Run a git command and return stdout.
fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| DzError::Sync(format!("failed to run git {}: {e}", args.join(" "))))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DzError::Sync(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_git() {
        // git should be available in CI and dev environments
        assert!(detect_git());
    }

    #[test]
    fn test_is_repo_false() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_repo(tmp.path()));
    }

    #[test]
    fn test_is_repo_true() {
        let tmp = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(is_repo(tmp.path()));
    }

    #[test]
    fn test_create_gitignore() {
        let tmp = TempDir::new().unwrap();
        create_gitignore(tmp.path()).unwrap();
        let content = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(content.contains("logs/"));
        assert!(content.contains("history.db"));
        assert!(content.contains("config.local.toml"));
    }

    #[test]
    fn test_init_repo() {
        let tmp = TempDir::new().unwrap();
        // Need git user config for commit
        std::fs::write(
            tmp.path().join(".gitconfig"),
            "",
        )
        .ok();

        // Set local git config
        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();

        // Now test our gitignore creation and commit
        create_gitignore(tmp.path()).unwrap();
        run_git(tmp.path(), &["add", "-A"]).unwrap();
        run_git(tmp.path(), &["commit", "-m", "test"]).unwrap();

        assert!(is_repo(tmp.path()));
        assert!(tmp.path().join(".gitignore").exists());
    }

    #[test]
    fn test_get_status_not_initialized() {
        let tmp = TempDir::new().unwrap();
        let info = get_status(tmp.path()).unwrap();
        assert_eq!(info.status, SyncStatus::NotInitialized);
    }

    #[test]
    fn test_get_status_no_remote() {
        let tmp = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::fs::write(tmp.path().join("test.yaml"), "name: test\nsteps:\n  - id: s1\n    cmd: echo hi\n").unwrap();
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();

        let info = get_status(tmp.path()).unwrap();
        assert_eq!(info.status, SyncStatus::NoRemote);
        assert!(!info.branch.is_empty());
    }

    #[test]
    fn test_generate_commit_msg_single() {
        let tmp = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();

        // Create and stage a file
        std::fs::create_dir_all(tmp.path().join("backup")).unwrap();
        std::fs::write(tmp.path().join("backup/db-full.yaml"), "test").unwrap();
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(tmp.path())
            .output()
            .unwrap();

        let msg = generate_commit_msg(tmp.path()).unwrap();
        assert!(msg.contains("Add") && msg.contains("backup/db-full"));
    }
}
