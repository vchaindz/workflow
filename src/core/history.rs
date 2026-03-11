use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub command: String,
    pub timestamp: Option<i64>,
}

/// Detect the shell history file: $HISTFILE first, then probe
/// ~/.zsh_history and ~/.bash_history.
fn history_file_path() -> Option<PathBuf> {
    if let Ok(hf) = std::env::var("HISTFILE") {
        let p = PathBuf::from(&hf);
        if p.is_file() {
            return Some(p);
        }
    }

    if let Some(home) = dirs::home_dir() {
        for name in &[".zsh_history", ".bash_history"] {
            let p = home.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    None
}

/// Load shell history, returning newest-first, deduped, noise-filtered.
/// Reads at most `max_entries` lines from the end of the file.
pub fn load_shell_history(max_entries: usize) -> Vec<HistoryEntry> {
    let path = match history_file_path() {
        Some(p) => p,
        None => return Vec::new(),
    };

    let content = match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => return Vec::new(),
    };

    let is_zsh = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.contains("zsh"))
        .unwrap_or(false);

    let raw = if is_zsh {
        parse_zsh_history(&content)
    } else {
        parse_bash_history(&content)
    };

    // Take the tail (most recent), then dedup + filter
    let start = raw.len().saturating_sub(max_entries);
    let tail = &raw[start..];

    // Dedup keeping most recent (last occurrence)
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut deduped: Vec<HistoryEntry> = Vec::new();
    for entry in tail.iter().rev() {
        let key = entry.command.trim().to_string();
        if key.is_empty() {
            continue;
        }
        if seen.contains_key(&key) {
            continue;
        }
        seen.insert(key, deduped.len());
        deduped.push(entry.clone());
    }

    // Filter noise
    deduped.retain(|e| !is_noise(&e.command));

    // Already newest-first from the rev() iteration
    deduped
}

/// Parse zsh extended history format: `: 1773148418:0;command`
/// Handles multi-line continuations (trailing `\`).
fn parse_zsh_history(content: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    let mut current_cmd: Option<String> = None;
    let mut current_ts: Option<i64> = None;

    for line in content.lines() {
        // Check if we're continuing a multi-line command
        if let Some(ref mut cmd) = current_cmd {
            if cmd.ends_with('\\') {
                cmd.pop(); // remove trailing backslash
                cmd.push('\n');
                cmd.push_str(line);
                continue;
            } else {
                // Previous command is complete, push it
                entries.push(HistoryEntry {
                    command: cmd.clone(),
                    timestamp: current_ts,
                });
                current_cmd = None;
                current_ts = None;
            }
        }

        // Try to parse zsh extended format: `: timestamp:0;command`
        if let Some(rest) = line.strip_prefix(": ") {
            if let Some(semi_pos) = rest.find(';') {
                let meta = &rest[..semi_pos];
                let cmd = rest[semi_pos + 1..].to_string();
                let ts = meta
                    .split(':')
                    .next()
                    .and_then(|s| s.trim().parse::<i64>().ok());
                current_ts = ts;
                current_cmd = Some(cmd);
                continue;
            }
        }

        // Plain line (some zsh configs don't use extended format)
        if !line.is_empty() {
            current_cmd = Some(line.to_string());
            current_ts = None;
        }
    }

    // Flush last command
    if let Some(cmd) = current_cmd {
        if !cmd.ends_with('\\') {
            entries.push(HistoryEntry {
                command: cmd,
                timestamp: current_ts,
            });
        }
    }

    entries
}

/// Parse bash plain history (one command per line, no timestamps).
/// Handles multi-line continuations (trailing `\`).
fn parse_bash_history(content: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    let mut current_cmd: Option<String> = None;

    for line in content.lines() {
        if let Some(ref mut cmd) = current_cmd {
            if cmd.ends_with('\\') {
                cmd.pop();
                cmd.push('\n');
                cmd.push_str(line);
                continue;
            } else {
                entries.push(HistoryEntry {
                    command: cmd.clone(),
                    timestamp: None,
                });
                current_cmd = None;
            }
        }

        if !line.is_empty() {
            current_cmd = Some(line.to_string());
        }
    }

    if let Some(cmd) = current_cmd {
        if !cmd.ends_with('\\') {
            entries.push(HistoryEntry {
                command: cmd,
                timestamp: None,
            });
        }
    }

    entries
}

/// Filter out noisy / trivial commands.
fn is_noise(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    if trimmed.len() < 3 {
        return true;
    }
    let first_word = trimmed.split_whitespace().next().unwrap_or("");
    matches!(
        first_word,
        "cd" | "ls" | "clear" | "pwd" | "exit" | "fg" | "bg" | "history" | "which" | "echo"
    )
}

/// Suggest a category name based on keywords found in the selected commands.
pub fn suggest_category(commands: &[&str]) -> String {
    let mut scores: HashMap<&str, usize> = HashMap::new();

    let rules: &[(&[&str], &str)] = &[
        (&["docker", "podman", "docker-compose"], "docker"),
        (&["git", "gh"], "git"),
        (&["kubectl", "helm", "k9s"], "kubernetes"),
        (&["systemctl", "journalctl", "service"], "system"),
        (&["pg_dump", "psql", "mysql", "mysqldump", "sqlite3"], "database"),
        (&["rsync", "tar", "backup", "borg", "restic"], "backup"),
        (&["npm", "cargo", "make", "cmake", "gradle", "mvn", "yarn", "pnpm"], "build"),
        (&["ssh", "scp", "sftp", "rsync"], "remote"),
        (&["apt", "dnf", "pacman", "yum", "brew", "snap"], "packages"),
        (&["ansible", "terraform", "vagrant", "packer"], "infra"),
    ];

    for cmd in commands {
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let first = words.first().copied().unwrap_or("");
        // Also check the basename if it's a path
        let basename = first.rsplit('/').next().unwrap_or(first);

        for (keywords, category) in rules {
            if keywords.iter().any(|kw| basename == *kw || first == *kw) {
                *scores.entry(category).or_insert(0) += 1;
            }
        }
    }

    scores
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(cat, _)| cat.to_string())
        .unwrap_or_else(|| "tasks".to_string())
}

/// Derive a task name from the first command.
/// Extracts the first word, sanitizes to `[a-z0-9-]`, truncates to 30 chars.
pub fn derive_task_name(cmd: &str) -> String {
    let first_word = cmd
        .trim()
        .split_whitespace()
        .next()
        .unwrap_or("task")
        .rsplit('/')
        .next()
        .unwrap_or("task");

    let sanitized: String = first_word
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    // Remove leading/trailing dashes and collapse consecutive dashes
    let collapsed: String = sanitized
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    let name = if collapsed.is_empty() {
        "task".to_string()
    } else {
        collapsed
    };

    if name.len() > 30 {
        name[..30].to_string()
    } else {
        name
    }
}

/// Format a timestamp as relative time ("2h ago", "3d ago", etc.)
pub fn format_relative_time(ts: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - ts;

    if diff < 0 {
        return "future".to_string();
    }

    let minutes = diff / 60;
    let hours = diff / 3600;
    let days = diff / 86400;
    let weeks = diff / 604800;

    if minutes < 1 {
        "just now".to_string()
    } else if minutes < 60 {
        format!("{}m ago", minutes)
    } else if hours < 24 {
        format!("{}h ago", hours)
    } else if days < 7 {
        format!("{}d ago", days)
    } else if weeks < 52 {
        format!("{}w ago", weeks)
    } else {
        format!("{}y ago", days / 365)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_zsh_extended_format() {
        let content = ": 1773148418:0;cargo build --release\n\
                        : 1773148500:0;git status\n\
                        : 1773148600:0;docker compose up -d\n";
        let entries = parse_zsh_history(content);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "cargo build --release");
        assert_eq!(entries[0].timestamp, Some(1773148418));
        assert_eq!(entries[1].command, "git status");
        assert_eq!(entries[2].command, "docker compose up -d");
    }

    #[test]
    fn test_parse_zsh_multiline() {
        let content = ": 1773148418:0;docker run \\\n\
                        --name test \\\n\
                        -d nginx\n\
                        : 1773148500:0;echo done\n";
        let entries = parse_zsh_history(content);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].command.contains("docker run"));
        assert!(entries[0].command.contains("--name test"));
        assert!(entries[0].command.contains("-d nginx"));
    }

    #[test]
    fn test_parse_bash_format() {
        let content = "cargo build\ngit push\ndocker ps\n";
        let entries = parse_bash_history(content);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "cargo build");
        assert_eq!(entries[1].command, "git push");
        assert_eq!(entries[2].command, "docker ps");
        assert!(entries[0].timestamp.is_none());
    }

    #[test]
    fn test_bash_multiline() {
        let content = "docker run \\\n--name test \\\n-d nginx\necho done\n";
        let entries = parse_bash_history(content);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].command.contains("docker run"));
        assert!(entries[0].command.contains("-d nginx"));
    }

    #[test]
    fn test_noise_filter() {
        assert!(is_noise("cd"));
        assert!(is_noise("ls"));
        assert!(is_noise("clear"));
        assert!(is_noise("pwd"));
        assert!(is_noise("exit"));
        assert!(is_noise("ab")); // too short
        assert!(!is_noise("cargo build"));
        assert!(!is_noise("docker ps"));
    }

    #[test]
    fn test_suggest_category_docker() {
        let cmds = vec!["docker compose up -d", "docker ps", "docker logs app"];
        assert_eq!(suggest_category(&cmds), "docker");
    }

    #[test]
    fn test_suggest_category_git() {
        let cmds = vec!["git status", "git add .", "git commit -m 'fix'"];
        assert_eq!(suggest_category(&cmds), "git");
    }

    #[test]
    fn test_suggest_category_mixed() {
        let cmds = vec!["pg_dump mydb > dump.sql", "rsync -av dump.sql remote:/backups/"];
        let cat = suggest_category(&cmds);
        // Both database and backup/remote have 1 match each; any is valid
        assert!(["database", "backup", "remote"].contains(&cat.as_str()));
    }

    #[test]
    fn test_suggest_category_fallback() {
        let cmds = vec!["some-custom-tool --flag", "another-thing"];
        assert_eq!(suggest_category(&cmds), "tasks");
    }

    #[test]
    fn test_derive_task_name_simple() {
        assert_eq!(derive_task_name("cargo build --release"), "cargo");
        assert_eq!(derive_task_name("docker compose up -d"), "docker");
        assert_eq!(derive_task_name("/usr/bin/pg_dump mydb"), "pg-dump");
    }

    #[test]
    fn test_derive_task_name_sanitize() {
        assert_eq!(derive_task_name("my_script.sh --verbose"), "my-script-sh");
    }

    #[test]
    fn test_derive_task_name_truncate() {
        let long = "a]".repeat(20) + " arg";
        let name = derive_task_name(&long);
        assert!(name.len() <= 30);
    }

    #[test]
    fn test_derive_task_name_empty() {
        assert_eq!(derive_task_name(""), "task");
        assert_eq!(derive_task_name("   "), "task");
    }

    #[test]
    fn test_format_relative_time() {
        let now = chrono::Utc::now().timestamp();
        assert_eq!(format_relative_time(now), "just now");
        assert_eq!(format_relative_time(now - 120), "2m ago");
        assert_eq!(format_relative_time(now - 7200), "2h ago");
        assert_eq!(format_relative_time(now - 86400 * 3), "3d ago");
        assert_eq!(format_relative_time(now - 604800 * 2), "2w ago");
    }
}
