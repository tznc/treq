//! Web and Supabase endpoints, from the same `package.json` `env` block the
//! desktop frontend reads. Builds use `prod` unless `TREQ_GPUI_ENV=dev` is set
//! at compile time (to point a debug build at a local Supabase stack).

use std::sync::OnceLock;

use serde::Deserialize;

const PACKAGE_JSON: &str = include_str!("../../package.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Env {
  pub web_url: String,
  pub supabase: Supabase,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Supabase {
  pub url: String,
  pub anon_key: String,
}

#[derive(Deserialize)]
struct Package {
  env: Envs,
}

#[derive(Deserialize)]
struct Envs {
  dev: Env,
  prod: Env,
}

pub fn env() -> &'static Env {
  static ENV: OnceLock<Env> = OnceLock::new();
  ENV.get_or_init(|| {
    let envs = serde_json::from_str::<Package>(PACKAGE_JSON)
      .expect("package.json has an `env` block")
      .env;
    match option_env!("TREQ_GPUI_ENV") {
      Some("dev") => envs.dev,
      _ => envs.prod,
    }
  })
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
