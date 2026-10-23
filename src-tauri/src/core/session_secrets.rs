//! Keeps the Supabase session in the OS keychain (macOS Keychain, Windows
//! Credential Manager, the Secret Service on Linux) instead of the app
//! database.
//!
//! The database stays as the fallback: a machine without a keychain (a Linux
//! box with no Secret Service running) keeps working, and a session saved
//! there by an older build moves into the keychain the first time it loads.

use super::auth::SessionStore;

/// One secret in the OS keychain.
pub trait Secret: Send + Sync {
  fn get(&self) -> Result<Option<String>, String>;
  fn set(&self, value: &str) -> Result<(), String>;
  fn delete(&self) -> Result<(), String>;
}

/// A [`SessionStore`] that prefers the keychain and falls back to `fallback`.
pub struct KeychainSessionStore<S, F> {
  secret: S,
  fallback: F,
}

impl<S: Secret, F: SessionStore> KeychainSessionStore<S, F> {
  pub fn new(secret: S, fallback: F) -> Self {
    Self { secret, fallback }
  }

  /// Clears the fallback copy, so the token lives in one place only.
  fn clear_fallback(&self) {
    if self.fallback.load().is_some() {
      self.fallback.save("");
    }
  }
}

impl<S: Secret, F: SessionStore> SessionStore for KeychainSessionStore<S, F> {
  fn load(&self) -> Option<String> {
    match self.secret.get() {
      Ok(Some(value)) if !value.is_empty() => Some(value),
      Ok(_) => {
        let legacy = self.fallback.load()?;
        match self.secret.set(&legacy) {
          Ok(()) => self.fallback.save(""),
          Err(error) => tracing::warn!("moving the session into the keychain: {error}"),
        }
        Some(legacy)
      }
      Err(error) => {
        tracing::warn!("reading the session from the keychain: {error}");
        self.fallback.load()
      }
    }
  }

  fn save(&self, value: &str) {
    if value.is_empty() {
      if let Err(error) = self.secret.delete() {
        tracing::warn!("deleting the session from the keychain: {error}");
      }
      self.clear_fallback();
      return;
    }
    match self.secret.set(value) {
      Ok(()) => self.clear_fallback(),
      Err(error) => {
        tracing::warn!("saving the session to the keychain: {error}");
        self.fallback.save(value);
      }
    }
  }
}

/// The OS keychain entry the desktop app keeps its session under.
#[cfg(all(feature = "gui", desktop))]
pub struct KeyringSecret(keyring::Entry);

#[cfg(all(feature = "gui", desktop))]
impl KeyringSecret {
  pub fn new(service: &str, account: &str) -> Result<Self, String> {
    keyring::Entry::new(service, account)
      .map(Self)
      .map_err(|e| e.to_string())
  }
}

#[cfg(all(feature = "gui", desktop))]
impl Secret for KeyringSecret {
  fn get(&self) -> Result<Option<String>, String> {
    match self.0.get_password() {
      Ok(value) => Ok(Some(value)),
      Err(keyring::Error::NoEntry) => Ok(None),
      Err(error) => Err(error.to_string()),
    }
  }

  fn set(&self, value: &str) -> Result<(), String> {
    self.0.set_password(value).map_err(|e| e.to_string())
  }

  fn delete(&self) -> Result<(), String> {
    match self.0.delete_credential() {
      Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
      Err(error) => Err(error.to_string()),
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use super::*;
  use crate::core::supabase::test_support::MemoryStore;

  /// A keychain in memory; `broken` makes every call fail, as on a machine
  /// without one.
  #[derive(Default)]
  struct MemorySecret {
    value: Mutex<Option<String>>,
    broken: bool,
  }

  impl Secret for Arc<MemorySecret> {
    fn get(&self) -> Result<Option<String>, String> {
      if self.broken {
        return Err("no keychain".into());
      }
      Ok(self.value.lock().unwrap().clone())
    }
    fn set(&self, value: &str) -> Result<(), String> {
      if self.broken {
        return Err("no keychain".into());
      }
      *self.value.lock().unwrap() = Some(value.to_string());
      Ok(())
    }
    fn delete(&self) -> Result<(), String> {
      if self.broken {
        return Err("no keychain".into());
      }
      *self.value.lock().unwrap() = None;
      Ok(())
    }
  }

  impl SessionStore for Arc<MemoryStore> {
    fn load(&self) -> Option<String> {
      self.as_ref().load()
    }
    fn save(&self, value: &str) {
      self.as_ref().save(value)
    }
  }

  type TestStore = KeychainSessionStore<Arc<MemorySecret>, Arc<MemoryStore>>;

  fn store(broken: bool) -> (TestStore, Arc<MemorySecret>, Arc<MemoryStore>) {
    let secret = Arc::new(MemorySecret {
      broken,
      ..Default::default()
    });
    let db = Arc::new(MemoryStore::default());
    (
      KeychainSessionStore::new(secret.clone(), db.clone()),
      secret,
      db,
    )
  }

  #[test]
  fn saves_to_the_keychain_and_not_the_database() {
    let (store, secret, db) = store(false);
    store.save("tokens");
    assert_eq!(secret.value.lock().unwrap().as_deref(), Some("tokens"));
    assert_eq!(db.load(), None);
    assert_eq!(store.load().as_deref(), Some("tokens"));
  }

  #[test]
  fn moves_a_database_session_into_the_keychain_on_load() {
    let (store, secret, db) = store(false);
    db.save("legacy");
    assert_eq!(store.load().as_deref(), Some("legacy"));
    assert_eq!(secret.value.lock().unwrap().as_deref(), Some("legacy"));
    assert_eq!(db.load(), None);
  }

  #[test]
  fn falls_back_to_the_database_without_a_keychain() {
    let (store, _, db) = store(true);
    store.save("tokens");
    assert_eq!(db.load().as_deref(), Some("tokens"));
    assert_eq!(store.load().as_deref(), Some("tokens"));
  }

  #[test]
  fn sign_out_clears_both_copies() {
    let (store, secret, db) = store(false);
    store.save("tokens");
    db.save("stale");
    store.save("");
    assert_eq!(*secret.value.lock().unwrap(), None);
    assert_eq!(db.load(), None);
    assert_eq!(store.load(), None);
  }
}
