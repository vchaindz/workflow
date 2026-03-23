use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiTool {
    Claude,
    Codex,
    Gemini,
}

impl AiTool {
    pub fn name(&self) -> &'static str {
        match self {
            AiTool::Claude => "Claude",
            AiTool::Codex => "Codex",
            AiTool::Gemini => "Gemini",
        }
    }
}

pub struct AiResponse {
    pub commands: Vec<String>,
    pub task_name: Option<String>,
    pub category: Option<String>,
}

pub enum AiResult {
    Success(AiResponse),
    /// Full YAML response (used by AI update mode)
    Yaml(String),
    Error(String),
}

/// Check PATH for `claude`, `codex`, or `gemini`. Returns first found.
pub fn detect_ai_tool() -> Option<AiTool> {
    if which("claude") {
        Some(AiTool::Claude)
    } else if which("codex") {
        Some(AiTool::Codex)
    } else if which("gemini") {
        Some(AiTool::Gemini)
    } else {
        None
    }
}

fn which(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build a prompt section describing configured MCP servers.
/// Returns empty string if no servers or mcp feature is disabled.
pub fn build_mcp_prompt_section(server_aliases: &[String]) -> String {
    if server_aliases.is_empty() || !cfg!(feature = "mcp") {
        return String::new();
    }

    let server_list: String = server_aliases
        .iter()
        .map(|s| format!("  - {}", s))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "\n\nMCP SERVERS AVAILABLE:\n\
         The following MCP servers are configured and can be used via mcp: steps instead of shell commands:\n\
         {}\n\n\
         When a matching MCP server is available, PREFER using mcp: steps over curl/shell commands.\n\
         MCP step YAML syntax:\n\
         ```yaml\n\
         - id: step-name\n\
           mcp:\n\
             server: <server-alias>\n\
             tool: <tool-name>\n\
             args:\n\
               key: value\n\
         ```\n\
         Example with template variables:\n\
         ```yaml\n\
         - id: create-issue\n\
           mcp:\n\
             server: github\n\
             tool: create_issue\n\
             args:\n\
               repo: \"{{{{repo}}}}\"\n\
               title: \"Release {{{{version}}}}\"\n\
         ```\n\
         You may mix mcp: steps and cmd: steps in the same workflow.\n\
         Each step must have exactly one of: cmd, call, or mcp.",
        server_list
    )
}

/// Invoke AI synchronously. Crafts a system prompt requesting raw shell commands
/// plus a task name and category, calls `claude -p` or `codex exec`, parses response.
pub fn invoke_ai(tool: AiTool, user_prompt: &str, mcp_server_aliases: &[String]) -> AiResult {
    let mcp_section = build_mcp_prompt_section(mcp_server_aliases);
    let prompt = format!(
        "You are a Linux sysadmin assistant. Generate shell commands for a workflow task.\n\n\
         STRICT OUTPUT FORMAT — follow exactly:\n\
         - Line 1: TASK_NAME: a-descriptive-kebab-name that clearly describes the task (e.g. check-nginx-status, backup-postgres-db, top-cpu-processes, list-open-ports, disk-usage-report)\n\
         - Line 2: CATEGORY: one-word category (e.g. monitoring, backup, deploy, database, system, docker, network)\n\
         - Remaining lines: ONLY executable shell commands, one per line\n\n\
         CRITICAL RULES:\n\
         - Output ONLY valid shell commands that can be passed to bash -c\n\
         - Do NOT include explanations, descriptions, markdown, bullet points, or prose\n\
         - Do NOT include lines like \"Here are the commands...\" or \"This will...\"\n\
         - Do NOT describe what commands do in separate lines — use echo statements instead\n\
         - Use echo statements for human-readable output (e.g. echo \"Checking nginx status...\")\n\
         - Every line after TASK_NAME/CATEGORY must be a runnable shell command\n\
         - No markdown fencing, no line numbers, no commentary\n\
         - IMPORTANT: Scripts may run under `set -e`. Never use `[ ... ] && cmd` or `test ... && cmd` \
           patterns — when the test is false, `[` returns exit code 1 which kills the script under set -e. \
           Always use `if [ ... ]; then cmd; fi` instead{}\n\n\
         User request: {}",
        mcp_section, user_prompt
    );

    let output = match tool {
        AiTool::Claude => Command::new("claude")
            .arg("-p")
            .arg(&prompt)
            .output(),
        AiTool::Codex => Command::new("codex")
            .arg("exec")
            .arg(&prompt)
            .output(),
        AiTool::Gemini => Command::new("gemini")
            .arg("-p")
            .arg(&prompt)
            .output(),
    };

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
            let response = parse_ai_response(&stdout);
            if response.commands.is_empty() {
                AiResult::Error("AI returned no usable commands".to_string())
            } else {
                AiResult::Success(response)
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
            AiResult::Error(format!(
                "AI tool exited with code {}: {}",
                o.status.code().unwrap_or(-1),
                stderr.lines().next().unwrap_or("unknown error")
            ))
        }
        Err(e) => AiResult::Error(format!("Failed to run AI tool: {}", e)),
    }
}

/// Invoke AI to update an existing workflow YAML based on user instructions.
pub fn invoke_ai_update(tool: AiTool, existing_yaml: &str, user_prompt: &str, mcp_server_aliases: &[String]) -> AiResult {
    let mcp_section = build_mcp_prompt_section(mcp_server_aliases);
    let prompt = format!(
        "You are a Linux sysadmin assistant. You will receive an existing workflow YAML \
         and instructions to update it.\n\n\
         STRICT OUTPUT FORMAT:\n\
         - Output ONLY the complete updated YAML workflow\n\
         - The YAML must be valid and parseable\n\
         - Preserve the `name:` and `steps:` structure\n\
         - Do NOT include markdown fencing, explanations, or commentary\n\
         - Do NOT include ```yaml or ``` markers\n\
         - Each step must have `id:` and `cmd:` or `mcp:` fields\n\
         - Preserve existing step IDs where possible\n\
         - You may add, remove, reorder, or modify steps as requested\n\
         - IMPORTANT: Scripts may run under `set -e`. Never use `[ ... ] && cmd` or `test ... && cmd` \
           patterns — when the test is false, `[` returns exit code 1 which kills the script under set -e. \
           Always use `if [ ... ]; then cmd; fi` instead{}\n\n\
         EXISTING WORKFLOW:\n{}\n\n\
         UPDATE INSTRUCTIONS: {}",
        mcp_section, existing_yaml, user_prompt
    );

    let output = match tool {
        AiTool::Claude => Command::new("claude")
            .arg("-p")
            .arg(&prompt)
            .output(),
        AiTool::Codex => Command::new("codex")
            .arg("exec")
            .arg(&prompt)
            .output(),
        AiTool::Gemini => Command::new("gemini")
            .arg("-p")
            .arg(&prompt)
            .output(),
    };

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
            match parse_ai_yaml_response(&stdout) {
                Ok(yaml) => AiResult::Yaml(yaml),
                Err(msg) => AiResult::Error(msg),
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
            AiResult::Error(format!(
                "AI tool exited with code {}: {}",
                o.status.code().unwrap_or(-1),
                stderr.lines().next().unwrap_or("unknown error")
            ))
        }
        Err(e) => AiResult::Error(format!("Failed to run AI tool: {}", e)),
    }
}

/// Parse AI YAML response: strip markdown fencing and surrounding prose.
pub fn parse_ai_yaml_response(raw: &str) -> std::result::Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("AI returned empty response".to_string());
    }

    let mut lines: Vec<&str> = trimmed.lines().collect();

    // Strip markdown fencing
    if lines.first().map(|l| l.trim().starts_with("```")).unwrap_or(false) {
        lines.remove(0);
    }
    if lines.last().map(|l| l.trim() == "```").unwrap_or(false) {
        lines.pop();
    }

    // Find the first line starting with "name:" to strip leading prose
    let start = lines.iter().position(|l| l.trim_start().starts_with("name:"));
    let start = match start {
        Some(i) => i,
        None => return Err("AI response does not contain a valid workflow (missing 'name:')".to_string()),
    };
    let lines = &lines[start..];

    let yaml = lines.join("\n");

    // Basic validation
    if !yaml.contains("steps:") {
        return Err("AI response does not contain 'steps:' section".to_string());
    }

    Ok(yaml)
}

/// Invoke AI synchronously and return raw stdout for free-form responses.
pub fn invoke_ai_raw(tool: AiTool, prompt: &str) -> std::result::Result<String, String> {
    let output = match tool {
        AiTool::Claude => Command::new("claude")
            .arg("-p")
            .arg(prompt)
            .output(),
        AiTool::Codex => Command::new("codex")
            .arg("exec")
            .arg(prompt)
            .output(),
        AiTool::Gemini => Command::new("gemini")
            .arg("-p")
            .arg(prompt)
            .output(),
    };

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
            if stdout.trim().is_empty() {
                Err("AI returned empty response".to_string())
            } else {
                Ok(stdout)
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
            Err(format!(
                "AI tool exited with code {}: {}",
                o.status.code().unwrap_or(-1),
                stderr.lines().next().unwrap_or("unknown error")
            ))
        }
        Err(e) => Err(format!("Failed to run AI tool: {}", e)),
    }
}

/// Parse AI stdout: extract TASK_NAME/CATEGORY metadata, then shell commands.
fn parse_ai_response(raw: &str) -> AiResponse {
    let mut task_name = None;
    let mut category = None;
    let mut commands = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip markdown fences
        if trimmed.starts_with("```") {
            continue;
        }

        // Extract metadata lines (case-insensitive)
        let upper = trimmed.to_uppercase();
        if upper.starts_with("TASK_NAME:") || upper.starts_with("TASK NAME:") {
            let val = trimmed[trimmed.find(':').unwrap() + 1..].trim();
            if !val.is_empty() {
                task_name = Some(sanitize_name(val));
            }
            continue;
        }
        if upper.starts_with("CATEGORY:") {
            let val = trimmed[trimmed.find(':').unwrap() + 1..].trim();
            if !val.is_empty() {
                category = Some(sanitize_name(val));
            }
            continue;
        }

        // Skip comment lines
        if trimmed.starts_with('#') {
            continue;
        }

        // Strip leading `$ `
        let l = trimmed.strip_prefix("$ ").unwrap_or(trimmed);
        // Strip numbered prefixes like `1. `, `2) `
        let l = strip_number_prefix(l);
        if !l.is_empty() && looks_like_command(l) {
            commands.push(l.to_string());
        }
    }

    AiResponse { commands, task_name, category }
}

/// Sanitize a name to kebab-case [a-z0-9-], max 30 chars.
/// Splits smushed words: "top5cpu" → "top5-cpu", "checkNginx" → "check-nginx"
fn sanitize_name(s: &str) -> String {
    // Insert hyphens at word boundaries before lowercasing
    let mut expanded = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            // digit→letter or letter→digit boundary (e.g. top5cpu → top5-cpu)
            // lowercase→uppercase boundary (e.g. checkNginx → check-Nginx)
            if (prev.is_ascii_digit() && c.is_ascii_alphabetic())
                || (prev.is_ascii_alphabetic() && c.is_ascii_digit())
                || (prev.is_ascii_lowercase() && c.is_ascii_uppercase())
            {
                expanded.push('-');
            }
        }
        expanded.push(c);
    }

    let collapsed: String = expanded
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
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
        let end = name.char_indices().map(|(i,_)|i).take_while(|&i| i<=30).last().unwrap_or(0);
        name[..end].to_string()
    } else { name }
}

/// Heuristic: reject lines that look like prose/markdown rather than shell commands.
/// A valid command should start with something that could be a command name or shell builtin.
fn looks_like_command(line: &str) -> bool {
    // Reject markdown bullet points
    if line.starts_with("- ") || line.starts_with("* ") {
        return false;
    }

    // Reject lines starting with markdown bold/italic
    if line.starts_with("**") || line.starts_with("__") {
        return false;
    }

    // Get the first word
    let first_word = line.split_whitespace().next().unwrap_or("");

    // Reject empty
    if first_word.is_empty() {
        return false;
    }

    // Lines that start with a capital letter and don't contain '=' or '/' or start
    // with known shell patterns are likely prose. Shell commands rarely start with
    // uppercase unless they are env vars (FOO=bar cmd) or paths.
    if first_word.starts_with(|c: char| c.is_ascii_uppercase()) {
        // Allow: ENV_VAR=value, /usr/bin/Something, commands with = (assignment)
        if first_word.contains('=') || first_word.starts_with('/') {
            return true;
        }
        // Known uppercase commands/builtins
        let upper_commands = ["PATH", "HOME", "LANG", "LC_ALL", "TZ"];
        if upper_commands.iter().any(|&c| first_word.starts_with(c)) {
            return true;
        }
        return false;
    }

    // Reject lines that end with ':' and have no shell metacharacters — likely labels/headers
    if line.ends_with(':') && !line.contains('|') && !line.contains(';') && !line.contains('&') {
        // Allow "cmd:" only if it looks like a real command (contains / or is short)
        let no_colon = &line[..line.len() - 1];
        if no_colon.contains(' ') && !no_colon.contains('/') {
            return false;
        }
    }

    true
}

/// Strip leading number prefixes: "1. cmd", "2) cmd", "3: cmd"
fn strip_number_prefix(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    // Skip digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() {
        // Check for `. `, `) `, `: ` after digits
        if matches!(bytes[i], b'.' | b')' | b':') {
            let rest = &s[i + 1..];
            return rest.strip_prefix(' ').unwrap_or(rest);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_metadata() {
        let raw = "TASK_NAME: check-nginx\nCATEGORY: monitoring\nsystemctl status nginx\ncurl -I http://localhost\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.task_name.as_deref(), Some("check-nginx"));
        assert_eq!(resp.category.as_deref(), Some("monitoring"));
        assert_eq!(resp.commands, vec!["systemctl status nginx", "curl -I http://localhost"]);
    }

    #[test]
    fn test_parse_clean_output() {
        let raw = "systemctl status nginx\ncurl -I http://localhost\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.commands, vec!["systemctl status nginx", "curl -I http://localhost"]);
        assert!(resp.task_name.is_none());
        assert!(resp.category.is_none());
    }

    #[test]
    fn test_parse_markdown_fenced() {
        let raw = "TASK_NAME: backup-db\nCATEGORY: database\n```bash\npg_dump mydb > dump.sql\n```\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.task_name.as_deref(), Some("backup-db"));
        assert_eq!(resp.category.as_deref(), Some("database"));
        assert_eq!(resp.commands, vec!["pg_dump mydb > dump.sql"]);
    }

    #[test]
    fn test_parse_numbered_list() {
        let raw = "1. systemctl status nginx\n2. curl -I http://localhost\n3) echo done\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.commands, vec![
            "systemctl status nginx",
            "curl -I http://localhost",
            "echo done",
        ]);
    }

    #[test]
    fn test_parse_dollar_prefix() {
        let raw = "$ systemctl status nginx\n$ curl http://localhost\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.commands, vec!["systemctl status nginx", "curl http://localhost"]);
    }

    #[test]
    fn test_parse_empty_output() {
        let resp = parse_ai_response("");
        assert!(resp.commands.is_empty());
    }

    #[test]
    fn test_parse_comments_only() {
        let raw = "# This checks nginx\n# Then curls\n";
        let resp = parse_ai_response(raw);
        assert!(resp.commands.is_empty());
    }

    #[test]
    fn test_parse_mixed_noise() {
        let raw = "```\n# Setup\n$ systemctl restart nginx\n\n2. curl -I http://localhost\n```\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.commands, vec!["systemctl restart nginx", "curl -I http://localhost"]);
    }

    #[test]
    fn test_parse_metadata_case_insensitive() {
        let raw = "task_name: My Task\ncategory: Deploy\necho deploy\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.task_name.as_deref(), Some("my-task"));
        assert_eq!(resp.category.as_deref(), Some("deploy"));
        assert_eq!(resp.commands, vec!["echo deploy"]);
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("Check Nginx Status"), "check-nginx-status");
        assert_eq!(sanitize_name("backup_postgres"), "backup-postgres");
        assert_eq!(sanitize_name(""), "task");
        // Word boundary splitting
        assert_eq!(sanitize_name("top5cpu"), "top-5-cpu");
        assert_eq!(sanitize_name("checkNginx"), "check-nginx");
        assert_eq!(sanitize_name("listDiskUsage"), "list-disk-usage");
        assert_eq!(sanitize_name("get5TopProcesses"), "get-5-top-processes");
        // Already kebab-case passes through
        assert_eq!(sanitize_name("check-nginx-status"), "check-nginx-status");
    }

    #[test]
    fn test_strip_number_prefix_no_number() {
        assert_eq!(strip_number_prefix("echo hello"), "echo hello");
    }

    #[test]
    fn test_strip_number_prefix_dot() {
        assert_eq!(strip_number_prefix("1. echo hello"), "echo hello");
    }

    #[test]
    fn test_strip_number_prefix_paren() {
        assert_eq!(strip_number_prefix("2) echo hello"), "echo hello");
    }

    #[test]
    fn test_strip_number_prefix_colon() {
        assert_eq!(strip_number_prefix("3: echo hello"), "echo hello");
    }

    #[test]
    fn test_looks_like_command_rejects_prose() {
        assert!(!looks_like_command("Here are the commands to show CPU usage:"));
        assert!(!looks_like_command("This will display the top 20 processes"));
        assert!(!looks_like_command("- **%CPU** - CPU utilization percentage"));
        assert!(!looks_like_command("- **%MEM** - Memory usage percentage"));
        assert!(!looks_like_command("Want me to run either of these for you?"));
        assert!(!looks_like_command("For a real-time, continuously updating view, you can also use:"));
    }

    #[test]
    fn test_looks_like_command_accepts_commands() {
        assert!(looks_like_command("ps aux --sort=-%cpu | head -20"));
        assert!(looks_like_command("echo 'hello world'"));
        assert!(looks_like_command("systemctl status nginx"));
        assert!(looks_like_command("docker ps -a"));
        assert!(looks_like_command("/usr/bin/top -bn1"));
        assert!(looks_like_command("MY_VAR=hello echo $MY_VAR"));
        assert!(looks_like_command("curl -I http://localhost"));
    }

    #[test]
    fn test_parse_filters_prose_from_ai_output() {
        let raw = "TASK_NAME: top5cpu\nCATEGORY: monitoring\n\
                   Here are the commands to show the most CPU-intensive processes:\n\
                   ps aux --sort=-%cpu | head -20\n\
                   This will display the top 20 processes sorted by CPU usage in descending order. The columns shown are:\n\
                   - **%CPU** - CPU utilization percentage\n\
                   - **%MEM** - Memory usage percentage\n\
                   For a real-time, continuously updating view, you can also use:\n\
                   top -bn1 -o %CPU | head -25\n\
                   Want me to run either of these for you?\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.task_name.as_deref(), Some("top-5-cpu"));
        assert_eq!(resp.category.as_deref(), Some("monitoring"));
        assert_eq!(resp.commands, vec![
            "ps aux --sort=-%cpu | head -20",
            "top -bn1 -o %CPU | head -25",
        ]);
    }

    #[test]
    fn test_parse_ai_yaml_response_clean() {
        let yaml = "name: my-task\nsteps:\n  - id: step-1\n    cmd: echo hello\n";
        let result = parse_ai_yaml_response(yaml).unwrap();
        assert!(result.contains("name: my-task"));
        assert!(result.contains("steps:"));
    }

    #[test]
    fn test_parse_ai_yaml_response_strips_fencing() {
        let raw = "```yaml\nname: my-task\nsteps:\n  - id: step-1\n    cmd: echo hello\n```";
        let result = parse_ai_yaml_response(raw).unwrap();
        assert!(result.starts_with("name:"));
        assert!(!result.contains("```"));
    }

    #[test]
    fn test_parse_ai_yaml_response_strips_prose() {
        let raw = "Here is the updated workflow:\n\nname: my-task\nsteps:\n  - id: step-1\n    cmd: echo hello\n\nThis should work well.";
        let result = parse_ai_yaml_response(raw).unwrap();
        assert!(result.starts_with("name:"));
        assert!(!result.contains("Here is"));
    }

    #[test]
    fn test_parse_ai_yaml_response_missing_name() {
        let raw = "steps:\n  - id: step-1\n    cmd: echo hello\n";
        let result = parse_ai_yaml_response(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ai_yaml_response_missing_steps() {
        let raw = "name: my-task\nother: stuff\n";
        let result = parse_ai_yaml_response(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ai_yaml_response_empty() {
        let result = parse_ai_yaml_response("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_with_echo_statements() {
        let raw = "TASK_NAME: check-nginx\nCATEGORY: monitoring\n\
                   echo '=== Checking Nginx ==='\nsystemctl status nginx\n\
                   echo 'Status check complete'\n";
        let resp = parse_ai_response(raw);
        assert_eq!(resp.commands.len(), 3);
        assert_eq!(resp.commands[0], "echo '=== Checking Nginx ==='");
        assert_eq!(resp.commands[1], "systemctl status nginx");
        assert_eq!(resp.commands[2], "echo 'Status check complete'");
    }

    #[test]
    fn test_build_mcp_prompt_section_with_servers() {
        let servers = vec!["github".to_string(), "slack".to_string(), "postgres".to_string()];
        let section = build_mcp_prompt_section(&servers);
        if cfg!(feature = "mcp") {
            assert!(section.contains("MCP SERVERS AVAILABLE"));
            assert!(section.contains("  - github"));
            assert!(section.contains("  - slack"));
            assert!(section.contains("  - postgres"));
            assert!(section.contains("PREFER using mcp: steps"));
            assert!(section.contains("server: <server-alias>"));
            assert!(section.contains("tool: <tool-name>"));
        } else {
            assert!(section.is_empty());
        }
    }

    #[test]
    fn test_build_mcp_prompt_section_no_servers() {
        let servers: Vec<String> = vec![];
        let section = build_mcp_prompt_section(&servers);
        assert!(section.is_empty());
    }
}
