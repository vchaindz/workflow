use std::path::Path;

/// Lint a shell script file for common pitfalls (e.g. `set -e` + `[ ... ] && ...`).
/// Returns a list of (line_number, warning_message) pairs.
pub fn lint_shell_script(path: &Path) -> Vec<(usize, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    lint_shell_content(&content)
}

/// Lint shell script content for common pitfalls.
pub fn lint_shell_content(content: &str) -> Vec<(usize, String)> {
    let mut warnings = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Check if script uses `set -e` (or `set -euo pipefail`, etc.)
    let has_set_e = lines.iter().any(|l| {
        let t = l.trim();
        // Match: set -e, set -eu, set -euo pipefail, set -o errexit, etc.
        (t.starts_with("set ") && (t.contains("-e") || t.contains("errexit")))
            && !t.starts_with('#')
    });

    if !has_set_e {
        return warnings;
    }

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        // Pattern: `[ ... ] && cmd` or `[[ ... ]] && cmd` on a single line
        // without `||` (which would provide a fallback exit code).
        // This is the classic set -e pitfall: when the test is false,
        // `[` returns 1 and `set -e` kills the script.
        if (trimmed.starts_with("[ ") || trimmed.starts_with("[[ "))
            && trimmed.contains(" && ")
            && !trimmed.contains(" || ")
        {
            warnings.push((
                i + 1,
                format!(
                    "set -e pitfall: `[ ... ] && cmd` exits the script when the test is false. \
                     Use `if [ ... ]; then cmd; fi` instead. Line: {}",
                    trimmed
                ),
            ));
        }

        // Also catch: `test ... && cmd`
        if trimmed.starts_with("test ")
            && trimmed.contains(" && ")
            && !trimmed.contains(" || ")
        {
            warnings.push((
                i + 1,
                format!(
                    "set -e pitfall: `test ... && cmd` exits the script when the test is false. \
                     Use `if test ...; then cmd; fi` instead. Line: {}",
                    trimmed
                ),
            ));
        }

        // Catch: `grep -q ... && cmd` (grep returns 1 on no match)
        if trimmed.starts_with("grep ")
            && trimmed.contains(" && ")
            && !trimmed.contains(" || ")
            && !trimmed.starts_with("grep -c")
        {
            warnings.push((
                i + 1,
                format!(
                    "set -e pitfall: `grep ... && cmd` exits the script when grep finds no match. \
                     Use `if grep ...; then cmd; fi` instead. Line: {}",
                    trimmed
                ),
            ));
        }
    }

    warnings
}

/// Check if a command matches known dangerous patterns.
/// Returns a warning message if dangerous, None if safe.
pub fn check_dangerous(cmd: &str) -> Option<&'static str> {
    let trimmed = cmd.trim();

    // Fork bomb
    if trimmed.contains(":(){ :|:& };:") || trimmed.contains(":(){:|:&};:") {
        return Some("Fork bomb detected — will crash the system");
    }

    // rm -rf / or rm -rf /* (but not rm -rf /tmp/something)
    let rm_re = regex::Regex::new(r"rm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+(-[a-zA-Z]*r[a-zA-Z]*\s+)?|(-[a-zA-Z]*r[a-zA-Z]*\s+)?-[a-zA-Z]*f[a-zA-Z]*\s+)/\*?\s*$").unwrap();
    if rm_re.is_match(trimmed) {
        return Some("Recursive force-delete of root filesystem detected");
    }

    // dd writing to block devices
    if trimmed.contains("dd ") && trimmed.contains("of=/dev/sd") {
        return Some("Direct write to block device via dd detected");
    }
    if trimmed.contains("dd ") && trimmed.contains("of=/dev/nvme") {
        return Some("Direct write to block device via dd detected");
    }

    // mkfs on real devices (not loop or files)
    let mkfs_re = regex::Regex::new(r"mkfs[\.\s]\S*\s+/dev/sd").unwrap();
    if mkfs_re.is_match(trimmed) {
        return Some("Filesystem creation on real device detected");
    }

    // Redirect to block device
    let dev_redirect_re = regex::Regex::new(r">\s*/dev/sd[a-z]").unwrap();
    if dev_redirect_re.is_match(trimmed) {
        return Some("Output redirect to block device detected");
    }

    // chmod -R 777 /
    let chmod_re = regex::Regex::new(r"chmod\s+(-[a-zA-Z]*R[a-zA-Z]*\s+)?777\s+/\s*$").unwrap();
    if chmod_re.is_match(trimmed) {
        return Some("Recursive chmod 777 on root filesystem detected");
    }

    // mv /* /dev/null
    if trimmed.contains("mv") && trimmed.contains("/dev/null") && trimmed.contains("/*") {
        return Some("Moving filesystem contents to /dev/null detected");
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_set_e_and_bracket() {
        let script = "#!/bin/bash\nset -euo pipefail\n[ \"$X\" -lt 14 ] && echo WARNING\n";
        let w = lint_shell_content(script);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].0, 3);
        assert!(w[0].1.contains("set -e pitfall"));
    }

    #[test]
    fn test_lint_if_then_is_fine() {
        let script = "#!/bin/bash\nset -e\nif [ \"$X\" -lt 14 ]; then\n    echo WARNING\nfi\n";
        let w = lint_shell_content(script);
        assert!(w.is_empty());
    }

    #[test]
    fn test_lint_no_set_e_no_warning() {
        let script = "#!/bin/bash\n[ \"$X\" -lt 14 ] && echo WARNING\n";
        let w = lint_shell_content(script);
        assert!(w.is_empty());
    }

    #[test]
    fn test_lint_or_fallback_suppresses() {
        let script = "#!/bin/bash\nset -e\n[ \"$X\" -lt 14 ] && echo WARNING || true\n";
        let w = lint_shell_content(script);
        assert!(w.is_empty());
    }

    #[test]
    fn test_lint_double_bracket() {
        let script = "#!/bin/bash\nset -euo pipefail\n[[ -f /tmp/foo ]] && rm /tmp/foo\n";
        let w = lint_shell_content(script);
        assert_eq!(w.len(), 1);
        assert!(w[0].1.contains("set -e pitfall"));
    }

    #[test]
    fn test_lint_test_command() {
        let script = "#!/bin/bash\nset -e\ntest -f /tmp/foo && rm /tmp/foo\n";
        let w = lint_shell_content(script);
        assert_eq!(w.len(), 1);
        assert!(w[0].1.contains("test ... && cmd"));
    }

    #[test]
    fn test_lint_grep_and() {
        let script = "#!/bin/bash\nset -e\ngrep -q pattern file.txt && echo found\n";
        let w = lint_shell_content(script);
        assert_eq!(w.len(), 1);
        assert!(w[0].1.contains("grep"));
    }

    #[test]
    fn test_lint_comment_ignored() {
        let script = "#!/bin/bash\nset -e\n# [ \"$X\" -lt 14 ] && echo WARNING\n";
        let w = lint_shell_content(script);
        assert!(w.is_empty());
    }

    #[test]
    fn test_lint_set_o_errexit() {
        let script = "#!/bin/bash\nset -o errexit\n[ -z \"$X\" ] && echo empty\n";
        let w = lint_shell_content(script);
        assert_eq!(w.len(), 1);
    }
}
