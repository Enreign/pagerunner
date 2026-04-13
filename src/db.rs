use crate::error::{PagerunnerError, Result};
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use redb::{Database, ReadableDatabase, TableDefinition};

const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("kv");

pub struct Db {
    inner: Database,
    key: [u8; 32],
}

impl Db {
    pub fn generate_key() -> [u8; 32] {
        let k = Aes256Gcm::generate_key(OsRng);
        k.into()
    }

    pub fn master_key(&self) -> [u8; 32] {
        self.key
    }

    /// Load key from macOS Keychain, generating and storing one on first run.
    ///
    /// If the `PAGERUNNER_DB_KEY` environment variable is set to a 64-char hex
    /// string, that key is used directly (bypasses the Keychain). Intended for
    /// CI and non-macOS environments where the `security` CLI is unavailable.
    pub fn load_or_create_key() -> Result<[u8; 32]> {
        if let Ok(hex) = std::env::var("PAGERUNNER_DB_KEY") {
            let bytes = hex::decode(hex.trim())
                .map_err(|e| PagerunnerError::Config(format!("Bad PAGERUNNER_DB_KEY: {}", e)))?;
            if bytes.len() != 32 {
                return Err(PagerunnerError::Config(
                    "PAGERUNNER_DB_KEY must be 64 hex chars (32 bytes)".into(),
                ));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }

        let read = std::process::Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                "pagerunner",
                "-a",
                "db_key",
                "-w",
            ])
            .output()
            .map_err(|e| PagerunnerError::Config(format!("security CLI error: {}", e)))?;

        if read.status.success() {
            let hex = String::from_utf8_lossy(&read.stdout).trim().to_string();
            let bytes = hex::decode(&hex)
                .map_err(|e| PagerunnerError::Config(format!("Bad key in Keychain: {}", e)))?;
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }

        let key = Self::generate_key();
        let hex_key = hex::encode(key);
        let store = std::process::Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                "pagerunner",
                "-a",
                "db_key",
                "-w",
                &hex_key,
            ])
            .status()
            .map_err(|e| PagerunnerError::Config(format!("security CLI error: {}", e)))?;
        if !store.success() {
            return Err(PagerunnerError::Config(
                "Failed to store DB key in Keychain".into(),
            ));
        }
        Ok(key)
    }

    pub fn open_with_key(path: &str, key: [u8; 32]) -> Result<Self> {
        let inner = Database::create(path)
            .map_err(|e| PagerunnerError::Config(format!("DB open error: {}", e)))?;
        let write = inner
            .begin_write()
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        write
            .open_table(TABLE)
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        write
            .commit()
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        Ok(Self { inner, key })
    }

    pub fn open(path: &str) -> Result<Self> {
        let key = Self::load_or_create_key()?;
        Self::open_with_key(path, key)
    }

    pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let nonce = Aes256Gcm::generate_nonce(OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| PagerunnerError::Config(format!("Encrypt error: {}", e)))?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            return Err(PagerunnerError::Config("Ciphertext too short".into()));
        }
        let (nonce_bytes, ciphertext) = data.split_at(12);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| PagerunnerError::Config(format!("Decrypt error: {}", e)))
    }

    pub fn put(&self, table: &str, record_key: &str, value: &[u8]) -> Result<()> {
        let compound = format!("{}/{}", table, record_key);
        let encrypted = Self::encrypt(&self.key, value)?;
        let write = self
            .inner
            .begin_write()
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        {
            let mut t = write
                .open_table(TABLE)
                .map_err(|e| PagerunnerError::Config(e.to_string()))?;
            t.insert(compound.as_str(), encrypted.as_slice())
                .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        }
        write
            .commit()
            .map_err(|e| PagerunnerError::Config(e.to_string()))
    }

    pub fn get(&self, table: &str, record_key: &str) -> Result<Option<Vec<u8>>> {
        let compound = format!("{}/{}", table, record_key);
        let read = self
            .inner
            .begin_read()
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        let t = read
            .open_table(TABLE)
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        match t
            .get(compound.as_str())
            .map_err(|e| PagerunnerError::Config(e.to_string()))?
        {
            None => Ok(None),
            Some(v) => Self::decrypt(&self.key, v.value()).map(Some),
        }
    }

    /// List all record keys (and their decrypted values) whose compound key starts with
    /// `table/key_prefix`. Returns record keys with the `table/` prefix stripped.
    pub fn scan_prefix(&self, table: &str, key_prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let full_prefix = format!("{}/{}", table, key_prefix);
        let read = self
            .inner
            .begin_read()
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        let t = read
            .open_table(TABLE)
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        let mut results = Vec::new();
        for entry in t
            .range(full_prefix.as_str()..)
            .map_err(|e| PagerunnerError::Config(e.to_string()))?
        {
            let (k, v) = entry.map_err(|e| PagerunnerError::Config(e.to_string()))?;
            let full_key = k.value();
            if !full_key.starts_with(&full_prefix) {
                break;
            }
            let record_key = full_key[table.len() + 1..].to_string();
            let value = Self::decrypt(&self.key, v.value())?;
            results.push((record_key, value));
        }
        Ok(results)
    }

    /// Delete all records in `table` whose record key starts with `prefix`.
    /// Used to purge all vault entries for a session: `delete_prefix("anon", "sess_id:")`.
    /// No-op if no matching keys exist.
    pub fn delete_prefix(&self, table: &str, prefix: &str) -> Result<()> {
        // Collect matching record keys (scan_prefix returns keys with table/ stripped)
        let record_keys: Vec<String> = self
            .scan_prefix(table, prefix)?
            .into_iter()
            .map(|(k, _)| k)
            .collect();

        if record_keys.is_empty() {
            return Ok(());
        }

        // Batch all deletes in a single write transaction
        let write = self
            .inner
            .begin_write()
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        {
            let mut t = write
                .open_table(TABLE)
                .map_err(|e| PagerunnerError::Config(e.to_string()))?;
            for record_key in &record_keys {
                let compound = format!("{}/{}", table, record_key);
                t.remove(compound.as_str())
                    .map_err(|e| PagerunnerError::Config(e.to_string()))?;
            }
        }
        write
            .commit()
            .map_err(|e| PagerunnerError::Config(e.to_string()))
    }

    /// Delete a single record. This is a no-op when the key is absent.
    pub fn delete(&self, table: &str, record_key: &str) -> Result<()> {
        let compound = format!("{}/{}", table, record_key);
        let write = self
            .inner
            .begin_write()
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        {
            let mut t = write
                .open_table(TABLE)
                .map_err(|e| PagerunnerError::Config(e.to_string()))?;
            t.remove(compound.as_str())
                .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        }
        write
            .commit()
            .map_err(|e| PagerunnerError::Config(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = Db::generate_key();
        let plain = b"cookie-data-here";
        let cipher = Db::encrypt(&key, plain).unwrap();
        let decoded = Db::decrypt(&key, &cipher).unwrap();
        assert_eq!(decoded, plain);
    }

    #[test]
    fn test_encrypt_produces_different_ciphertexts() {
        let key = Db::generate_key();
        let a = Db::encrypt(&key, b"same").unwrap();
        let b = Db::encrypt(&key, b"same").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn test_put_get_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let key = Db::generate_key();
        let db = Db::open_with_key(db_path.to_str().unwrap(), key).unwrap();
        db.put("snapshots", "key1", b"value1").unwrap();
        let got = db.get("snapshots", "key1").unwrap();
        assert_eq!(got, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_get_missing_key_returns_none() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();
        assert_eq!(db.get("snapshots", "nonexistent").unwrap(), None);
    }

    #[test]
    fn test_scan_prefix_returns_matching_keys() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();
        db.put("snaps", "alpha/t1", b"v1").unwrap();
        db.put("snaps", "alpha/t2", b"v2").unwrap();
        db.put("snaps", "beta/t1", b"v3").unwrap();
        let hits = db.scan_prefix("snaps", "alpha/").unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|(k, v)| k == "alpha/t1" && v == b"v1"));
    }

    #[test]
    fn test_scan_prefix_empty_when_no_match() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();
        db.put("snaps", "alpha/t1", b"v1").unwrap();
        let hits = db.scan_prefix("snaps", "gamma/").unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn test_delete_nonexistent_is_noop() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();
        // Deleting a key that doesn't exist should succeed silently
        db.delete("snaps", "nonexistent").unwrap();
    }

    #[test]
    fn test_delete_removes_key() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();
        db.put("snaps", "k1", b"v1").unwrap();
        db.delete("snaps", "k1").unwrap();
        assert_eq!(db.get("snaps", "k1").unwrap(), None);
    }

    #[test]
    fn test_delete_prefix_removes_matching_keys() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        db.put("anon", "sess1:tok1", b"val1").unwrap();
        db.put("anon", "sess1:tok2", b"val2").unwrap();
        db.put("anon", "sess2:tok3", b"val3").unwrap();
        db.put("other", "key", b"other").unwrap();

        db.delete_prefix("anon", "sess1:").unwrap();

        // sess1 keys gone
        assert!(db.get("anon", "sess1:tok1").unwrap().is_none());
        assert!(db.get("anon", "sess1:tok2").unwrap().is_none());
        // sess2 and other keys untouched
        assert!(db.get("anon", "sess2:tok3").unwrap().is_some());
        assert!(db.get("other", "key").unwrap().is_some());
    }

    #[test]
    fn test_delete_prefix_empty_prefix_deletes_all_in_table() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        db.put("anon", "a", b"1").unwrap();
        db.put("anon", "b", b"2").unwrap();

        db.delete_prefix("anon", "").unwrap();

        assert!(db.get("anon", "a").unwrap().is_none());
        assert!(db.get("anon", "b").unwrap().is_none());
    }

    #[test]
    fn test_delete_prefix_no_match_is_ok() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        // No keys exist — should not error
        let result = db.delete_prefix("anon", "nonexistent:");
        assert!(result.is_ok());
    }
}
