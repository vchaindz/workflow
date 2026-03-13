use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use zeroize::Zeroize;

use crate::error::{DzError, Result};

/// In-memory decrypted secrets store. Values are zeroized on drop.
pub struct SecretsStore {
    secrets: HashMap<String, String>,
}

impl Drop for SecretsStore {
    fn drop(&mut self) {
        for val in self.secrets.values_mut() {
            val.zeroize();
        }
    }
}

impl SecretsStore {
    fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }

    pub fn set(&mut self, name: String, value: String) {
        self.secrets.insert(name, value);
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.secrets.get(name).map(|s| s.as_str())
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.secrets.remove(name).is_some()
    }

    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.secrets.keys().cloned().collect();
        names.sort();
        names
    }

    /// Load and decrypt secrets from the age-encrypted store.
    pub fn load(config_dir: &Path, identity_path: &Path) -> Result<Self> {
        let store_path = config_dir.join("secrets.age");
        if !store_path.exists() {
            return Err(DzError::Config(
                "secrets store not initialized — run `workflow secrets init` first".into(),
            ));
        }

        let identity_str = fs::read_to_string(identity_path).map_err(|e| {
            DzError::Config(format!("failed to read SSH key {}: {e}", identity_path.display()))
        })?;

        let identity = age::ssh::Identity::from_buffer(identity_str.as_bytes(), None)
            .map_err(|e| DzError::Config(format!("failed to parse SSH identity: {e}")))?;

        let encrypted = fs::read(&store_path).map_err(|e| {
            DzError::Config(format!("failed to read secrets store: {e}"))
        })?;

        let decryptor = age::Decryptor::new(&encrypted[..])
            .map_err(|e| DzError::Config(format!("failed to parse secrets store: {e}")))?;

        let mut decrypted = Vec::new();
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .map_err(|e| DzError::Config(format!("failed to decrypt secrets: {e}")))?;
        reader.read_to_end(&mut decrypted).map_err(|e| {
            DzError::Config(format!("failed to read decrypted secrets: {e}"))
        })?;

        let mut json_str = String::from_utf8(decrypted)
            .map_err(|e| DzError::Config(format!("secrets store is not valid UTF-8: {e}")))?;

        let wrapper: SecretsWrapper = serde_json::from_str(&json_str)
            .map_err(|e| DzError::Config(format!("failed to parse secrets JSON: {e}")))?;

        json_str.zeroize();

        Ok(Self {
            secrets: wrapper.secrets,
        })
    }

    /// Encrypt and save the store to disk.
    pub fn save(&self, config_dir: &Path, pubkey_path: &Path) -> Result<()> {
        let pubkey_str = fs::read_to_string(pubkey_path).map_err(|e| {
            DzError::Config(format!("failed to read SSH public key {}: {e}", pubkey_path.display()))
        })?;

        let recipient = age::ssh::Recipient::from_str(pubkey_str.trim())
            .map_err(|e| DzError::Config(format!("failed to parse SSH public key: {e:?}")))?;

        let wrapper = SecretsWrapper {
            secrets: self.secrets.clone(),
        };
        let mut json = serde_json::to_string_pretty(&wrapper)
            .map_err(|e| DzError::Config(format!("failed to serialize secrets: {e}")))?;

        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(recipient)];
        let encryptor = age::Encryptor::with_recipients(recipients.iter().map(|r| r.as_ref() as &dyn age::Recipient))
            .expect("at least one recipient");

        let mut encrypted = Vec::new();
        let mut writer = encryptor
            .wrap_output(&mut encrypted)
            .map_err(|e| DzError::Config(format!("failed to create age encryptor: {e}")))?;
        writer.write_all(json.as_bytes()).map_err(|e| {
            DzError::Config(format!("failed to write encrypted secrets: {e}"))
        })?;
        writer.finish().map_err(|e| {
            DzError::Config(format!("failed to finalize encrypted secrets: {e}"))
        })?;

        json.zeroize();

        let store_path = config_dir.join("secrets.age");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&store_path)
            .map_err(|e| DzError::Config(format!("failed to write secrets store: {e}")))?;
        file.write_all(&encrypted)?;

        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SecretsWrapper {
    secrets: HashMap<String, String>,
}

/// Auto-detect an SSH keypair. Returns (private_key_path, public_key_path).
pub fn detect_ssh_key() -> Result<(PathBuf, PathBuf)> {
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| DzError::Config("cannot determine home directory".into()))?
        .join(".ssh");

    for name in &["id_ed25519", "id_ecdsa", "id_rsa"] {
        let private = ssh_dir.join(name);
        let public = ssh_dir.join(format!("{name}.pub"));
        if private.exists() && public.exists() {
            return Ok((private, public));
        }
    }

    Err(DzError::Config(
        "no SSH keypair found in ~/.ssh — specify with --ssh-key".into(),
    ))
}

/// Initialize an empty secrets store.
pub fn init_store(config_dir: &Path, pubkey_path: &Path) -> Result<()> {
    let store_path = config_dir.join("secrets.age");
    if store_path.exists() {
        return Err(DzError::Config(
            "secrets store already exists — delete it first to reinitialize".into(),
        ));
    }

    fs::create_dir_all(config_dir)?;

    let store = SecretsStore::new();
    store.save(config_dir, pubkey_path)?;

    Ok(())
}

/// Derive the public key path from a private key path.
pub fn pubkey_path_from(private: &Path) -> PathBuf {
    let mut pub_path = private.as_os_str().to_owned();
    pub_path.push(".pub");
    PathBuf::from(pub_path)
}
