use crate::core::catalog;
use crate::core::config::Config;
use crate::error::Result;

const DEFAULT_REPO: &str = "https://github.com/denniszielke/dzworkflows";

pub fn cmd_templates(config: &Config, fetch: bool, json: bool) -> Result<()> {
    let cache_dir = config.workflows_dir.join(".template-cache");

    if fetch {
        eprintln!("Fetching templates from GitHub...");
        match catalog::fetch_templates(&cache_dir, DEFAULT_REPO) {
            Ok(n) => eprintln!("Downloaded {} template(s) to cache", n),
            Err(e) => eprintln!("Fetch failed: {e}"),
        }
    }

    let templates = catalog::all_templates(&cache_dir);

    if json {
        let items: Vec<serde_json::Value> = templates
            .iter()
            .map(|t| {
                serde_json::json!({
                    "category": t.category,
                    "slug": t.slug,
                    "name": t.name,
                    "description": t.description,
                    "source": format!("{}", t.source),
                    "variables": t.variables.iter().map(|v| {
                        serde_json::json!({
                            "name": v.name,
                            "description": v.description,
                            "default": v.default,
                        })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();

        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        if templates.is_empty() {
            println!("No templates found.");
            return Ok(());
        }

        println!(
            "{:<30} {:<35} SOURCE",
            "TEMPLATE", "DESCRIPTION"
        );
        println!("{}", "-".repeat(75));

        for t in &templates {
            let ref_name = format!("{}/{}", t.category, t.slug);
            let desc = t
                .description
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(33)
                .collect::<String>();
            println!("{:<30} {:<35} {}", ref_name, desc, t.source);
        }

        println!("\n{} template(s) available", templates.len());
    }

    Ok(())
}
