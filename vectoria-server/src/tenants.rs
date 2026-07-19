use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::RwLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TenantInfo {
    pub name: String,
    pub api_key: String,
    pub created_at: DateTime<Utc>,
}

/// Persistent tenant registry. Backed by a JSON sidecar file alongside the EdgeStore DB.
/// Manages dynamic tenants created via the API — distinct from static config-file tenants.
pub struct TenantStore {
    path: PathBuf,
    by_key: RwLock<HashMap<String, TenantInfo>>,
    by_name: RwLock<HashMap<String, String>>,
}

impl TenantStore {
    /// Create an empty in-memory store (no persistence). Used as fallback on load error.
    pub fn empty() -> Self {
        Self {
            path: PathBuf::new(),
            by_key: RwLock::new(HashMap::new()),
            by_name: RwLock::new(HashMap::new()),
        }
    }

    pub fn load(path: PathBuf) -> anyhow::Result<Self> {
        let tenants: Vec<TenantInfo> = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&path)?)?
        } else {
            vec![]
        };
        let by_key: HashMap<String, TenantInfo> =
            tenants.iter().map(|t| (t.api_key.clone(), t.clone())).collect();
        let by_name: HashMap<String, String> =
            tenants.iter().map(|t| (t.name.clone(), t.api_key.clone())).collect();
        Ok(Self {
            path,
            by_key: RwLock::new(by_key),
            by_name: RwLock::new(by_name),
        })
    }

    fn save(&self) -> anyhow::Result<()> {
        if self.path.as_os_str().is_empty() { return Ok(()); }
        let mut tenants: Vec<TenantInfo> = self.by_key.read().unwrap().values().cloned().collect();
        tenants.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                create_private_dir(parent)?;
            }
        }
        write_private_file(&self.path, serde_json::to_string_pretty(&tenants)?.as_bytes())?;
        Ok(())
    }

    pub fn lookup_key(&self, api_key: &str) -> Option<String> {
        self.by_key.read().unwrap().get(api_key).map(|t| t.name.clone())
    }

    pub fn list(&self) -> Vec<TenantInfo> {
        let mut out: Vec<TenantInfo> = self.by_key.read().unwrap().values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn exists(&self, name: &str) -> bool {
        self.by_name.read().unwrap().contains_key(name)
    }

    pub fn create(&self, name: &str) -> anyhow::Result<TenantInfo> {
        let api_key = generate_api_key();
        let tenant = TenantInfo { name: name.to_string(), api_key: api_key.clone(), created_at: Utc::now() };
        // Lock order: by_key then by_name — matches rotate_key() to prevent deadlock.
        let mut by_key = self.by_key.write().unwrap();
        let mut by_name = self.by_name.write().unwrap();
        if by_name.contains_key(name) {
            anyhow::bail!("tenant already exists");
        }
        by_key.insert(api_key.clone(), tenant.clone());
        by_name.insert(name.to_string(), api_key.clone());
        drop(by_key);
        drop(by_name);
        if let Err(e) = self.save() {
            self.by_key.write().unwrap().remove(&api_key);
            self.by_name.write().unwrap().remove(name);
            return Err(e);
        }
        Ok(tenant)
    }

    pub fn delete(&self, name: &str) -> anyhow::Result<bool> {
        let key = self.by_name.read().unwrap().get(name).cloned();
        if let Some(k) = key {
            let removed = self.by_key.write().unwrap().remove(&k);
            self.by_name.write().unwrap().remove(name);
            if let Err(e) = self.save() {
                if let Some(tenant) = removed {
                    self.by_key.write().unwrap().insert(k.clone(), tenant);
                    self.by_name.write().unwrap().insert(name.to_string(), k);
                }
                return Err(e);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn rotate_key(&self, name: &str) -> anyhow::Result<Option<TenantInfo>> {
        let old_key = self.by_name.read().unwrap().get(name).cloned();
        let Some(old_key) = old_key else { return Ok(None) };
        let new_key = generate_api_key();
        let mut by_key = self.by_key.write().unwrap();
        let mut by_name = self.by_name.write().unwrap();
        let Some(original) = by_key.remove(&old_key) else { return Ok(None) };
        let mut updated = original.clone();
        updated.api_key = new_key.clone();
        by_key.insert(new_key.clone(), updated.clone());
        by_name.insert(name.to_string(), new_key.clone());
        drop(by_key);
        drop(by_name);
        if let Err(e) = self.save() {
            let mut by_key = self.by_key.write().unwrap();
            let mut by_name = self.by_name.write().unwrap();
            by_key.remove(&new_key);
            by_key.insert(old_key.clone(), original);
            by_name.insert(name.to_string(), old_key);
            return Err(e);
        }
        Ok(Some(updated))
    }
}

fn generate_api_key() -> String {
    format!("vtk_{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
}

// ── Private-file helpers ──────────────────────────────────────────────────────
// tenants.json contains API keys. Always create with 0o600 / 0o700 so that
// other OS users cannot read it even if the process umask is permissive.

#[cfg(unix)]
fn create_private_dir(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir(path: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(unix)]
fn write_private_file(path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true).create(true).truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(data)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_file(path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
    std::fs::write(path, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn create_returns_vtk_key() {
        let s = TenantStore::empty();
        let t = s.create("acme").unwrap();
        assert_eq!(t.name, "acme");
        assert!(t.api_key.starts_with("vtk_"));
    }

    #[test]
    fn create_duplicate_sequential_errors() {
        let s = TenantStore::empty();
        s.create("acme").unwrap();
        assert!(s.create("acme").unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn delete_removes_and_lookup_key_gone() {
        let s = TenantStore::empty();
        let t = s.create("gone").unwrap();
        assert!(s.delete("gone").unwrap());
        assert!(!s.exists("gone"));
        assert!(s.lookup_key(&t.api_key).is_none());
    }

    #[test]
    fn rotate_key_invalidates_old() {
        let s = TenantStore::empty();
        let old = s.create("r").unwrap();
        let new = s.rotate_key("r").unwrap().unwrap();
        assert_ne!(old.api_key, new.api_key);
        assert!(s.lookup_key(&old.api_key).is_none());
        assert_eq!(s.lookup_key(&new.api_key).unwrap(), "r");
    }

    // TOCTOU regression: concurrent creates for the same name must produce exactly one success.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_create_same_name_exactly_one_succeeds() {
        let store = Arc::new(TenantStore::empty());
        let mut set = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let s = Arc::clone(&store);
            set.spawn(async move { s.create("race") });
        }
        let mut ok = 0usize;
        while let Some(r) = set.join_next().await {
            if r.expect("task panicked").is_ok() { ok += 1; }
        }
        assert_eq!(ok, 1, "exactly one concurrent create must succeed; got {ok}");
        assert_eq!(store.list().len(), 1);
    }
}
