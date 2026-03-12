use std::collections::HashMap;
use std::path::Path;

use crate::error::{DzError, Result};

#[derive(Debug, Clone)]
pub struct TemplateVariable {
    pub name: String,
    pub description: String,
    pub default: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TemplateEntry {
    pub category: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub variables: Vec<TemplateVariable>,
    pub source: TemplateSource,
    pub raw_yaml: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TemplateSource {
    Bundled,
    Cached,
}

impl std::fmt::Display for TemplateSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateSource::Bundled => write!(f, "bundled"),
            TemplateSource::Cached => write!(f, "cached"),
        }
    }
}

// Embedded template contents
const TRIVY_CVE_CHECK: &str = include_str!("../../templates/security/trivy-cve-check.yaml");
const CLAUDE_UPDATE: &str = include_str!("../../templates/tools/claude-update.yaml");
const CODEX_UPDATE: &str = include_str!("../../templates/tools/codex-update.yaml");
const GIT_SYNC: &str = include_str!("../../templates/tools/git-sync.yaml");
const GIT_SYNC_PULL: &str = include_str!("../../templates/tools/git-sync-pull.yaml");
const WEBSITE_CONTENT_CHECK: &str =
    include_str!("../../templates/monitoring/website-content-check.yaml");

// Docker templates
const DOCKER_CONTAINER_STATUS: &str = include_str!("../../templates/docker/container-status.yaml");
const DOCKER_CLEANUP: &str = include_str!("../../templates/docker/cleanup.yaml");
const DOCKER_LOGS_TAIL: &str = include_str!("../../templates/docker/logs-tail.yaml");
const DOCKER_IMAGE_UPDATE: &str = include_str!("../../templates/docker/image-update.yaml");
const DOCKER_NETWORK_INSPECT: &str = include_str!("../../templates/docker/network-inspect.yaml");
const DOCKER_VOLUME_BACKUP: &str = include_str!("../../templates/docker/volume-backup.yaml");
const DOCKER_COMPOSE_STATUS: &str = include_str!("../../templates/docker/compose-status.yaml");
const DOCKER_SECURITY_SCAN: &str = include_str!("../../templates/docker/security-scan.yaml");
const DOCKER_RESTART_UNHEALTHY: &str =
    include_str!("../../templates/docker/restart-unhealthy.yaml");
const DOCKER_RESOURCE_LIMITS: &str = include_str!("../../templates/docker/resource-limits.yaml");

// Kubectl templates
const KUBECTL_CLUSTER_HEALTH: &str = include_str!("../../templates/kubectl/cluster-health.yaml");
const KUBECTL_POD_STATUS: &str = include_str!("../../templates/kubectl/pod-status.yaml");
const KUBECTL_FAILED_PODS: &str = include_str!("../../templates/kubectl/failed-pods.yaml");
const KUBECTL_RESOURCE_USAGE: &str = include_str!("../../templates/kubectl/resource-usage.yaml");
const KUBECTL_DEPLOYMENT_STATUS: &str =
    include_str!("../../templates/kubectl/deployment-status.yaml");
const KUBECTL_SERVICE_ENDPOINTS: &str =
    include_str!("../../templates/kubectl/service-endpoints.yaml");
const KUBECTL_PV_STORAGE: &str = include_str!("../../templates/kubectl/pv-storage.yaml");
const KUBECTL_NAMESPACE_AUDIT: &str = include_str!("../../templates/kubectl/namespace-audit.yaml");
const KUBECTL_SECRET_CONFIGMAP: &str =
    include_str!("../../templates/kubectl/secret-configmap-audit.yaml");
const KUBECTL_RBAC_REVIEW: &str = include_str!("../../templates/kubectl/rbac-review.yaml");

// Sysadmin templates
const DISK_USAGE: &str = include_str!("../../templates/sysadmin/disk-usage.yaml");
const MEMORY_CHECK: &str = include_str!("../../templates/sysadmin/memory-check.yaml");
const SERVICE_STATUS: &str = include_str!("../../templates/sysadmin/service-status.yaml");
const LOG_CLEANUP: &str = include_str!("../../templates/sysadmin/log-cleanup.yaml");
const SYSTEM_UPDATE: &str = include_str!("../../templates/sysadmin/system-update.yaml");
const FAILED_SERVICES: &str = include_str!("../../templates/sysadmin/failed-services.yaml");
const PORT_SCAN: &str = include_str!("../../templates/sysadmin/port-scan.yaml");
const USER_AUDIT: &str = include_str!("../../templates/sysadmin/user-audit.yaml");
const BACKUP_VERIFY: &str = include_str!("../../templates/sysadmin/backup-verify.yaml");
const CPU_LOAD: &str = include_str!("../../templates/sysadmin/cpu-load.yaml");
const SSL_CERT_EXPIRY: &str = include_str!("../../templates/sysadmin/ssl-cert-expiry.yaml");
const SMART_DISK_HEALTH: &str = include_str!("../../templates/sysadmin/smart-disk-health.yaml");
const NTP_SYNC_CHECK: &str = include_str!("../../templates/sysadmin/ntp-sync-check.yaml");
const CRON_AUDIT: &str = include_str!("../../templates/sysadmin/cron-audit.yaml");
const SSH_KEY_AUDIT: &str = include_str!("../../templates/sysadmin/ssh-key-audit.yaml");
const FIREWALL_REVIEW: &str = include_str!("../../templates/sysadmin/firewall-review.yaml");

/// Parse template metadata (name, description, variables) from YAML text.
fn parse_template_meta(yaml: &str) -> (String, Option<String>, Vec<TemplateVariable>) {
    let value: serde_yaml::Value = match serde_yaml::from_str(yaml) {
        Ok(v) => v,
        Err(_) => return ("Unknown".to_string(), None, Vec::new()),
    };

    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let variables = value
        .get("variables")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?.to_string();
                    let desc = item
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let default = item
                        .get("default")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    Some(TemplateVariable {
                        name,
                        description: desc,
                        default,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    (name, description, variables)
}

/// Return all bundled (compiled-in) templates.
pub fn bundled_templates() -> Vec<TemplateEntry> {
    let registry: Vec<(&str, &str, &str)> = vec![
        ("security", "trivy-cve-check", TRIVY_CVE_CHECK),
        ("tools", "claude-update", CLAUDE_UPDATE),
        ("tools", "codex-update", CODEX_UPDATE),
        ("tools", "git-sync", GIT_SYNC),
        ("tools", "git-sync-pull", GIT_SYNC_PULL),
        ("monitoring", "website-content-check", WEBSITE_CONTENT_CHECK),
        ("docker", "cleanup", DOCKER_CLEANUP),
        ("docker", "compose-status", DOCKER_COMPOSE_STATUS),
        ("docker", "container-status", DOCKER_CONTAINER_STATUS),
        ("docker", "image-update", DOCKER_IMAGE_UPDATE),
        ("docker", "logs-tail", DOCKER_LOGS_TAIL),
        ("docker", "network-inspect", DOCKER_NETWORK_INSPECT),
        ("docker", "resource-limits", DOCKER_RESOURCE_LIMITS),
        ("docker", "restart-unhealthy", DOCKER_RESTART_UNHEALTHY),
        ("docker", "security-scan", DOCKER_SECURITY_SCAN),
        ("docker", "volume-backup", DOCKER_VOLUME_BACKUP),
        ("kubectl", "cluster-health", KUBECTL_CLUSTER_HEALTH),
        ("kubectl", "deployment-status", KUBECTL_DEPLOYMENT_STATUS),
        ("kubectl", "failed-pods", KUBECTL_FAILED_PODS),
        ("kubectl", "namespace-audit", KUBECTL_NAMESPACE_AUDIT),
        ("kubectl", "pod-status", KUBECTL_POD_STATUS),
        ("kubectl", "pv-storage", KUBECTL_PV_STORAGE),
        ("kubectl", "rbac-review", KUBECTL_RBAC_REVIEW),
        ("kubectl", "resource-usage", KUBECTL_RESOURCE_USAGE),
        ("kubectl", "secret-configmap-audit", KUBECTL_SECRET_CONFIGMAP),
        ("kubectl", "service-endpoints", KUBECTL_SERVICE_ENDPOINTS),
        ("sysadmin", "backup-verify", BACKUP_VERIFY),
        ("sysadmin", "cpu-load", CPU_LOAD),
        ("sysadmin", "disk-usage", DISK_USAGE),
        ("sysadmin", "failed-services", FAILED_SERVICES),
        ("sysadmin", "log-cleanup", LOG_CLEANUP),
        ("sysadmin", "memory-check", MEMORY_CHECK),
        ("sysadmin", "port-scan", PORT_SCAN),
        ("sysadmin", "service-status", SERVICE_STATUS),
        ("sysadmin", "system-update", SYSTEM_UPDATE),
        ("sysadmin", "user-audit", USER_AUDIT),
        ("sysadmin", "cron-audit", CRON_AUDIT),
        ("sysadmin", "firewall-review", FIREWALL_REVIEW),
        ("sysadmin", "ntp-sync-check", NTP_SYNC_CHECK),
        ("sysadmin", "smart-disk-health", SMART_DISK_HEALTH),
        ("sysadmin", "ssh-key-audit", SSH_KEY_AUDIT),
        ("sysadmin", "ssl-cert-expiry", SSL_CERT_EXPIRY),
    ];

    registry
        .into_iter()
        .map(|(category, slug, raw)| {
            let (name, description, variables) = parse_template_meta(raw);
            TemplateEntry {
                category: category.to_string(),
                slug: slug.to_string(),
                name,
                description,
                variables,
                source: TemplateSource::Bundled,
                raw_yaml: raw.to_string(),
            }
        })
        .collect()
}

/// Scan a cache directory for downloaded templates.
pub fn cached_templates(cache_dir: &Path) -> Vec<TemplateEntry> {
    let mut entries = Vec::new();
    if !cache_dir.exists() {
        return entries;
    }

    for cat_entry in walkdir::WalkDir::new(cache_dir)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !cat_entry.path().is_dir() {
            continue;
        }
        let category = cat_entry
            .file_name()
            .to_str()
            .unwrap_or("unknown")
            .to_string();

        for file_entry in walkdir::WalkDir::new(cat_entry.path())
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = file_entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "yaml" && ext != "yml" {
                continue;
            }

            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let raw = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let (name, description, variables) = parse_template_meta(&raw);
            entries.push(TemplateEntry {
                category: category.clone(),
                slug,
                name,
                description,
                variables,
                source: TemplateSource::Cached,
                raw_yaml: raw,
            });
        }
    }

    entries
}

/// Merge bundled and cached templates. Cached entries override bundled ones
/// with the same category/slug.
pub fn all_templates(cache_dir: &Path) -> Vec<TemplateEntry> {
    let mut map: HashMap<String, TemplateEntry> = HashMap::new();

    for entry in bundled_templates() {
        let key = format!("{}/{}", entry.category, entry.slug);
        map.insert(key, entry);
    }

    for entry in cached_templates(cache_dir) {
        let key = format!("{}/{}", entry.category, entry.slug);
        map.insert(key, entry);
    }

    let mut result: Vec<TemplateEntry> = map.into_values().collect();
    result.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then(a.slug.cmp(&b.slug))
    });
    result
}

/// Download templates from a GitHub repository tarball into the cache directory.
pub fn fetch_templates(cache_dir: &Path, repo_url: &str) -> Result<usize> {
    std::fs::create_dir_all(cache_dir)?;

    // Build tarball URL from repo URL
    let tarball_url = if repo_url.contains("/archive/") {
        repo_url.to_string()
    } else {
        format!(
            "{}/archive/refs/heads/main.tar.gz",
            repo_url.trim_end_matches('/')
        )
    };

    // Download using curl
    let output = std::process::Command::new("curl")
        .args(["-sL", "--fail", &tarball_url])
        .output()
        .map_err(|e| DzError::Template(format!("failed to run curl: {e}")))?;

    if !output.status.success() {
        return Err(DzError::Template(format!(
            "failed to download templates: HTTP error ({})",
            output.status
        )));
    }

    // Extract tar.gz
    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(&output.stdout));
    let mut archive = tar::Archive::new(decoder);

    let mut count = 0;

    for entry in archive.entries().map_err(|e| DzError::Template(format!("tar error: {e}")))? {
        let mut entry = entry.map_err(|e| DzError::Template(format!("tar entry error: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| DzError::Template(format!("tar path error: {e}")))?
            .to_path_buf();

        // Look for templates/{category}/{file}.yaml inside the archive
        let components: Vec<&str> = path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        // Pattern: {repo-name}/templates/{category}/{file}.yaml
        if let Some(templates_pos) = components.iter().position(|c| *c == "templates") {
            if components.len() == templates_pos + 3 {
                let cat = components[templates_pos + 1];
                let filename = components[templates_pos + 2];

                // Reject traversal attempts
                if cat.contains("..") || cat.contains('/') || cat.contains('\\')
                    || filename.contains("..") || filename.contains('/') || filename.contains('\\')
                {
                    continue;
                }

                if filename.ends_with(".yaml") || filename.ends_with(".yml") {
                    let dest_dir = cache_dir.join(cat);
                    std::fs::create_dir_all(&dest_dir)?;
                    let dest_file = dest_dir.join(filename);
                    let mut dest = std::fs::File::create(&dest_file)?;
                    std::io::copy(&mut entry, &mut dest)?;
                    count += 1;
                }
            }
        }
    }

    Ok(count)
}

/// Extract template variable placeholders from YAML text.
/// Finds `{{var}}` patterns excluding builtins (date, datetime, hostname).
pub fn extract_variables(yaml: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\{\{(\w+)\}\}").unwrap();
    let builtins = ["date", "datetime", "hostname"];

    let mut seen = std::collections::HashSet::new();
    let mut vars = Vec::new();

    for cap in re.captures_iter(yaml) {
        let name = cap[1].to_string();
        if !builtins.contains(&name.as_str()) && seen.insert(name.clone()) {
            vars.push(name);
        }
    }

    vars
}

/// Instantiate a template by substituting variable values and stripping
/// template metadata (description, variables) from the output.
pub fn instantiate_template(entry: &TemplateEntry, values: &HashMap<String, String>) -> String {
    // Expand variables using the existing template engine
    let expanded = crate::core::template::expand_template(&entry.raw_yaml, values);

    // Strip template-only metadata fields (description, variables)
    strip_template_metadata(&expanded)
}

/// Remove `description:` and `variables:` blocks from YAML text.
fn strip_template_metadata(yaml: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut skip_block = false;

    for line in yaml.lines() {
        if line.starts_with("description:") {
            continue;
        }
        if line.starts_with("variables:") {
            skip_block = true;
            continue;
        }
        if skip_block {
            // Variable block entries are indented
            if line.starts_with("  ") || line.starts_with("\t") {
                continue;
            }
            skip_block = false;
        }
        lines.push(line);
    }

    let result = lines.join("\n");
    // Remove any resulting double blank lines
    let re = regex::Regex::new(r"\n{3,}").unwrap();
    re.replace_all(&result, "\n\n").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundled_templates_not_empty() {
        let templates = bundled_templates();
        assert!(!templates.is_empty());
        assert!(templates.len() >= 4);
    }

    #[test]
    fn test_bundled_template_metadata() {
        let templates = bundled_templates();
        let trivy = templates
            .iter()
            .find(|t| t.slug == "trivy-cve-check")
            .unwrap();
        assert_eq!(trivy.category, "security");
        assert_eq!(trivy.name, "Trivy CVE Check");
        assert!(trivy.description.is_some());
        assert!(!trivy.variables.is_empty());
        assert_eq!(trivy.variables[0].name, "severity");
        assert_eq!(trivy.source, TemplateSource::Bundled);
    }

    #[test]
    fn test_extract_variables() {
        let yaml = "env:\n  DZ_URL: \"{{url}}\"\n  DZ_DATE: \"{{date}}\"\n  DZ_NAME: \"{{name}}\"";
        let vars = extract_variables(yaml);
        assert!(vars.contains(&"url".to_string()));
        assert!(vars.contains(&"name".to_string()));
        assert!(!vars.contains(&"date".to_string())); // builtin excluded
    }

    #[test]
    fn test_strip_template_metadata() {
        let yaml = "name: Test\ndescription: A test template\nvariables:\n  - name: foo\n    default: bar\nsteps:\n  - id: s1\n    cmd: echo hi\n";
        let stripped = strip_template_metadata(yaml);
        assert!(stripped.contains("name: Test"));
        assert!(stripped.contains("steps:"));
        assert!(!stripped.contains("description:"));
        assert!(!stripped.contains("variables:"));
        assert!(!stripped.contains("- name: foo"));
    }

    #[test]
    fn test_instantiate_template() {
        let entry = TemplateEntry {
            category: "test".to_string(),
            slug: "test".to_string(),
            name: "Test".to_string(),
            description: Some("A test".to_string()),
            variables: vec![TemplateVariable {
                name: "url".to_string(),
                description: "URL".to_string(),
                default: Some("https://example.com".to_string()),
            }],
            source: TemplateSource::Bundled,
            raw_yaml: "name: Test\ndescription: A test\nvariables:\n  - name: url\n    default: https://example.com\nenv:\n  DZ_URL: \"{{url}}\"\nsteps:\n  - id: s1\n    cmd: echo $DZ_URL\n".to_string(),
        };

        let mut values = HashMap::new();
        values.insert("url".to_string(), "https://mysite.com".to_string());
        let result = instantiate_template(&entry, &values);

        assert!(result.contains("https://mysite.com"));
        assert!(!result.contains("description:"));
        assert!(!result.contains("variables:"));
        assert!(result.contains("name: Test"));
        assert!(result.contains("steps:"));
    }

    #[test]
    fn test_all_templates_with_no_cache() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = tmp.path().join(".template-cache");
        let templates = all_templates(&cache);
        assert!(templates.len() >= 4);
    }

    #[test]
    fn test_cached_templates_override_bundled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = tmp.path().join(".template-cache");
        std::fs::create_dir_all(cache.join("security")).unwrap();
        std::fs::write(
            cache.join("security/trivy-cve-check.yaml"),
            "name: Custom Trivy\nsteps:\n  - id: s1\n    cmd: echo custom\n",
        )
        .unwrap();

        let templates = all_templates(&cache);
        let trivy = templates
            .iter()
            .find(|t| t.slug == "trivy-cve-check")
            .unwrap();
        assert_eq!(trivy.source, TemplateSource::Cached);
        assert_eq!(trivy.name, "Custom Trivy");
    }

    #[test]
    fn test_template_source_display() {
        assert_eq!(format!("{}", TemplateSource::Bundled), "bundled");
        assert_eq!(format!("{}", TemplateSource::Cached), "cached");
    }
}
