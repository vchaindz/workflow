use std::collections::HashMap;

/// Replace secret values with [REDACTED] in text.
pub fn mask_secrets(text: &str, secret_values: &[String]) -> String {
    let mut result = text.to_string();
    for secret in secret_values {
        if !secret.is_empty() {
            result = result.replace(secret, "[REDACTED]");
        }
    }
    result
}

/// Load secret values from the encrypted secrets store.
///
/// Returns a map of secret names to values, suitable for passing to `send_notifications`
/// so that notification URL templates (e.g. `mattermost://$WEBHOOK_URL`) can be expanded.
pub fn load_secret_env(
    secret_names: &[String],
    workflows_dir: &std::path::Path,
    secrets_ssh_key: Option<&std::path::Path>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if secret_names.is_empty() {
        return env;
    }
    if let Some(ssh_key) = secrets_ssh_key {
        if workflows_dir.join("secrets.age").exists() {
            if let Ok(store) = crate::core::secrets::SecretsStore::load(workflows_dir, ssh_key) {
                for name in secret_names {
                    if let Some(val) = store.get(name) {
                        env.insert(name.clone(), val.to_string());
                    }
                }
            }
        }
    }
    env
}

/// Load secret values from the encrypted secrets store, returning an error for any missing secret.
///
/// Unlike `load_secret_env` which silently ignores missing secrets, this function ensures
/// all requested secrets are present and returns a descriptive error if any are missing.
pub fn load_secret_env_strict(
    secret_names: &[String],
    workflows_dir: &std::path::Path,
    secrets_ssh_key: Option<&std::path::Path>,
) -> crate::error::Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    if secret_names.is_empty() {
        return Ok(env);
    }
    let ssh_key = secrets_ssh_key.ok_or_else(|| {
        crate::error::DzError::Config(format!(
            "secrets requested ({}) but no SSH key configured — set secrets_ssh_key in config.toml or use --ssh-key",
            secret_names.join(", ")
        ))
    })?;
    let store_path = workflows_dir.join("secrets.age");
    if !store_path.exists() {
        return Err(crate::error::DzError::Config(format!(
            "secrets requested ({}) but secrets store not found — run `workflow secrets init` first",
            secret_names.join(", ")
        )));
    }
    let store = crate::core::secrets::SecretsStore::load(workflows_dir, ssh_key)?;
    for name in secret_names {
        match store.get(name) {
            Some(val) => { env.insert(name.clone(), val.to_string()); }
            None => {
                return Err(crate::error::DzError::Config(format!(
                    "secret '{}' not found in secrets store — add it with `workflow secrets set {}`",
                    name, name
                )));
            }
        }
    }
    Ok(env)
}
