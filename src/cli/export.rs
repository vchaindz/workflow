use std::fs::File;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::Builder;

use crate::core::config::Config;
use crate::error::Result;

pub fn cmd_export(config: &Config, output: Option<&Path>, include_history: bool) -> Result<()> {
    let workflows_dir = &config.workflows_dir;

    if !workflows_dir.exists() {
        eprintln!("Workflows directory does not exist: {}", workflows_dir.display());
        return Ok(());
    }

    let output_path = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let date = chrono::Local::now().format("%Y%m%d-%H%M%S");
            PathBuf::from(format!("workflow-export-{}.tar.gz", date))
        }
    };

    let file = File::create(&output_path)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut archive = Builder::new(enc);

    let mut count = 0u32;

    for entry in walkdir::WalkDir::new(workflows_dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let full_path = entry.path();
        let rel_path = full_path.strip_prefix(workflows_dir).unwrap_or(full_path);
        let rel_str = rel_path.to_string_lossy();

        // Skip logs directory
        if rel_str.starts_with("logs") || rel_str.starts_with("logs/") {
            continue;
        }

        // Skip history.db unless requested
        if rel_str == "history.db" || rel_str.starts_with("history.db-") {
            if !include_history {
                continue;
            }
        }

        // Skip config.toml (user-local config)
        if rel_str == "config.toml" {
            continue;
        }

        if full_path.is_file() {
            archive.append_path_with_name(full_path, rel_path)?;
            count += 1;
        } else if full_path.is_dir() {
            archive.append_dir(rel_path, full_path)?;
        }
    }

    archive.into_inner()?.finish()?;

    println!("Exported {} files to {}", count, output_path.display());
    if include_history {
        println!("(includes run history database)");
    }

    Ok(())
}
