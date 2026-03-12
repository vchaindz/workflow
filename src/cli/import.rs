use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;

use crate::core::config::Config;
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq)]
enum ConflictChoice {
    Overwrite,
    Skip,
    OverwriteAll,
    SkipAll,
}

pub fn cmd_import(
    config: &Config,
    archive_path: &Path,
    overwrite: bool,
    skip_existing: bool,
) -> Result<()> {
    if !archive_path.exists() {
        eprintln!("Archive not found: {}", archive_path.display());
        return Ok(());
    }

    let workflows_dir = &config.workflows_dir;
    fs::create_dir_all(workflows_dir)?;

    // First pass: collect all entries to detect conflicts
    let file = File::open(archive_path)?;
    let dec = GzDecoder::new(file);
    let mut archive = Archive::new(dec);

    let mut conflicts: Vec<PathBuf> = Vec::new();
    let mut all_entries: Vec<PathBuf> = Vec::new();

    for entry in archive.entries()? {
        let entry = entry?;
        let raw_path = entry.path()?.into_owned();
        let path = match sanitize_archive_path(&raw_path) {
            Some(p) => p,
            None => continue,
        };
        if entry.header().entry_type().is_file() {
            let dest = workflows_dir.join(&path);
            all_entries.push(path.clone());
            if dest.exists() {
                conflicts.push(path);
            }
        }
    }

    if all_entries.is_empty() {
        println!("Archive is empty, nothing to import.");
        return Ok(());
    }

    println!(
        "Archive contains {} file(s), {} conflict(s) with existing workflows.",
        all_entries.len(),
        conflicts.len()
    );

    // Determine per-file actions for conflicts
    let mut actions: HashMap<PathBuf, ConflictChoice> = HashMap::new();

    if overwrite {
        // --overwrite flag: overwrite everything
        for c in &conflicts {
            actions.insert(c.clone(), ConflictChoice::Overwrite);
        }
    } else if skip_existing {
        // --skip-existing flag: skip everything
        for c in &conflicts {
            actions.insert(c.clone(), ConflictChoice::Skip);
        }
    } else if !conflicts.is_empty() {
        // Interactive prompt for each conflict
        let mut all_choice: Option<ConflictChoice> = None;

        for conflict in &conflicts {
            if let Some(choice) = all_choice {
                actions.insert(conflict.clone(), choice);
                continue;
            }

            let choice = prompt_conflict(conflict)?;
            match choice {
                ConflictChoice::OverwriteAll => {
                    all_choice = Some(ConflictChoice::Overwrite);
                    actions.insert(conflict.clone(), ConflictChoice::Overwrite);
                }
                ConflictChoice::SkipAll => {
                    all_choice = Some(ConflictChoice::Skip);
                    actions.insert(conflict.clone(), ConflictChoice::Skip);
                }
                other => {
                    actions.insert(conflict.clone(), other);
                }
            }
        }
    }

    // Second pass: extract files
    let file = File::open(archive_path)?;
    let dec = GzDecoder::new(file);
    let mut archive = Archive::new(dec);

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut overwritten = 0u32;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.into_owned();
        let path = match sanitize_archive_path(&raw_path) {
            Some(p) => p,
            None => continue,
        };
        let dest = workflows_dir.join(&path);

        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }

        if !entry.header().entry_type().is_file() {
            continue;
        }

        // Check if this is a conflict
        if dest.exists() {
            match actions.get(&path) {
                Some(ConflictChoice::Skip) => {
                    skipped += 1;
                    continue;
                }
                Some(ConflictChoice::Overwrite) => {
                    overwritten += 1;
                }
                _ => {
                    // No conflict entry means file didn't exist in first pass
                }
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        // Read content and write to destination
        let mut content = Vec::new();
        entry.read_to_end(&mut content)?;
        let mut out = File::create(&dest)?;
        out.write_all(&content)?;

        // Preserve executable permissions for .sh files
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if dest.extension().is_some_and(|ext| ext == "sh") {
                fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
            }
        }

        imported += 1;
    }

    println!("Import complete:");
    println!("  {} file(s) imported", imported);
    if overwritten > 0 {
        println!("  {} file(s) overwritten", overwritten);
    }
    if skipped > 0 {
        println!("  {} file(s) skipped (kept existing)", skipped);
    }

    Ok(())
}

/// Reject archive paths that could escape the target directory.
fn sanitize_archive_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        eprintln!("  Skipping absolute path: {}", path.display());
        return None;
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            eprintln!("  Skipping path with '..': {}", path.display());
            return None;
        }
    }
    Some(path.to_path_buf())
}

fn prompt_conflict(path: &Path) -> Result<ConflictChoice> {
    eprint!(
        "Conflict: '{}' already exists. [o]verwrite / [s]kip / overwrite [a]ll / skip a[l]l? ",
        path.display()
    );
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    match input.trim().to_lowercase().as_str() {
        "o" | "overwrite" => Ok(ConflictChoice::Overwrite),
        "s" | "skip" => Ok(ConflictChoice::Skip),
        "a" | "all" | "overwrite all" => Ok(ConflictChoice::OverwriteAll),
        "l" | "skip all" => Ok(ConflictChoice::SkipAll),
        _ => {
            eprintln!("  Unknown choice, skipping this file.");
            Ok(ConflictChoice::Skip)
        }
    }
}
