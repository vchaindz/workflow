use std::fs;

use crate::cli::args::TrashAction;
use crate::core::config::Config;
use crate::error::Result;

pub fn cmd_trash(config: &Config, action: TrashAction) -> Result<()> {
    let trash_dir = config.workflows_dir.join(".trash");

    match action {
        TrashAction::List => {
            if !trash_dir.exists() {
                println!("Trash is empty.");
                return Ok(());
            }

            let mut entries: Vec<_> = fs::read_dir(&trash_dir)
                .map_err(crate::error::DzError::Io)?
                .filter_map(|e| e.ok())
                .collect();

            if entries.is_empty() {
                println!("Trash is empty.");
                return Ok(());
            }

            entries.sort_by_key(|e| e.file_name());

            println!("{:<22} ORIGINAL NAME", "TRASHED AT");
            println!("{}", "-".repeat(50));

            for entry in &entries {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                // Format: YYYYMMDD_HHMMSS_originalname.ext
                if let Some((timestamp_part, original)) = split_trash_name(&name_str) {
                    println!("{:<22} {}", timestamp_part, original);
                } else {
                    println!("{:<22} {}", "unknown", name_str);
                }
            }

            println!("\n{} file(s) in trash.", entries.len());
        }

        TrashAction::Empty => {
            if !trash_dir.exists() {
                println!("Trash is already empty.");
                return Ok(());
            }

            let count = fs::read_dir(&trash_dir)
                .map_err(crate::error::DzError::Io)?
                .filter_map(|e| e.ok())
                .count();

            if count == 0 {
                println!("Trash is already empty.");
                return Ok(());
            }

            fs::remove_dir_all(&trash_dir)
                .map_err(crate::error::DzError::Io)?;
            fs::create_dir_all(&trash_dir)
                .map_err(crate::error::DzError::Io)?;

            println!("Emptied trash ({} file(s) removed).", count);
        }

        TrashAction::Restore { name } => {
            if !trash_dir.exists() {
                println!("Trash is empty — nothing to restore.");
                return Ok(());
            }

            let entries: Vec<_> = fs::read_dir(&trash_dir)
                .map_err(crate::error::DzError::Io)?
                .filter_map(|e| e.ok())
                .collect();

            // Find matching entry (match against original name portion)
            let matched = entries.iter().find(|e| {
                let fname = e.file_name();
                let fname_str = fname.to_string_lossy();
                if let Some((_, original)) = split_trash_name(&fname_str) {
                    original.contains(&name)
                } else {
                    fname_str.contains(&name)
                }
            });

            let entry = match matched {
                Some(e) => e,
                None => {
                    println!("No trashed file matching '{}'.", name);
                    println!("Use 'workflow trash list' to see available files.");
                    return Ok(());
                }
            };

            let fname = entry.file_name();
            let fname_str = fname.to_string_lossy().to_string();
            let original_name = split_trash_name(&fname_str)
                .map(|(_, orig)| orig.to_string())
                .unwrap_or(fname_str.clone());

            // Restore to workflows root (user can move to category manually)
            let dest = config.workflows_dir.join(&original_name);
            if dest.exists() {
                println!(
                    "Cannot restore: '{}' already exists at {}",
                    original_name,
                    dest.display()
                );
                return Ok(());
            }

            fs::rename(entry.path(), &dest)
                .map_err(crate::error::DzError::Io)?;

            println!("Restored '{}' to {}", original_name, dest.display());
        }
    }

    Ok(())
}

/// Split a trash filename like "20260313_141522_backup.yaml" into
/// ("2026-03-13 14:15:22", "backup.yaml").
fn split_trash_name(name: &str) -> Option<(&str, &str)> {
    // Format: YYYYMMDD_HHMMSS_rest
    // That's 15 characters for the timestamp prefix plus underscore
    if name.len() < 16 || &name[8..9] != "_" || &name[15..16] != "_" {
        return None;
    }
    let timestamp = &name[..15];
    let original = &name[16..];
    Some((timestamp, original))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_trash_name() {
        let (ts, orig) = split_trash_name("20260313_141522_backup.yaml").unwrap();
        assert_eq!(ts, "20260313_141522");
        assert_eq!(orig, "backup.yaml");
    }

    #[test]
    fn test_split_trash_name_invalid() {
        assert!(split_trash_name("backup.yaml").is_none());
        assert!(split_trash_name("short").is_none());
    }
}
