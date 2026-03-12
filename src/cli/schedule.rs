use std::path::PathBuf;

use crate::core::config::Config;
use crate::core::discovery::{resolve_task_ref, scan_workflows};
use crate::error::{DzError, Result};

pub fn cmd_schedule(
    config: &Config,
    task_ref: &str,
    cron_expr: Option<&str>,
    systemd: bool,
    remove: bool,
) -> Result<()> {
    // Validate task exists
    let categories = scan_workflows(&config.workflows_dir)?;
    let task = resolve_task_ref(&categories, task_ref)?;
    let canonical_ref = format!("{}/{}", task.category, task.name);

    let workflow_exe = std::env::current_exe()
        .map_err(|e| DzError::Execution(format!("cannot find executable: {e}")))?;
    let exe_str = workflow_exe.display().to_string();

    let dir_flag = if config.workflows_dir != crate::core::config::Config::default().workflows_dir {
        format!(" --dir {}", config.workflows_dir.display())
    } else {
        String::new()
    };

    if remove {
        if systemd {
            return remove_systemd_timer(&canonical_ref);
        } else {
            return remove_crontab_entry(&canonical_ref);
        }
    }

    if systemd {
        let cron = cron_expr.unwrap_or("*-*-* 02:00:00"); // default daily 2am
        install_systemd_timer(&canonical_ref, &exe_str, &dir_flag, cron)
    } else {
        let cron = cron_expr.ok_or_else(|| {
            DzError::Execution("--cron expression required (e.g. \"0 2 * * *\")".to_string())
        })?;
        install_crontab_entry(&canonical_ref, &exe_str, &dir_flag, cron)
    }
}

fn install_crontab_entry(task_ref: &str, exe: &str, dir_flag: &str, cron_expr: &str) -> Result<()> {
    let marker = format!("# workflow:{}", task_ref);
    let entry = format!("{} {}{} run {} --no-tui", cron_expr, exe, dir_flag, task_ref);

    // Read existing crontab
    let existing = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    // Remove old entry for same task if present
    let filtered: Vec<&str> = existing
        .lines()
        .filter(|l| !l.contains(&marker) && !l.ends_with(&format!("run {}", task_ref)))
        .collect();

    let mut new_crontab = filtered.join("\n");
    if !new_crontab.is_empty() && !new_crontab.ends_with('\n') {
        new_crontab.push('\n');
    }
    new_crontab.push_str(&format!("{}\n{}\n", marker, entry));

    // Install via pipe to crontab -
    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| DzError::Execution(format!("failed to run crontab: {e}")))?;

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(new_crontab.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(DzError::Execution("crontab installation failed".to_string()));
    }

    eprintln!("Installed crontab entry for {}", task_ref);
    eprintln!("  {}", entry);
    Ok(())
}

fn remove_crontab_entry(task_ref: &str) -> Result<()> {
    let marker = format!("# workflow:{}", task_ref);

    let existing = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let filtered: Vec<&str> = existing
        .lines()
        .filter(|l| !l.contains(&marker) && !l.ends_with(&format!("run {}", task_ref)))
        .collect();

    let new_crontab = format!("{}\n", filtered.join("\n"));

    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| DzError::Execution(format!("failed to run crontab: {e}")))?;

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(new_crontab.as_bytes())?;
    }
    child.wait()?;

    eprintln!("Removed crontab entry for {}", task_ref);
    Ok(())
}

fn install_systemd_timer(task_ref: &str, exe: &str, dir_flag: &str, on_calendar: &str) -> Result<()> {
    let unit_name = format!("workflow-{}", task_ref.replace('/', "-"));
    let user_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("systemd/user");
    std::fs::create_dir_all(&user_dir)?;

    let service_path = user_dir.join(format!("{}.service", unit_name));
    let timer_path = user_dir.join(format!("{}.timer", unit_name));

    let service_content = format!(
        "[Unit]\nDescription=workflow: {task_ref}\n\n[Service]\nType=oneshot\nExecStart={exe}{dir_flag} run {task_ref} --no-tui\n"
    );

    let timer_content = format!(
        "[Unit]\nDescription=workflow timer: {task_ref}\n\n[Timer]\nOnCalendar={on_calendar}\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
    );

    std::fs::write(&service_path, &service_content)?;
    std::fs::write(&timer_path, &timer_content)?;

    // Reload and enable
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", &format!("{}.timer", unit_name)])
        .status()
        .map_err(|e| DzError::Execution(format!("systemctl failed: {e}")))?;

    if !status.success() {
        return Err(DzError::Execution("failed to enable systemd timer".to_string()));
    }

    eprintln!("Installed systemd timer for {}", task_ref);
    eprintln!("  Service: {}", service_path.display());
    eprintln!("  Timer:   {}", timer_path.display());
    eprintln!("  Schedule: {}", on_calendar);
    Ok(())
}

fn remove_systemd_timer(task_ref: &str) -> Result<()> {
    let unit_name = format!("workflow-{}", task_ref.replace('/', "-"));

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", &format!("{}.timer", unit_name)])
        .status();

    let user_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("systemd/user");

    let _ = std::fs::remove_file(user_dir.join(format!("{}.service", unit_name)));
    let _ = std::fs::remove_file(user_dir.join(format!("{}.timer", unit_name)));

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    eprintln!("Removed systemd timer for {}", task_ref);
    Ok(())
}
