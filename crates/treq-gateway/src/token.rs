//! The `x-treq-gateway-token` header: `v1.<placement_id>.<expiry>.<mac>`.
//!
//! `mac` is lowercase hex HMAC-SHA256 over `v1.<placement_id>.<expiry>`, keyed
//! with the placement's gateway secret (the bytes of the secret file, trailing
//! whitespace trimmed). `expiry` is Unix seconds. The Edge Function side mints
//! the same format in `supabase/functions/_shared/remote/gateway-token.ts`;
//! both suites check the same fixed vectors.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Longest lifetime a token may claim, so a leaked token is short-lived.
pub const MAX_TTL_SECS: u64 = 120;

#[derive(Debug, PartialEq, Eq)]
pub enum TokenError {
  Malformed,
  WrongPlacement,
  Expired,
  TooFarInFuture,
  BadSignature,
}

fn mac_hex(secret: &[u8], placement_id: &str, expiry: u64) -> String {
  let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC takes any key length");
  mac.update(format!("v1.{placement_id}.{expiry}").as_bytes());
  hex::encode(mac.finalize().into_bytes())
}

/// Mints a token. The gateway only verifies; minting is here for tests and parity.
pub fn mint(secret: &[u8], placement_id: &str, expiry: u64) -> String {
  format!(
    "v1.{placement_id}.{expiry}.{}",
    mac_hex(secret, placement_id, expiry)
  )
}

/// Checks a token for this gateway's placement at time `now` (Unix seconds).
pub fn verify(
  secret: &[u8],
  placement_id: &str,
  token: &str,
  now: u64,
  max_ttl: u64,
) -> Result<(), TokenError> {
  let parts: Vec<&str> = token.split('.').collect();
  let [version, placement, expiry, mac] = parts[..] else {
    return Err(TokenError::Malformed);
  };
  if version != "v1" || mac.len() != 64 {
    return Err(TokenError::Malformed);
  }
  let expiry: u64 = expiry.parse().map_err(|_| TokenError::Malformed)?;
  let expected = mac_hex(secret, placement, expiry);
  if !bool::from(expected.as_bytes().ct_eq(mac.as_bytes())) {
    return Err(TokenError::BadSignature);
  }
  if !bool::from(placement.as_bytes().ct_eq(placement_id.as_bytes())) {
    return Err(TokenError::WrongPlacement);
  }
  if expiry < now {
    return Err(TokenError::Expired);
  }
  if expiry > now.saturating_add(max_ttl) {
    return Err(TokenError::TooFarInFuture);
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  // Shared with supabase/functions/tests/gateway_token.test.ts.
  const SECRET: &[u8] = b"test-secret-0123456789abcdef";
  const PLACEMENT: &str = "11111111-2222-3333-4444-555555555555";
  const VECTOR: &str = "v1.11111111-2222-3333-4444-555555555555.1700000120.de3bbc5d0ee1ddca9929f572086391dca3b8e6897df5ec790edbcfd993475d9f";

  #[test]
  fn mints_the_shared_test_vector() {
    assert_eq!(mint(SECRET, PLACEMENT, 1_700_000_120), VECTOR);
  }

  #[test]
  fn accepts_the_shared_test_vector_before_expiry() {
    assert_eq!(
      verify(SECRET, PLACEMENT, VECTOR, 1_700_000_000, MAX_TTL_SECS),
      Ok(())
    );
  }

  #[test]
  fn rejects_expired_token() {
    assert_eq!(
      verify(SECRET, PLACEMENT, VECTOR, 1_700_000_121, MAX_TTL_SECS),
      Err(TokenError::Expired)
    );
  }

  #[test]
  fn rejects_expiry_beyond_max_ttl() {
    assert_eq!(
      verify(SECRET, PLACEMENT, VECTOR, 1_699_999_999, MAX_TTL_SECS),
      Err(TokenError::TooFarInFuture)
    );
  }

  #[test]
  fn rejects_token_for_another_placement() {
    let other = mint(SECRET, "other", 1_700_000_120);
    assert_eq!(
      verify(SECRET, PLACEMENT, &other, 1_700_000_000, MAX_TTL_SECS),
      Err(TokenError::WrongPlacement)
    );
  }

  #[test]
  fn rejects_token_signed_with_another_secret() {
    let forged = mint(b"wrong", PLACEMENT, 1_700_000_120);
    assert_eq!(
      verify(SECRET, PLACEMENT, &forged, 1_700_000_000, MAX_TTL_SECS),
      Err(TokenError::BadSignature)
    );
  }

  #[test]
  fn rejects_malformed_token() {
    assert_eq!(
      verify(SECRET, PLACEMENT, "garbage", 0, MAX_TTL_SECS),
      Err(TokenError::Malformed)
    );
  }
}
