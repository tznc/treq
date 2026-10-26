//! Web and Supabase endpoints, from the root `package.json` `env` block
//! (read by treq's `core::supabase`). Builds use `prod` unless
//! `TREQ_GPUI_ENV=dev` is set at compile time (to point a debug build at a
//! local Supabase stack).

use std::sync::OnceLock;

pub use crate::core::supabase::Env;

pub fn env() -> &'static Env {
  static ENV: OnceLock<Env> = OnceLock::new();
  ENV
    .get_or_init(|| crate::core::supabase::package_env(option_env!("TREQ_GPUI_ENV") == Some("dev")))
}

#[cfg(test)]
mod tests {
  #[test]
  fn reads_prod_by_default() {
    let env = super::env();
    assert!(env.web_url.starts_with("https://"));
    assert!(env.supabase.url.starts_with("https://"));
    assert!(!env.supabase.anon_key.is_empty());
  }
}
