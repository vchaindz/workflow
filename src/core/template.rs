use std::collections::HashMap;

/// Expand template variables in a string.
/// Supports: {{date}}, {{datetime}}, {{hostname}}, and custom vars.
pub fn expand_template(input: &str, vars: &HashMap<String, String>) -> String {
    let mut result = input.to_string();

    // Built-in variables
    let now = chrono::Utc::now();
    result = result.replace("{{date}}", &now.format("%Y-%m-%d").to_string());
    result = result.replace("{{datetime}}", &now.format("%Y-%m-%d_%H-%M-%S").to_string());

    if let Ok(hostname) = hostname() {
        result = result.replace("{{hostname}}", &hostname);
    }

    // Custom variables
    for (key, value) in vars {
        let pattern = format!("{{{{{key}}}}}");
        result = result.replace(&pattern, value);
    }

    result
}

fn hostname() -> std::io::Result<String> {
    let output = std::process::Command::new("hostname").output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_date() {
        let vars = HashMap::new();
        let result = expand_template("backup-{{date}}.sql", &vars);
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert!(result.contains(&today));
    }

    #[test]
    fn test_expand_custom_vars() {
        let mut vars = HashMap::new();
        vars.insert("env".to_string(), "prod".to_string());
        vars.insert("db".to_string(), "mydb".to_string());

        let result = expand_template("deploy to {{env}} db={{db}}", &vars);
        assert_eq!(result, "deploy to prod db=mydb");
    }

    #[test]
    fn test_no_vars_passthrough() {
        let vars = HashMap::new();
        let result = expand_template("plain text", &vars);
        assert_eq!(result, "plain text");
    }
}
