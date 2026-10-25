//! A small Supabase client: PostgREST `select` and `rpc`, and Edge Function
//! calls, authenticated with the signed-in user's session ([`Auth`]).
//!
//! The project's URL and anon key come from the root `package.json` `env`
//! block, the same one the frontend and the website read. Desktop and
//! mobile share this client; each gives it a [`SessionStore`] for its own
//! settings database.

use std::sync::{Arc, OnceLock};

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use super::auth::{Auth, SessionStore};

const PACKAGE_JSON: &str = include_str!("../../../package.json");

/// One environment of `package.json`'s `env` block.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Env {
  pub web_url: String,
  pub supabase: SupabaseEnv,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupabaseEnv {
  pub url: String,
  pub anon_key: String,
}

#[derive(Deserialize)]
struct Envs {
  dev: Env,
  prod: Env,
}

#[derive(Deserialize)]
struct Package {
  env: Envs,
}

/// The `dev` or `prod` environment of `package.json`.
#[expect(
  clippy::expect_used,
  reason = "package.json is embedded at build time and checked by tests"
)]
pub fn package_env(dev: bool) -> Env {
  let envs = serde_json::from_str::<Package>(PACKAGE_JSON)
    .expect("package.json has an `env` block")
    .env;
  if dev {
    envs.dev
  } else {
    envs.prod
  }
}

/// The desktop app's environment: `dev` in debug builds, as the frontend
/// picks it (`import.meta.env.PROD`).
pub fn desktop_env() -> &'static Env {
  static ENV: OnceLock<Env> = OnceLock::new();
  ENV.get_or_init(|| package_env(cfg!(debug_assertions)))
}

/// A failed Supabase call: an HTTP refusal, or `status` 0 for a transport
/// failure. Displays as `remoteFunctionError` formats Edge Function errors.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
  pub status: u16,
  pub code: String,
  pub message: String,
  pub correlation_id: Option<String>,
}

impl std::fmt::Display for ApiError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "[{}] {}", self.code, self.message)?;
    if self.status > 0 {
      write!(f, "\nHTTP {}", self.status)?;
      if let Some(id) = &self.correlation_id {
        write!(f, " · Correlation ID: {id}")?;
      }
    }
    Ok(())
  }
}

impl std::error::Error for ApiError {}

/// The PostgREST code `set_merge_queue_enabled` raises without Pro.
const POSTGREST_PRO_REQUIRED: &str = "PT402";

impl ApiError {
  pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
    Self {
      status: 0,
      code: code.into(),
      message: message.into(),
      correlation_id: None,
    }
  }

  pub(crate) fn transport(error: impl std::fmt::Display) -> Self {
    Self::new("network", error.to_string())
  }

  /// `isProRequiredError`: the server refused for want of Pro.
  pub fn is_pro_required(&self) -> bool {
    self.status == 402 || self.code == "pro_required" || self.code == POSTGREST_PRO_REQUIRED
  }
}

/// The error bodies of Edge Functions, PostgREST and GoTrue.
#[derive(Deserialize, Default)]
struct ErrorBody {
  #[serde(default)]
  error: Option<Value>,
  #[serde(default)]
  message: Option<String>,
  #[serde(default)]
  msg: Option<String>,
  #[serde(default)]
  error_description: Option<String>,
  #[serde(default)]
  code: Option<Value>,
  #[serde(default)]
  hint: Option<String>,
  #[serde(default)]
  provider_error: Option<String>,
  #[serde(default)]
  correlation_id: Option<String>,
}

fn api_error(status: u16, header_id: Option<String>, body: ErrorBody) -> ApiError {
  let code = body
    .code
    .and_then(|code| match code {
      Value::String(code) => Some(code),
      Value::Number(n) => Some(n.to_string()),
      _ => None,
    })
    .filter(|code| !code.is_empty())
    .or(body.hint.filter(|h| h == "pro_required"))
    .or(body.provider_error)
    .unwrap_or_else(|| format!("http_{status}"));
  let error = body.error.and_then(|e| match e {
    Value::String(e) => Some(e),
    _ => None,
  });
  ApiError {
    status,
    code,
    message: body
      .error_description
      .or(error)
      .or(body.message)
      .or(body.msg)
      .unwrap_or_else(|| "Request failed".into()),
    correlation_id: body.correlation_id.or(header_id),
  }
}

/// Reads a response: its JSON on success, an [`ApiError`] otherwise.
pub(crate) async fn read<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ApiError> {
  let status = response.status();
  if !status.is_success() {
    let header_id = response
      .headers()
      .get("x-correlation-id")
      .and_then(|v| v.to_str().ok())
      .map(str::to_owned);
    let body = response.json::<ErrorBody>().await.unwrap_or_default();
    return Err(api_error(status.as_u16(), header_id, body));
  }
  let text = response.text().await.map_err(ApiError::transport)?;
  // `rpc` of a void function answers 204 with no body.
  let text = if text.trim().is_empty() {
    "null"
  } else {
    &text
  };
  serde_json::from_str(text).map_err(ApiError::transport)
}

/// The client: PostgREST, Edge Functions, and the session they run as.
pub struct Supabase {
  url: String,
  anon_key: String,
  http: reqwest::Client,
  pub auth: Arc<Auth>,
}

impl Supabase {
  pub fn new(env: &SupabaseEnv, store: Arc<dyn SessionStore>) -> Self {
    // The overall timeout sits above the Edge Function wall-clock limit
    // (150 s), so it never cuts a call the server would still finish.
    let http = reqwest::Client::builder()
      .user_agent(concat!("treq/", env!("CARGO_PKG_VERSION")))
      .connect_timeout(std::time::Duration::from_secs(10))
      .timeout(std::time::Duration::from_secs(160))
      .build()
      .unwrap_or_else(|error| {
        tracing::warn!(%error, "falling back to a default Supabase HTTP client");
        reqwest::Client::default()
      });
    Self {
      auth: Arc::new(Auth::new(env, http.clone(), store)),
      url: env.url.trim_end_matches('/').to_string(),
      anon_key: env.anon_key.clone(),
      http,
    }
  }

  pub fn url(&self) -> &str {
    &self.url
  }

  /// The user's access token, or the anon key while signed out.
  async fn bearer(&self) -> String {
    self
      .auth
      .access_token()
      .await
      .unwrap_or_else(|| self.anon_key.clone())
  }

  async fn send<T: DeserializeOwned>(
    &self,
    request: reqwest::RequestBuilder,
  ) -> Result<T, ApiError> {
    let response = request
      .bearer_auth(self.bearer().await)
      .header("apikey", &self.anon_key)
      .send()
      .await
      .map_err(ApiError::transport)?;
    read(response).await
  }

  /// `from(table).select(...)`: `query` is PostgREST's query string, as in
  /// `select=id,full_name`.
  pub async fn select<T: DeserializeOwned>(&self, table: &str, query: &str) -> Result<T, ApiError> {
    let url = format!("{}/rest/v1/{table}?{query}", self.url);
    self.send(self.http.get(url)).await
  }

  /// `.select(...).single()`: the one row, `None` when there is none.
  pub async fn select_single<T: DeserializeOwned>(
    &self,
    table: &str,
    query: &str,
  ) -> Result<Option<T>, ApiError> {
    let rows: Vec<T> = self.select(table, query).await?;
    let mut rows = rows.into_iter();
    match (rows.next(), rows.next()) {
      (Some(row), None) => Ok(Some(row)),
      (None, _) => Ok(None),
      (Some(_), Some(_)) => Err(ApiError::new(
        "PGRST116",
        "JSON object requested, multiple (or no) rows returned",
      )),
    }
  }

  /// `rpc(name, args)`.
  pub async fn rpc<T: DeserializeOwned>(&self, name: &str, args: &Value) -> Result<T, ApiError> {
    let url = format!("{}/rest/v1/rpc/{name}", self.url);
    self.send(self.http.post(url).json(args)).await
  }

  /// `functions.invoke(name, { body })`.
  pub async fn invoke<T: DeserializeOwned>(&self, name: &str, body: &Value) -> Result<T, ApiError> {
    let url = format!("{}/functions/v1/{name}", self.url);
    self.send(self.http.post(url).json(body)).await
  }
}

#[cfg(test)]
pub(crate) mod test_support {
  use std::sync::Mutex;

  use super::*;

  /// A session store in memory.
  #[derive(Default)]
  pub struct MemoryStore(pub Mutex<Option<String>>);

  impl SessionStore for MemoryStore {
    fn load(&self) -> Option<String> {
      self.0.lock().unwrap().clone().filter(|v| !v.is_empty())
    }
    fn save(&self, value: &str) {
      *self.0.lock().unwrap() = Some(value.to_string());
    }
  }

  pub fn env(url: &str) -> SupabaseEnv {
    SupabaseEnv {
      url: url.to_string(),
      anon_key: "anon".into(),
    }
  }

  /// A client of the fake server at `url`, with an empty store.
  pub fn client(url: &str) -> (Supabase, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    (Supabase::new(&env(url), store.clone()), store)
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;
  use wiremock::{
    matchers::{body_json, header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
  };

  use super::{test_support::client, *};

  #[test]
  fn reads_both_environments_from_package_json() {
    let prod = package_env(false);
    assert!(prod.web_url.starts_with("https://"));
    assert!(prod.supabase.url.starts_with("https://"));
    assert!(!prod.supabase.anon_key.is_empty());
    assert_ne!(package_env(true), prod);
  }

  #[test]
  fn errors_format_like_remote_function_error() {
    let error = api_error(
      409,
      Some("cid".into()),
      ErrorBody {
        error: Some(json!("Key has been revoked")),
        provider_error: Some("sprites".into()),
        ..Default::default()
      },
    );
    assert_eq!(error.code, "sprites");
    assert_eq!(
      error.to_string(),
      "[sprites] Key has been revoked\nHTTP 409 · Correlation ID: cid"
    );
    assert_eq!(api_error(500, None, ErrorBody::default()).code, "http_500");
  }

  #[test]
  fn pro_refusals_are_recognized() {
    let rpc = api_error(
      402,
      None,
      serde_json::from_value(json!({"code": "PT402", "message": "Pro"})).unwrap(),
    );
    assert!(rpc.is_pro_required());
    assert_eq!(rpc.message, "Pro");
    assert!(!ApiError::new("network", "down").is_pro_required());
  }

  #[tokio::test]
  async fn selects_rows_as_the_anon_key_while_signed_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
      .and(path("/rest/v1/github_repositories"))
      .and(query_param("select", "id,full_name"))
      .and(header("authorization", "Bearer anon"))
      .and(header("apikey", "anon"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"id": 1}])))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let rows: Vec<Value> = supabase
      .select("github_repositories", "select=id,full_name")
      .await
      .unwrap();
    assert_eq!(rows, vec![json!({"id": 1})]);
  }

  #[tokio::test]
  async fn single_is_none_without_a_row() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
      .and(path("/rest/v1/subscriptions"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let row: Option<Value> = supabase
      .select_single("subscriptions", "select=*")
      .await
      .unwrap();
    assert_eq!(row, None);
  }

  #[tokio::test]
  async fn calls_rpcs_and_functions_and_reads_their_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/get_merge_queue_enabled"))
      .and(body_json(json!({"p_repo_full_name": "o/r"})))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!(true)))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/void_fn"))
      .respond_with(ResponseTemplate::new(204))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .respond_with(
        ResponseTemplate::new(402)
          .insert_header("x-correlation-id", "c1")
          .set_body_json(json!({"error": "Needs Pro", "code": "pro_required"})),
      )
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let enabled: bool = supabase
      .rpc(
        "get_merge_queue_enabled",
        &json!({"p_repo_full_name": "o/r"}),
      )
      .await
      .unwrap();
    assert!(enabled);
    let unit: Value = supabase.rpc("void_fn", &json!({})).await.unwrap();
    assert_eq!(unit, Value::Null);
    let error = supabase
      .invoke::<Value>("remote-instance", &json!({"action": "status"}))
      .await
      .unwrap_err();
    assert!(error.is_pro_required());
    assert_eq!(
      error.to_string(),
      "[pro_required] Needs Pro\nHTTP 402 · Correlation ID: c1"
    );
  }
}
