use crate::core::ai;
use crate::core::compare::{self, build_ai_prompt, format_compare};
use crate::core::config::Config;
use crate::core::db;
use crate::error::{DzError, Result};

/// Normalize task ref: accept both "." and "/" as separator.
fn normalize_task_ref(s: &str) -> String {
    s.replace('.', "/")
}

pub fn cmd_compare(
    config: &Config,
    task: &str,
    run_id: Option<&str>,
    with_id: Option<&str>,
    json: bool,
    use_ai: bool,
) -> Result<()> {
    let task_ref = normalize_task_ref(task);
    let conn = db::open_db(&config.db_path())?;

    let (base, current) = match (run_id, with_id) {
        (Some(rid), Some(wid)) => {
            let current = db::get_run_by_id(&conn, rid)?
                .ok_or_else(|| DzError::Compare(format!("run not found: {}", rid)))?;
            let base = db::get_run_by_id(&conn, wid)?
                .ok_or_else(|| DzError::Compare(format!("run not found: {}", wid)))?;
            (base, current)
        }
        (Some(rid), None) => {
            let current = db::get_run_by_id(&conn, rid)?
                .ok_or_else(|| DzError::Compare(format!("run not found: {}", rid)))?;
            // Find the run just before this one
            let history = db::get_task_history(&conn, &task_ref, 50)?;
            let pos = history.iter().position(|r| r.id == rid);
            let base = match pos {
                Some(p) if p + 1 < history.len() => history[p + 1].clone(),
                _ => return Err(DzError::Compare("no previous run to compare against".to_string())),
            };
            (base, current)
        }
        (None, Some(wid)) => {
            let base = db::get_run_by_id(&conn, wid)?
                .ok_or_else(|| DzError::Compare(format!("run not found: {}", wid)))?;
            let history = db::get_task_history(&conn, &task_ref, 1)?;
            let current = history.into_iter().next()
                .ok_or_else(|| DzError::Compare("no runs found for this task".to_string()))?;
            (base, current)
        }
        (None, None) => {
            let history = db::get_task_history(&conn, &task_ref, 2)?;
            if history.len() < 2 {
                return Err(DzError::Compare(
                    format!("need at least 2 runs to compare (found {})", history.len()),
                ));
            }
            // history is newest-first: [0] = current, [1] = base
            (history[1].clone(), history[0].clone())
        }
    };

    let mut result = compare::compare_runs(&base, &current);

    if use_ai {
        if let Some(tool) = ai::detect_ai_tool() {
            let prompt = build_ai_prompt(&base, &current);
            match ai::invoke_ai_raw(tool, &prompt) {
                Ok(analysis) => result.ai_analysis = Some(analysis),
                Err(msg) => eprintln!("AI analysis failed: {}", msg),
            }
        } else {
            eprintln!("No AI tool found (install `claude` or `codex`)");
        }
    }

    if json {
        let json_str = serde_json::to_string_pretty(&result)?;
        println!("{}", json_str);
    } else {
        print!("{}", format_compare(&result, true));
    }

    Ok(())
}
