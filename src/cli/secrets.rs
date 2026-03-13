use std::path::PathBuf;

use crate::core::config::Config;
use crate::core::secrets::{self, SecretsStore};
use crate::error::{DzError, Result};

pub fn cmd_secrets_init(config: &mut Config, ssh_key: Option<&str>) -> Result<()> {
    let (private, public) = if let Some(key_path) = ssh_key {
        let private = PathBuf::from(key_path);
        let public = secrets::pubkey_path_from(&private);
        if !private.exists() {
            return Err(DzError::Config(format!("SSH key not found: {}", private.display())));
        }
        if !public.exists() {
            return Err(DzError::Config(format!("SSH public key not found: {}", public.display())));
        }
        (private, public)
    } else {
        eprintln!("auto-detecting SSH key...");
        let (priv_path, pub_path) = secrets::detect_ssh_key()?;
        eprintln!("found: {}", priv_path.display());
        (priv_path, pub_path)
    };

    secrets::init_store(&config.workflows_dir, &public)?;

    // Save SSH key path to config
    config.secrets_ssh_key = Some(private.to_string_lossy().to_string());
    config.save_bookmarks()?; // re-serializes full config

    eprintln!("secrets store initialized: {}/secrets.age", config.workflows_dir.display());
    Ok(())
}

pub fn cmd_secrets_set(config: &Config, name: &str, value: Option<&str>) -> Result<()> {
    let ssh_key = config.secrets_ssh_key.as_deref().ok_or_else(|| {
        DzError::Config("secrets not initialized — run `workflow secrets init` first".into())
    })?;
    let private = PathBuf::from(ssh_key);
    let public = secrets::pubkey_path_from(&private);

    let secret_value = if let Some(v) = value {
        v.to_string()
    } else {
        rpassword::prompt_password_stderr(&format!("enter value for {name}: "))
            .map_err(|e| DzError::Config(format!("failed to read secret value: {e}")))?
    };

    let mut store = SecretsStore::load(&config.workflows_dir, &private)?;
    store.set(name.to_string(), secret_value);
    store.save(&config.workflows_dir, &public)?;

    eprintln!("secret '{name}' saved");
    Ok(())
}

pub fn cmd_secrets_get(config: &Config, name: &str) -> Result<()> {
    let ssh_key = config.secrets_ssh_key.as_deref().ok_or_else(|| {
        DzError::Config("secrets not initialized — run `workflow secrets init` first".into())
    })?;
    let private = PathBuf::from(ssh_key);

    let store = SecretsStore::load(&config.workflows_dir, &private)?;
    match store.get(name) {
        Some(val) => {
            // Print to stdout (not stderr) so it can be piped
            println!("{val}");
            Ok(())
        }
        None => Err(DzError::Config(format!("secret '{name}' not found"))),
    }
}

pub fn cmd_secrets_list(config: &Config) -> Result<()> {
    let ssh_key = config.secrets_ssh_key.as_deref().ok_or_else(|| {
        DzError::Config("secrets not initialized — run `workflow secrets init` first".into())
    })?;
    let private = PathBuf::from(ssh_key);

    let store = SecretsStore::load(&config.workflows_dir, &private)?;
    let names = store.list();
    if names.is_empty() {
        eprintln!("no secrets stored");
    } else {
        for name in &names {
            println!("{name}");
        }
    }
    Ok(())
}

pub fn cmd_secrets_rm(config: &Config, name: &str) -> Result<()> {
    let ssh_key = config.secrets_ssh_key.as_deref().ok_or_else(|| {
        DzError::Config("secrets not initialized — run `workflow secrets init` first".into())
    })?;
    let private = PathBuf::from(ssh_key);
    let public = secrets::pubkey_path_from(&private);

    let mut store = SecretsStore::load(&config.workflows_dir, &private)?;
    if store.remove(name) {
        store.save(&config.workflows_dir, &public)?;
        eprintln!("secret '{name}' removed");
    } else {
        eprintln!("secret '{name}' not found");
    }
    Ok(())
}
