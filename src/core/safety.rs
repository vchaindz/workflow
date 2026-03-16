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
