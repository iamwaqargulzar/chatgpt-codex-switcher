use crate::error::{AppError, R};
use crate::models::{Account, SnapshotMap, UsageSnapshot};
use crate::paths::Paths;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const SERVICE: &str = "codexdesk-vault";
const USER: &str = "master";

#[derive(Serialize, Deserialize)]
struct VaultData {
    version: u32,
    active_account_id: Option<String>,
    accounts: Vec<Account>,
    snapshots: SnapshotMap,
}

impl Default for VaultData {
    fn default() -> Self {
        Self {
            version: 1,
            active_account_id: None,
            accounts: Vec::new(),
            snapshots: SnapshotMap::new(),
        }
    }
}

/// On-disk envelope. With a keyring key the payload is AES-256-GCM ciphertext;
/// otherwise the plain VaultData object is stored and `encrypted()` reports
/// false so the UI can warn.
#[derive(Serialize, Deserialize)]
struct Envelope {
    enc: bool,
    nonce: Option<String>,
    data: String,
}

pub struct Vault {
    paths: Paths,
    data: VaultData,
    key: Option<Vec<u8>>,
}

/// AES-256 master key persisted in the OS keyring. Falls back to `None` when
/// no keyring service is reachable (plaintext file with 0600 permissions).
fn load_or_create_key() -> Option<Vec<u8>> {
    let entry = keyring::Entry::new(SERVICE, USER).ok()?;
    match entry.get_password() {
        Ok(p) => {
            #[cfg(debug_assertions)]
            eprintln!("[vault] keyring key loaded ({} chars)", p.len());
            B64.decode(p).ok()
        }
        Err(keyring::Error::NoEntry) => {
            let mut key = vec![0u8; 32];
            rand::rng().fill_bytes(&mut key);
            #[cfg(debug_assertions)]
            eprintln!("[vault] creating new keyring key");
            if entry.set_password(&B64.encode(&key)).is_ok() {
                Some(key)
            } else {
                eprintln!("[vault] WARNING: could not store the vault key in the keyring");
                None
            }
        }
        Err(e) => {
            eprintln!("[vault] WARNING: keyring unavailable ({e}) — plaintext vault");
            None
        }
    }
}

fn encrypt(key: &[u8], plaintext: &[u8]) -> R<(String, String)> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| AppError::Msg("vault crypto init failed".into()))?;
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|_| AppError::Msg("vault encryption failed".into()))?;
    Ok((B64.encode(nonce_bytes), B64.encode(ct)))
}

fn decrypt(key: &[u8], nonce_b64: &str, ct_b64: &str) -> R<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| AppError::Msg("vault crypto init failed".into()))?;
    let nonce_bytes = B64.decode(nonce_b64).map_err(|_| AppError::Msg("bad nonce".into()))?;
    let ct = B64.decode(ct_b64).map_err(|_| AppError::Msg("bad ciphertext".into()))?;
    cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ct.as_slice())
        .map_err(|_| AppError::Msg("vault decryption failed — keyring key changed?".into()))
}

impl Vault {
    pub fn load(paths: Paths) -> R<Self> {
        let key = load_or_create_key();
        let data = if paths.vault_file.exists() {
            let raw = fs::read_to_string(&paths.vault_file)?;
            let json: serde_json::Value = serde_json::from_str(&raw)?;
            if json.get("enc").and_then(|v| v.as_bool()) == Some(true) {
                let env: Envelope = serde_json::from_value(json)?;
                let key_ref = key.as_ref().ok_or_else(|| {
                    AppError::Msg("vault is encrypted but the OS keyring is unavailable".into())
                })?;
                let plain = decrypt(key_ref, env.nonce.as_deref().unwrap_or(""), &env.data)?;
                serde_json::from_slice(&plain)?
            } else {
                serde_json::from_value(json)?
            }
        } else {
            VaultData::default()
        };
        Ok(Self { paths, data, key })
    }

    pub fn save(&self) -> R<()> {
        let plain = serde_json::to_vec_pretty(&self.data)?;
        let text = match &self.key {
            Some(k) => {
                let (nonce, ct) = encrypt(k, &plain)?;
                serde_json::to_string_pretty(&Envelope {
                    enc: true,
                    nonce: Some(nonce),
                    data: ct,
                })?
            }
            None => String::from_utf8(plain).map_err(|e| AppError::Msg(e.to_string()))?,
        };
        // Atomic replace + rolling backup so a crash mid-save can never
        // corrupt the vault.
        let tmp = self.paths.vault_file.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        set_private(&tmp);
        if self.paths.vault_file.exists() {
            let bak = self.paths.vault_file.with_extension("json.bak");
            let _ = fs::copy(&self.paths.vault_file, &bak);
        }
        fs::rename(&tmp, &self.paths.vault_file)?;
        set_private(&self.paths.vault_file);
        Ok(())
    }

    pub fn encrypted(&self) -> bool {
        self.key.is_some()
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.data.accounts.clone()
    }

    pub fn account(&self, id: &str) -> Option<Account> {
        self.data.accounts.iter().find(|a| a.id == id).cloned()
    }

    pub fn find_by_name(&self, name: &str) -> Option<Account> {
        self.data
            .accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .cloned()
    }

    pub fn upsert(&mut self, account: Account) -> R<()> {
        match self
            .data
            .accounts
            .iter_mut()
            .find(|a| a.id == account.id)
        {
            Some(existing) => *existing = account,
            None => self.data.accounts.push(account),
        }
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> R<()> {
        self.data.accounts.retain(|a| a.id != id);
        if self.data.active_account_id.as_deref() == Some(id) {
            self.data.active_account_id = None;
        }
        self.data.snapshots.remove(id);
        self.save()
    }

    pub fn set_active(&mut self, id: Option<&str>) -> R<()> {
        self.data.active_account_id = id.map(|s| s.to_string());
        self.save()
    }

    pub fn active_id(&self) -> Option<String> {
        self.data.active_account_id.clone()
    }

    pub fn snapshot(&self, id: &str) -> Option<UsageSnapshot> {
        self.data.snapshots.get(id).cloned()
    }

    pub fn snapshots(&self) -> SnapshotMap {
        self.data.snapshots.clone()
    }

    pub fn set_snapshot(&mut self, id: &str, snap: UsageSnapshot) -> R<()> {
        self.data.snapshots.insert(id.to_string(), snap);
        self.save()
    }
}

fn set_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Append one line to a JSONL log, keeping the most recent `max_lines`.
pub fn append_log(path: &Path, entry: &serde_json::Value, max_lines: usize) -> R<()> {
    let line = serde_json::to_string(entry)?;
    let mut lines: Vec<String> = if path.exists() {
        fs::read_to_string(path)?
            .lines()
            .map(|l| l.to_string())
            .filter(|l| !l.trim().is_empty())
            .collect()
    } else {
        Vec::new()
    };
    lines.push(line);
    if lines.len() > max_lines {
        let drop = lines.len() - max_lines;
        lines.drain(..drop);
    }
    fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

/// Read the last `limit` entries of a JSONL log, newest first.
pub fn read_log<T: for<'de> Deserialize<'de>>(path: &Path, limit: usize) -> Vec<T> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out: Vec<T> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    out.reverse();
    out.truncate(limit);
    out
}
