use std::io::Read;

use crate::cli::args::SnapshotAction;
use crate::core::config::Config;
use crate::core::db;
use crate::error::Result;

pub fn cmd_snapshot(config: &Config, action: SnapshotAction) -> Result<()> {
    let db_path = config.workflows_dir.join("history.db");
    let conn = db::open_db(&db_path)?;

    match action {
        SnapshotAction::Set { task, key, value } => {
            let val = match value {
                Some(v) => v,
                None => {
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)?;
                    buf.trim_end().to_string()
                }
            };
            db::store_snapshot(&conn, &task, &key, &val)?;
            eprintln!("Snapshot stored: {} / {}", task, key);
        }

        SnapshotAction::Get { task, key } => {
            match db::get_snapshot(&conn, &task, &key)? {
                Some((value, _created)) => print!("{}", value),
                None => std::process::exit(1),
            }
        }

        SnapshotAction::Delete { task, key } => {
            if db::delete_snapshot(&conn, &task, &key)? {
                eprintln!("Snapshot deleted: {} / {}", task, key);
            } else {
                eprintln!("No snapshot found: {} / {}", task, key);
                std::process::exit(1);
            }
        }

        SnapshotAction::List { task, json } => {
            let rows = db::list_snapshots(&conn, task.as_deref())?;
            if json {
                let entries: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|(tr, k, c)| {
                        serde_json::json!({ "task_ref": tr, "key": k, "created": c })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else if rows.is_empty() {
                eprintln!("No snapshots found.");
            } else {
                for (tr, k, c) in &rows {
                    println!("{:<40} {:<20} {}", tr, k, c);
                }
            }
        }
    }

    Ok(())
}
