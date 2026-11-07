//! Where the device's secrets live: the store passphrase (it unlocks the encryption keys) and
//! the session (its access token).
//!
//! [`SecretStore::Keychain`] uses the OS credential store — macOS Keychain, Windows Credential
//! Manager, the Secret Service on Linux — and falls back to private files (mode 0600 in the
//! account's data folder) where none is available, saying so. Files left by an earlier version
//! move into the keychain the first time they are read. [`SecretStore::Files`] keeps them in
//! files (tests, headless machines).

use std::path::{Path, PathBuf};

/// Service name for keychain entries.
const SERVICE: &str = "ChatCraft";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretStore {
    Keychain,
    Files,
}

impl SecretStore {
    /// The keychain where the platform has one; `CHATCRAFT_SECRETS=files` forces files.
    pub fn default_for_platform() -> SecretStore {
        if std::env::var("CHATCRAFT_SECRETS").is_ok_and(|v| v.eq_ignore_ascii_case("files")) {
            return SecretStore::Files;
        }
        if cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux")) { SecretStore::Keychain } else { SecretStore::Files }
    }
}

pub(crate) struct Secrets {
    store: SecretStore,
    dir: PathBuf,
    /// The keychain account: `user@homeserver`.
    account: String,
    /// Set when the keychain failed and files were used instead.
    pub fallback: Option<String>,
}

pub(crate) fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

impl Secrets {
    pub fn new(store: SecretStore, dir: &Path, account: &str) -> Secrets {
        Secrets { store, dir: dir.to_path_buf(), account: account.to_string(), fallback: None }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(match name {
            "session" => "session.json",
            other => other,
        })
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn entry(&self, name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, &format!("{}/{name}", self.account)).map_err(|e| e.to_string())
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn keychain_get(&self, name: &str) -> Result<Option<String>, String> {
        match self.entry(name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn keychain_set(&self, name: &str, value: &str) -> Result<(), String> {
        self.entry(name)?.set_password(value).map_err(|e| e.to_string())
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn keychain_delete(&self, name: &str) -> Result<(), String> {
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn keychain_get(&self, _: &str) -> Result<Option<String>, String> {
        Err("no OS keychain on this platform".into())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn keychain_set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no OS keychain on this platform".into())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn keychain_delete(&self, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn file_get(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.file(name)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    /// Reads a secret. A file left from before moves into the keychain.
    pub fn get(&mut self, name: &str) -> Option<String> {
        if self.store == SecretStore::Files {
            return self.file_get(name);
        }
        match self.keychain_get(name) {
            Ok(Some(v)) => Some(v),
            Ok(None) => {
                let v = self.file_get(name)?;
                if self.keychain_set(name, &v).is_ok() {
                    let _ = std::fs::remove_file(self.file(name));
                }
                Some(v)
            }
            Err(e) => {
                self.fallback = Some(e);
                self.file_get(name)
            }
        }
    }

    /// Stores a secret (keychain, or a private file when the keychain fails).
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        if self.store == SecretStore::Keychain {
            match self.keychain_set(name, value) {
                Ok(()) => {
                    let _ = std::fs::remove_file(self.file(name));
                    return Ok(());
                }
                Err(e) => self.fallback = Some(e),
            }
        }
        write_private(&self.file(name), value)
    }

    /// Forgets a secret everywhere it might be.
    pub fn delete(&mut self, name: &str) {
        if self.store == SecretStore::Keychain {
            let _ = self.keychain_delete(name);
        }
        let _ = std::fs::remove_file(self.file(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_store_round_trips_and_deletes() {
        let dir = std::env::temp_dir().join(format!("chatcraft-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Secrets::new(SecretStore::Files, &dir, "t@test");
        assert_eq!(s.get("session"), None);
        s.set("session", "{\"a\":1}").unwrap();
        assert_eq!(s.get("session").as_deref(), Some("{\"a\":1}"));
        assert!(dir.join("session.json").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("session.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        s.delete("session");
        assert_eq!(s.get("session"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Touches the real OS keychain: run by hand with `cargo test -p chatcraft-matrix -- --ignored keychain`.
#[cfg(all(test, any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod keychain_tests {
    use super::*;

    #[test]
    #[ignore = "writes to the OS keychain"]
    fn keychain_round_trip_and_migration() {
        let dir = std::env::temp_dir().join(format!("chatcraft-keychain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let account = format!("keychain-test-{}@localhost", std::process::id());
        // A file from an earlier version moves into the keychain.
        write_private(&dir.join("passphrase"), "from-a-file").unwrap();
        let mut s = Secrets::new(SecretStore::Keychain, &dir, &account);
        assert_eq!(s.get("passphrase").as_deref(), Some("from-a-file"));
        assert!(s.fallback.is_none(), "keychain error: {:?}", s.fallback);
        assert!(!dir.join("passphrase").exists(), "the file is gone once migrated");
        assert_eq!(s.keychain_get("passphrase").unwrap().as_deref(), Some("from-a-file"));
        s.set("session", "{\"token\":\"x\"}").unwrap();
        assert!(!dir.join("session.json").exists());
        assert_eq!(s.get("session").as_deref(), Some("{\"token\":\"x\"}"));
        s.delete("passphrase");
        s.delete("session");
        assert_eq!(s.keychain_get("passphrase").unwrap(), None);
        assert_eq!(s.keychain_get("session").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
