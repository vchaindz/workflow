use std::path::Path;
use walkdir::WalkDir;

use crate::core::models::{Category, Task, TaskKind};
use crate::error::{DzError, Result};

const DEFAULT_CATEGORY: &str = "_default";

pub fn scan_workflows(root: &Path) -> Result<Vec<Category>> {
    if !root.exists() {
        return Err(DzError::Discovery(format!(
            "workflows directory does not exist: {}",
            root.display()
        )));
    }

    let mut categories: std::collections::BTreeMap<String, Category> =
        std::collections::BTreeMap::new();

    for entry in WalkDir::new(root)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip the root directory itself
        if path == root {
            continue;
        }

        // Skip logs/ directory and config.toml
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name == "logs" || name == "config.toml" {
                continue;
            }
        }

        // Skip entries inside logs/
        if path
            .strip_prefix(root)
            .ok()
            .and_then(|p| p.components().next())
            .and_then(|c| c.as_os_str().to_str())
            == Some("logs")
        {
            continue;
        }

        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let kind = match ext {
            "sh" => TaskKind::ShellScript,
            "yaml" | "yml" => TaskKind::YamlWorkflow,
            _ => continue,
        };

        let task_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let relative = path.strip_prefix(root).map_err(|e| {
            DzError::Discovery(format!("failed to get relative path: {e}"))
        })?;

        let category_name = relative
            .parent()
            .and_then(|p| p.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_CATEGORY)
            .to_string();

        let task = Task {
            name: task_name,
            kind,
            path: path.to_path_buf(),
            category: category_name.clone(),
            last_run: None,
        };

        categories
            .entry(category_name.clone())
            .or_insert_with(|| Category {
                name: category_name,
                path: path.parent().unwrap_or(root).to_path_buf(),
                tasks: Vec::new(),
            })
            .tasks
            .push(task);
    }

    // Sort tasks within each category
    for cat in categories.values_mut() {
        cat.tasks.sort_by(|a, b| a.name.cmp(&b.name));
    }

    Ok(categories.into_values().collect())
}

/// Resolve a task reference like "backup/db-full" or "backup.db-full"
pub fn resolve_task_ref<'a>(
    categories: &'a [Category],
    ref_str: &str,
) -> Result<&'a Task> {
    // Normalize: accept both "/" and "." as separator
    let normalized = ref_str.replace('.', "/");
    let parts: Vec<&str> = normalized.splitn(2, '/').collect();

    let (cat_name, task_name) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        // Search all categories for a matching task
        let task_name = parts[0];
        for cat in categories {
            if let Some(task) = cat.tasks.iter().find(|t| t.name == task_name) {
                return Ok(task);
            }
        }
        return Err(DzError::TaskNotFound(ref_str.to_string()));
    };

    for cat in categories {
        if cat.name == cat_name {
            if let Some(task) = cat.tasks.iter().find(|t| t.name == task_name) {
                return Ok(task);
            }
        }
    }

    Err(DzError::TaskNotFound(ref_str.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("backup")).unwrap();
        fs::create_dir_all(root.join("deploy")).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();

        fs::write(root.join("backup/db-full.sh"), "#!/bin/bash\necho backup").unwrap();
        fs::write(
            root.join("backup/mysql-daily.yaml"),
            "name: test\nsteps:\n  - id: s1\n    cmd: echo hi\n",
        )
        .unwrap();
        fs::write(root.join("deploy/staging.yaml"), "name: deploy\nsteps:\n  - id: s1\n    cmd: echo deploy\n").unwrap();
        fs::write(root.join("quick.sh"), "#!/bin/bash\necho quick").unwrap();
        fs::write(root.join("logs/old.json"), "{}").unwrap();
        fs::write(root.join("config.toml"), "[hooks]").unwrap();

        dir
    }

    #[test]
    fn test_scan_finds_tasks() {
        let dir = setup_test_dir();
        let cats = scan_workflows(dir.path()).unwrap();

        let all_tasks: Vec<&str> = cats
            .iter()
            .flat_map(|c| c.tasks.iter().map(|t| t.name.as_str()))
            .collect();

        assert!(all_tasks.contains(&"db-full"));
        assert!(all_tasks.contains(&"mysql-daily"));
        assert!(all_tasks.contains(&"staging"));
        assert!(all_tasks.contains(&"quick"));
    }

    #[test]
    fn test_scan_skips_logs_and_config() {
        let dir = setup_test_dir();
        let cats = scan_workflows(dir.path()).unwrap();

        let all_tasks: Vec<&str> = cats
            .iter()
            .flat_map(|c| c.tasks.iter().map(|t| t.name.as_str()))
            .collect();

        assert!(!all_tasks.contains(&"old"));
    }

    #[test]
    fn test_root_files_in_default_category() {
        let dir = setup_test_dir();
        let cats = scan_workflows(dir.path()).unwrap();

        let default_cat = cats.iter().find(|c| c.name == "_default");
        assert!(default_cat.is_some());
        assert!(default_cat
            .unwrap()
            .tasks
            .iter()
            .any(|t| t.name == "quick"));
    }

    #[test]
    fn test_resolve_task_ref_slash() {
        let dir = setup_test_dir();
        let cats = scan_workflows(dir.path()).unwrap();
        let task = resolve_task_ref(&cats, "backup/db-full").unwrap();
        assert_eq!(task.name, "db-full");
    }

    #[test]
    fn test_resolve_task_ref_dot() {
        let dir = setup_test_dir();
        let cats = scan_workflows(dir.path()).unwrap();
        let task = resolve_task_ref(&cats, "backup.db-full").unwrap();
        assert_eq!(task.name, "db-full");
    }

    #[test]
    fn test_resolve_task_ref_not_found() {
        let dir = setup_test_dir();
        let cats = scan_workflows(dir.path()).unwrap();
        assert!(resolve_task_ref(&cats, "nonexistent/task").is_err());
    }
}
