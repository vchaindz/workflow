use crate::core::ai;
use crate::core::config::Config;
use crate::core::discovery::{resolve_task_ref, scan_workflows};
use crate::core::parser::parse_workflow;
use crate::core::wizard;
use crate::error::{DzError, Result};

pub fn cmd_ai_update(
    config: &Config,
    task_ref: &str,
    prompt: &str,
    dry_run: bool,
    save_as: Option<&str>,
) -> Result<()> {
    let tool = ai::detect_ai_tool().ok_or_else(|| {
        DzError::Execution("No AI tool found. Install `claude`, `codex`, or `gemini`.".to_string())
    })?;

    let categories = scan_workflows(&config.workflows_dir)?;
    let task = resolve_task_ref(&categories, task_ref)?;
    let task_path = task.path.clone();

    let yaml = std::fs::read_to_string(&task_path)?;

    eprintln!("Using {} to update {}", tool.name(), task_ref);
    eprintln!("Prompt: {}", prompt);

    let result = ai::invoke_ai_update(tool, &yaml, prompt);

    let updated_yaml = match result {
        ai::AiResult::Yaml(y) => y,
        ai::AiResult::Error(msg) => {
            return Err(DzError::Execution(format!("AI update failed: {}", msg)));
        }
        ai::AiResult::Success(_) => {
            return Err(DzError::Execution("Unexpected AI response type".to_string()));
        }
    };

    // Validate the generated YAML by parsing it through a temp file
    let tmp_dir = std::env::temp_dir().join("workflow-ai-validate");
    std::fs::create_dir_all(&tmp_dir)?;
    let tmp_path = tmp_dir.join("validate.yaml");
    std::fs::write(&tmp_path, &updated_yaml)?;
    let validation = parse_workflow(&tmp_path);
    let _ = std::fs::remove_file(&tmp_path);
    let _ = std::fs::remove_dir(&tmp_dir);
    validation.map_err(|e| {
        DzError::Execution(format!("AI generated invalid YAML: {}", e))
    })?;

    if dry_run {
        println!("{}", updated_yaml);
        return Ok(());
    }

    if let Some(new_name) = save_as {
        // Parse category from task ref
        let category = task_ref
            .replace('.', "/").split('/')
            .next()
            .unwrap_or("_default")
            .to_string();

        let path = wizard::save_task(&config.workflows_dir, &category, new_name, &updated_yaml)?;
        eprintln!("Saved as: {}", path.display());
    } else {
        std::fs::write(&task_path, &updated_yaml)?;
        eprintln!("Updated: {}", task_path.display());
    }

    Ok(())
}
