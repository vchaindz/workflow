use crate::cli::args::SyncAction;
use crate::core::config::Config;
use crate::core::sync;
use crate::error::{DzError, Result};

pub fn cmd_sync(config: &mut Config, action: SyncAction) -> Result<i32> {
    let dir = &config.workflows_dir;

    match action {
        SyncAction::Init => {
            if sync::is_repo(dir) {
                println!("Already a git repository: {}", dir.display());
                return Ok(0);
            }

            if !sync::detect_git() {
                return Err(DzError::Sync("git not found on PATH".to_string()));
            }

            sync::init_repo(dir)?;
            println!("Initialized git repo in {}", dir.display());

            if sync::detect_gh() {
                println!("Tip: run `workflow sync setup` to create a GitHub repo and enable auto-sync");
            }

            Ok(0)
        }

        SyncAction::Clone { url } => {
            if !sync::detect_git() {
                return Err(DzError::Sync("git not found on PATH".to_string()));
            }

            // Backup existing workflows_dir if it has content
            if dir.exists() && std::fs::read_dir(dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
                let bak = dir.with_extension("bak");
                println!("Backing up existing workflows to {}", bak.display());
                if bak.exists() {
                    std::fs::remove_dir_all(&bak)?;
                }
                std::fs::rename(dir, &bak)?;
            }

            sync::clone_repo(&url, dir)?;
            println!("Cloned workflows from {url}");

            // Update config with remote URL
            config.sync.enabled = true;
            config.sync.remote_url = Some(url);
            config.save_sync_config()?;
            println!("Sync enabled. Ready to use.");

            Ok(0)
        }

        SyncAction::Push { message } => {
            if !sync::is_repo(dir) {
                return Err(DzError::Sync(
                    "Not a git repo. Run `workflow sync init` first.".to_string(),
                ));
            }

            if let Some(msg) = &message {
                // Manual commit with custom message
                sync::auto_commit(dir)?;
                // If auto_commit made a commit, amend with custom msg. Otherwise commit with msg.
                let status = std::process::Command::new("git")
                    .args(["status", "--porcelain"])
                    .current_dir(dir)
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();

                if status.is_empty() {
                    // auto_commit already committed, amend message
                    let _ = std::process::Command::new("git")
                        .args(["commit", "--amend", "-m", msg])
                        .current_dir(dir)
                        .output();
                }
            } else {
                match sync::auto_commit(dir)? {
                    Some(msg) => println!("Committed: {msg}"),
                    None => println!("Nothing to commit — working tree clean"),
                }
            }

            let branch = &config.sync.branch;
            sync::push(dir, branch)?;
            println!("Pushed to origin/{branch}");

            Ok(0)
        }

        SyncAction::Pull => {
            if !sync::is_repo(dir) {
                return Err(DzError::Sync(
                    "Not a git repo. Run `workflow sync init` first.".to_string(),
                ));
            }

            let branch = &config.sync.branch;
            match sync::pull(dir, branch)? {
                sync::PullResult::UpToDate => println!("Already up to date."),
                sync::PullResult::Updated(n) => println!("Updated ({n} change(s) pulled)."),
                sync::PullResult::Conflict(files) => {
                    println!("Merge conflicts detected! Resolve manually:");
                    for f in &files {
                        println!("  {f}");
                    }
                    return Ok(1);
                }
            }

            Ok(0)
        }

        SyncAction::Status => {
            let info = sync::get_status(dir)?;

            match &info.status {
                sync::SyncStatus::NotInitialized => {
                    println!("Not initialized — run `workflow sync init` to start");
                    return Ok(0);
                }
                sync::SyncStatus::Clean => println!("Status: clean"),
                sync::SyncStatus::Dirty(n) => println!("Status: {n} uncommitted change(s)"),
                sync::SyncStatus::Ahead(n) => println!("Status: {n} commit(s) ahead of remote"),
                sync::SyncStatus::Behind(n) => println!("Status: {n} commit(s) behind remote"),
                sync::SyncStatus::Diverged(a, b) => {
                    println!("Status: diverged ({a} ahead, {b} behind)")
                }
                sync::SyncStatus::NoRemote => println!("Status: no remote configured"),
                sync::SyncStatus::Offline => println!("Status: offline (cannot reach remote)"),
            }

            if !info.branch.is_empty() {
                println!("Branch: {}", info.branch);
            }
            if let Some(url) = &info.remote_url {
                println!("Remote: {url}");
            }
            if let Some(sync_time) = &info.last_sync {
                println!("Last sync: {sync_time}");
            }
            if !info.changed_files.is_empty() {
                println!("Changed files:");
                for f in &info.changed_files {
                    println!("  {f}");
                }
            }

            println!("Auto-sync: {}", if config.sync.enabled { "enabled" } else { "disabled" });

            Ok(0)
        }

        SyncAction::Setup => {
            if !sync::detect_git() {
                return Err(DzError::Sync("git not found on PATH. Install git first.".to_string()));
            }

            // Step 1: Init repo if needed
            if !sync::is_repo(dir) {
                println!("Initializing git repo in {}...", dir.display());
                sync::init_repo(dir)?;
                println!("Done.");
            } else {
                println!("Git repo already initialized.");
            }

            // Step 2: Check for gh and offer to create repo
            if sync::detect_gh() {
                println!("\nGitHub CLI (gh) detected.");
                println!("Creating private repo 'workflow-app-sync-repo'...");
                match sync::create_private_repo(dir) {
                    Ok(url) => {
                        println!("Created: {url}");
                        config.sync.remote_url = Some(url);
                    }
                    Err(e) => {
                        println!("Could not create repo: {e}");
                        println!("You can add a remote manually with: workflow sync push");
                        println!("  git -C {} remote add origin <url>", dir.display());
                    }
                }
            } else {
                println!("\nGitHub CLI (gh) not found.");
                println!("To connect a remote, run:");
                println!("  git -C {} remote add origin <your-repo-url>", dir.display());
            }

            // Step 3: Enable sync in config
            config.sync.enabled = true;
            config.sync.auto_commit = true;
            config.sync.auto_push = true;
            config.sync.auto_pull_on_start = true;
            config.save_sync_config()?;
            println!("\nSync enabled in config.toml.");
            println!("Workflows will auto-commit and push on changes.");

            Ok(0)
        }
    }
}
