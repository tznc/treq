//! Mobile device SSH key generation and storage.
//!
//! Desktop identifies with the user's existing `~/.ssh` keys (see
//! `remote_local_keys`). Mobile has no such directory and no user-facing
//! concept of one, so the app generates a dedicated ed25519 keypair per
//! device the first time it is needed.
//!
//! Private key storage is platform-specific:
//! - On mobile (Android/iOS), the private key is stored in the OS-native
//!   keystore/keychain via `tauri-plugin-keystore`, gated on the device
//!   having biometrics set up (`tauri-plugin-biometric`). It never leaves
//!   that store in plaintext except into memory here, to feed the SSH
//!   transport (`core::remote_ssh_transport`).
//! - On desktop this module is unused - desktop identifies with local
//!   `~/.ssh` keys instead (see `remote_local_keys`). `tauri-plugin-keystore`
//!   does ship a desktop fallback via the OS keyring, but it is not
//!   production-ready upstream (hardcoded identity, `unwrap()` on every
//!   error) - see its `src/desktop.rs`. Do not add a `#[cfg(not(mobile))]`
//!   path here without first checking whether that has changed upstream.

use russh::keys::ssh_key::private::{Ed25519Keypair, KeypairData};
use russh::keys::ssh_key::{HashAlg, PrivateKey};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceKeyInfo {
  pub public_key: String,
  pub fingerprint_sha256: String,
}

/// Machine-checkable prefix on the `Err(String)` this module returns when
/// secure storage isn't usable on this device (no biometrics enrolled, or
/// the OS keystore itself is unavailable). Callers check for this prefix to show a
/// dedicated "set up biometrics" state instead of a generic connection
/// error. Kept as a string prefix (not a typed Tauri command error) because
/// `ensure_device_key`'s `Result<_, String>` signature is shared with other
/// `remote_*` command error strings that already flow straight to JS as
/// plain messages; changing that return type is out of scope here.
pub const SECURE_STORAGE_UNAVAILABLE_PREFIX: &str = "secure_storage_unavailable:";

#[cfg_attr(
  not(any(mobile, test)),
  expect(dead_code, reason = "only used on mobile builds and in tests")
)]
fn generate_private_key() -> Result<PrivateKey, String> {
  let mut seed = [0u8; 32];
  getrandom::fill(&mut seed).map_err(|e| format!("failed to source randomness: {e}"))?;
  let keypair = Ed25519Keypair::from_seed(&seed);
  PrivateKey::new(KeypairData::Ed25519(keypair), "treq-mobile-device")
    .map_err(|e| format!("failed to generate device key: {e}"))
}

#[cfg_attr(
  not(any(mobile, test)),
  expect(dead_code, reason = "only used on mobile builds and in tests")
)]
fn device_key_info(key: &PrivateKey) -> Result<DeviceKeyInfo, String> {
  let public_key = key.public_key();
  let public_key_line = public_key
    .to_openssh()
    .map_err(|e| format!("failed to encode device public key: {e}"))?;
  let fingerprint_sha256 = public_key.fingerprint(HashAlg::Sha256).to_string();
  Ok(DeviceKeyInfo {
    public_key: public_key_line,
    fingerprint_sha256,
  })
}

#[cfg(mobile)]
mod mobile_storage {
  use super::{generate_private_key, PrivateKey};
  use tauri_plugin_biometric::BiometricExt;
  use tauri_plugin_keystore::{Error as KeystoreError, KeystoreExt, RetrieveRequest, StoreRequest};

  const KEYSTORE_KEY: &str = "com.treq.mobile-device-key/device-key";

  /// Whether a keystore-plugin error genuinely indicates the OS-native
  /// keystore/keychain is unavailable on this device, as opposed to some
  /// unrelated I/O failure. The plugin's `Error` type (see its `error.rs`)
  /// only has two variants: `Io`, for local filesystem/serialization
  /// problems that have nothing to do with device capability, and (on
  /// mobile) `PluginInvoke`, which wraps a rejection from the native
  /// Android/iOS side actually trying to use the platform keystore. Only
  /// the latter should surface as "secure storage unavailable" to the UI.
  fn indicates_keystore_unavailable(err: &KeystoreError) -> bool {
    match err {
      KeystoreError::Io(_) => false,
      #[cfg(mobile)]
      KeystoreError::PluginInvoke(_) => true,
    }
  }

  /// Formats a keystore error for the given `action` (e.g. "read device key
  /// from" or "store device key in"), prefixing it with
  /// [`super::SECURE_STORAGE_UNAVAILABLE_PREFIX`] when the failure indicates
  /// the platform keystore itself is unavailable.
  fn wrap_keystore_err(action: &str, e: KeystoreError) -> String {
    if indicates_keystore_unavailable(&e) {
      format!("{}{action}: {e}", super::SECURE_STORAGE_UNAVAILABLE_PREFIX)
    } else {
      format!("{action}: {e}")
    }
  }

  fn require_biometrics(app: &tauri::AppHandle) -> Result<(), String> {
    // A failure *calling* `status()` is a plugin/transport error (e.g. the
    // native side didn't respond), not evidence that biometrics/secure
    // storage are unavailable - that case is reported via `is_available`
    // below, once the call actually succeeds. Don't prefix it as
    // `SECURE_STORAGE_UNAVAILABLE_PREFIX`, or transient errors unrelated to
    // biometric enrollment would show the "set up biometrics" UI.
    let status = app
      .biometric()
      .status()
      .map_err(|e| format!("failed to read biometric status: {e}"))?;
    if !status.is_available {
      let reason = status.error.unwrap_or_else(|| {
        "Biometrics are not set up on this device; the device key cannot be stored securely."
          .to_string()
      });
      return Err(format!(
        "{}{reason}",
        super::SECURE_STORAGE_UNAVAILABLE_PREFIX
      ));
    }
    Ok(())
  }

  pub async fn load_or_create(app: &tauri::AppHandle) -> Result<PrivateKey, String> {
    require_biometrics(app)?;

    let existing = app
      .keystore()
      .retrieve(RetrieveRequest {
        key: KEYSTORE_KEY.to_string(),
        prompt: None,
      })
      .await
      .map_err(|e| wrap_keystore_err("failed to read device key from keystore", e))?;

    if let Some(openssh) = existing.value {
      return openssh
        .parse::<PrivateKey>()
        .map_err(|e| format!("failed to parse stored device key: {e}"));
    }

    let key = generate_private_key()?;
    let openssh = key
      .to_openssh(Default::default())
      .map_err(|e| format!("failed to encode device key: {e}"))?;
    app
      .keystore()
      .store(StoreRequest {
        key: KEYSTORE_KEY.to_string(),
        value: openssh.to_string(),
        prompt: None,
      })
      .await
      .map_err(|e| wrap_keystore_err("failed to store device key in keystore", e))?;
    Ok(key)
  }
}

/// Loads the device's ed25519 keypair, generating and persisting one on
/// first use. Returns only the public key line and its fingerprint - the
/// private key never leaves this module (and, on mobile, the OS keystore).
///
/// Mobile-only: desktop identifies with local `~/.ssh` keys instead (see
/// `remote_local_keys::list_local_ssh_identities`), so this always errors
/// on desktop rather than shipping the upstream keystore plugin's
/// unfinished desktop fallback (see module docs).
#[cfg(mobile)]
pub async fn ensure_device_key(app: &tauri::AppHandle) -> Result<DeviceKeyInfo, String> {
  let key = mobile_storage::load_or_create(app).await?;
  device_key_info(&key)
}

/// Hands the SSH transport a way to load the device private key when an
/// endpoint authenticates with
/// [`crate::core::remote_ssh_transport::DEVICE_KEYSTORE_KEY_REFERENCE`].
/// The key is read from the keystore per new connection and dropped once
/// authentication finishes; it is never cached in the pool or sent to JS.
#[cfg(mobile)]
pub fn device_key_provider(
  app: tauri::AppHandle,
) -> crate::core::remote_ssh_transport::DeviceKeyProvider {
  std::sync::Arc::new(move || {
    let app = app.clone();
    Box::pin(async move { mobile_storage::load_or_create(&app).await })
  })
}

#[cfg(all(not(mobile), feature = "gui"))]
pub async fn ensure_device_key(_app: &tauri::AppHandle) -> Result<DeviceKeyInfo, String> {
  Err(
    "Device key storage is only available on mobile builds; desktop uses local SSH identities \
     instead (see list_local_ssh_identities)."
      .to_string(),
  )
}

#[cfg(test)]
mod tests {
  // Exercising `ensure_device_key` end to end requires a running mobile
  // keystore/biometric plugin, which is not available in a unit test
  // process - a device build covers that instead. This module keeps the platform-independent key-material
  // helpers covered.
  use super::*;

  #[test]
  fn generates_a_valid_ed25519_key() {
    let key = generate_private_key().unwrap();
    let info = device_key_info(&key).unwrap();
    assert!(info.public_key.starts_with("ssh-ed25519 "));
  }
}
