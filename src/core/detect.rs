/// Heuristic detection of interactive or streaming commands that need
/// inherited stdio instead of captured pipes.
///
/// Returns `true` if the command likely requires user interaction (REPLs, TUI tools)
/// or streams indefinitely (log tailing, watch).
pub fn is_interactive_command(cmd: &str) -> bool {
    // Normalize: strip shell quotes and check all parts of compound commands
    // (&&, ||, ;, |). We scan the whole string so `cd /app && docker logs -f` is caught.
    let stripped = strip_shell_quotes(cmd);
    let lower = stripped.to_lowercase();

    // --- Streaming flags ---
    // journalctl -f, docker logs -f, tail -f, kubectl logs -f, --follow
    if has_streaming_flag(&lower) {
        return true;
    }

    // --- Known streaming tools ---
    if contains_command(&lower, "docker events")
        || contains_command(&lower, "wrangler tail")
        || starts_with_command(&lower, "watch ")
    {
        return true;
    }

    // --- REPLs / shells ---
    for repl in &["psql", "mysql", "redis-cli", "mongo", "mongosh", "sqlite3"] {
        if is_bare_command(&lower, repl) {
            return true;
        }
    }

    // python/node without -c or script argument → REPL mode
    if is_repl_invocation(&lower, "python") || is_repl_invocation(&lower, "python3") {
        return true;
    }
    if is_repl_invocation(&lower, "node") {
        return true;
    }

    // --- TUI tools ---
    for tui_tool in &[
        "vim", "nvim", "nano", "less", "more", "htop", "top", "btop", "mc", "ncdu",
        "nmon", "glances", "iftop", "nethogs", "tmux", "screen",
    ] {
        if is_bare_command(&lower, tui_tool) {
            return true;
        }
    }

    // --- docker exec -it ---
    if lower.contains("docker exec") && has_docker_interactive_flags(&lower) {
        return true;
    }

    // --- ssh without remote command ---
    if is_interactive_ssh(&lower) {
        return true;
    }

    false
}

/// Check for streaming flags: -f, --follow (but not in unrelated contexts)
fn has_streaming_flag(cmd: &str) -> bool {
    // Match patterns like `journalctl -f`, `tail -f`, `docker logs -f`, `kubectl logs -f`
    let streaming_contexts = [
        "journalctl", "tail", "docker logs", "kubectl logs", "stern",
    ];

    for ctx in &streaming_contexts {
        if cmd.contains(ctx) {
            // Check for -f or --follow after the command
            if let Some(pos) = cmd.find(ctx) {
                let after = &cmd[pos + ctx.len()..];
                if has_flag(after, "-f") || has_flag(after, "--follow") {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if a flag appears as a standalone token in the string.
fn has_flag(s: &str, flag: &str) -> bool {
    for token in s.split_whitespace() {
        if token == flag {
            return true;
        }
        // Handle combined short flags like -xf
        if flag == "-f" && token.starts_with('-') && !token.starts_with("--") && token.contains('f')
        {
            return true;
        }
    }
    false
}

/// Check if a command name appears as a standalone command (not as a substring
/// of another word). Handles compound commands with &&, ||, ;, |.
fn is_bare_command(cmd: &str, name: &str) -> bool {
    for segment in split_compound(cmd) {
        let trimmed = segment.trim();
        let first_word = trimmed.split_whitespace().next().unwrap_or("");
        // Handle paths like /usr/bin/psql
        let basename = first_word.rsplit('/').next().unwrap_or(first_word);
        if basename == name {
            return true;
        }
    }
    false
}

/// Check if a command string contains a specific multi-word command.
fn contains_command(cmd: &str, pattern: &str) -> bool {
    cmd.contains(pattern)
}

/// Check if any segment starts with the given command prefix.
fn starts_with_command(cmd: &str, prefix: &str) -> bool {
    for segment in split_compound(cmd) {
        if segment.trim().starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Split a compound command on &&, ||, ;, | into segments.
fn split_compound(cmd: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let bytes = cmd.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'&' if i + 1 < len && bytes[i + 1] == b'&' => {
                segments.push(&cmd[start..i]);
                i += 2;
                start = i;
            }
            b'|' if i + 1 < len && bytes[i + 1] == b'|' => {
                segments.push(&cmd[start..i]);
                i += 2;
                start = i;
            }
            b';' | b'|' => {
                segments.push(&cmd[start..i]);
                i += 1;
                start = i;
            }
            _ => {
                i += 1;
            }
        }
    }
    segments.push(&cmd[start..]);
    segments
}

/// Detect python/node in REPL mode (no script file or -c argument).
fn is_repl_invocation(cmd: &str, interpreter: &str) -> bool {
    for segment in split_compound(cmd) {
        let trimmed = segment.trim();
        let first_word = trimmed.split_whitespace().next().unwrap_or("");
        let basename = first_word.rsplit('/').next().unwrap_or(first_word);

        if basename == interpreter {
            let rest = trimmed[first_word.len()..].trim();
            // No arguments → REPL
            if rest.is_empty() {
                return true;
            }
            // -c means inline execution, not REPL
            if rest.starts_with("-c") || rest.starts_with("-c ") {
                return false;
            }
            // -e means inline execution (node)
            if rest.starts_with("-e") || rest.starts_with("-e ") {
                return false;
            }
            // If first non-flag arg doesn't start with -, it's a script file
            let mut has_script = false;
            for token in rest.split_whitespace() {
                if token == "-c" || token == "-e" || token == "-m" {
                    return false;
                }
                if !token.starts_with('-') {
                    has_script = true;
                    break;
                }
            }
            if has_script {
                return false;
            }
            // Only flags (like python -i) → likely interactive
            return true;
        }
    }
    false
}

/// Check docker exec for -it, -ti, or separate -i -t flags.
fn has_docker_interactive_flags(cmd: &str) -> bool {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let mut has_i = false;
    let mut has_t = false;

    for token in &tokens {
        if *token == "-it" || *token == "-ti" {
            return true;
        }
        if token.starts_with('-') && !token.starts_with("--") {
            if token.contains('i') && token.contains('t') {
                return true;
            }
            if token.contains('i') {
                has_i = true;
            }
            if token.contains('t') {
                has_t = true;
            }
        }
    }
    has_i && has_t
}

/// Strip single and double quotes from a command string.
/// `ssh host 'journalctl -f'` → `ssh host journalctl -f`
fn strip_shell_quotes(cmd: &str) -> String {
    cmd.chars().filter(|&c| c != '\'' && c != '"').collect()
}

/// SSH is interactive if there's no remote command (no quoted string after host).
fn is_interactive_ssh(cmd: &str) -> bool {
    for segment in split_compound(cmd) {
        let trimmed = segment.trim();
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        let basename = tokens[0].rsplit('/').next().unwrap_or(tokens[0]);
        if basename != "ssh" {
            continue;
        }

        // Walk tokens after "ssh", skip flags and their arguments
        let mut i = 1;
        let flag_with_arg = [
            "-o", "-p", "-i", "-l", "-F", "-J", "-L", "-R", "-D", "-b", "-c", "-e", "-m",
            "-O", "-Q", "-S", "-W", "-w",
        ];
        let mut found_host = false;
        while i < tokens.len() {
            let tok = tokens[i];
            if flag_with_arg.contains(&tok) {
                i += 2; // skip flag + its argument
                continue;
            }
            if tok.starts_with('-') {
                i += 1;
                continue;
            }
            if !found_host {
                found_host = true;
                i += 1;
                continue;
            }
            // Found something after host → remote command → not interactive
            return false;
        }
        // If we found a host but nothing after → interactive SSH
        if found_host {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_commands() {
        assert!(is_interactive_command("journalctl -f"));
        assert!(is_interactive_command("journalctl -fu nginx"));
        assert!(is_interactive_command("tail -f /var/log/syslog"));
        assert!(is_interactive_command("docker logs -f mycontainer"));
        assert!(is_interactive_command("kubectl logs -f pod/nginx"));
        assert!(is_interactive_command("tail --follow /var/log/syslog"));
    }

    #[test]
    fn test_streaming_tools() {
        assert!(is_interactive_command("docker events"));
        assert!(is_interactive_command("wrangler tail"));
        assert!(is_interactive_command("watch ls -la"));
    }

    #[test]
    fn test_repls() {
        assert!(is_interactive_command("psql"));
        assert!(is_interactive_command("mysql"));
        assert!(is_interactive_command("redis-cli"));
        assert!(is_interactive_command("mongo"));
        assert!(is_interactive_command("sqlite3"));
        assert!(is_interactive_command("python"));
        assert!(is_interactive_command("python3"));
        assert!(is_interactive_command("node"));
    }

    #[test]
    fn test_repl_with_script_not_interactive() {
        assert!(!is_interactive_command("python script.py"));
        assert!(!is_interactive_command("python -c 'print(1)'"));
        assert!(!is_interactive_command("python3 -m pytest"));
        assert!(!is_interactive_command("node app.js"));
        assert!(!is_interactive_command("node -e 'console.log(1)'"));
    }

    #[test]
    fn test_tui_tools() {
        assert!(is_interactive_command("htop"));
        assert!(is_interactive_command("top"));
        assert!(is_interactive_command("btop"));
        assert!(is_interactive_command("vim file.txt"));
        assert!(is_interactive_command("nvim"));
        assert!(is_interactive_command("nano config.yaml"));
        assert!(is_interactive_command("less /var/log/syslog"));
        assert!(is_interactive_command("mc"));
        assert!(is_interactive_command("ncdu /var"));
    }

    #[test]
    fn test_docker_exec_interactive() {
        assert!(is_interactive_command("docker exec -it mycontainer bash"));
        assert!(is_interactive_command("docker exec -ti mycontainer sh"));
        assert!(!is_interactive_command("docker exec mycontainer ls"));
    }

    #[test]
    fn test_ssh_interactive() {
        assert!(is_interactive_command("ssh root@host"));
        assert!(is_interactive_command("ssh -p 2222 user@host"));
        assert!(!is_interactive_command("ssh root@host 'systemctl status foo'"));
        assert!(!is_interactive_command("ssh root@host ls -la"));
    }

    #[test]
    fn test_ssh_with_streaming_remote_command() {
        // SSH with a streaming remote command should be detected as interactive
        assert!(is_interactive_command("ssh root@host 'journalctl -u agentx-agent -f'"));
        assert!(is_interactive_command("ssh root@host journalctl -f"));
        assert!(is_interactive_command("ssh root@host 'tail -f /var/log/syslog'"));
        assert!(is_interactive_command("ssh user@host 'docker logs -f webapp'"));
        assert!(is_interactive_command("ssh root@host \"journalctl -f\""));
        assert!(is_interactive_command("ssh root@46.225.122.49 'journalctl -u agentx-agent -f'"));
        // Non-streaming SSH should remain non-interactive
        assert!(!is_interactive_command("ssh root@host 'journalctl -u nginx --no-pager'"));
        assert!(!is_interactive_command("ssh root@host 'systemctl status nginx'"));
    }

    #[test]
    fn test_compound_commands() {
        assert!(is_interactive_command("cd /app && docker logs -f web"));
        assert!(is_interactive_command("export FOO=bar && htop"));
        assert!(!is_interactive_command("echo hello && echo world"));
    }

    #[test]
    fn test_non_interactive() {
        assert!(!is_interactive_command("echo hello"));
        assert!(!is_interactive_command("ls -la"));
        assert!(!is_interactive_command("curl https://example.com"));
        assert!(!is_interactive_command("docker ps"));
        assert!(!is_interactive_command("journalctl -u nginx --no-pager"));
        assert!(!is_interactive_command("git status"));
        assert!(!is_interactive_command("cargo build"));
    }

    #[test]
    fn test_path_based_commands() {
        assert!(is_interactive_command("/usr/bin/psql"));
        assert!(is_interactive_command("/usr/bin/vim file.txt"));
    }
}
