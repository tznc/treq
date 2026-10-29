//! The priced size tiers a cloud workspace can run as
//! (`get_machine_catalog`), and the account's billed usage
//! (`get_usage_summary`).
//!
//! Cloud workspaces are pay-as-you-go: every tier is billed by the second
//! at the price the catalog lists for the instance's region. The provider
//! behind a tier is never shown.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use super::supabase::{ApiError, Supabase};

/// One region a tier runs in, and its billed price there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TierRegion {
  pub region: String,
  pub machine_type_id: String,
  #[serde(deserialize_with = "number")]
  pub hourly_usd: f64,
}

/// A size tier (`s`, `m`, `l`, `xl`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tier {
  pub id: String,
  pub label: String,
  pub vcpu: u32,
  #[serde(deserialize_with = "number")]
  pub ram_gib: f64,
  pub disk_gb: u32,
  #[serde(default)]
  pub regions: Vec<TierRegion>,
}

impl Tier {
  /// The billed hourly price in `region`, else in the first region listed
  /// (the instance's region is unknown before it is created).
  pub fn hourly_usd(&self, region: Option<&str>) -> Option<f64> {
    let by_region = region.and_then(|r| self.regions.iter().find(|x| x.region == r));
    by_region.or(self.regions.first()).map(|r| r.hourly_usd)
  }
}

/// PostgREST sends `numeric` as a JSON number, but a string is accepted too.
fn number<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
  match Value::deserialize(d)? {
    Value::Number(n) => n
      .as_f64()
      .ok_or_else(|| serde::de::Error::custom("not a number")),
    Value::String(s) => s.trim().parse().map_err(serde::de::Error::custom),
    other => Err(serde::de::Error::custom(format!(
      "expected a number, got {other}"
    ))),
  }
}

/// Parses `get_machine_catalog()`'s array.
pub fn parse_catalog(value: Value) -> Result<Vec<Tier>, String> {
  serde_json::from_value(value).map_err(|e| format!("invalid machine catalog: {e}"))
}

pub async fn fetch_catalog(supabase: &Supabase) -> Result<Vec<Tier>, ApiError> {
  let value: Value = supabase.rpc("get_machine_catalog", &json!({})).await?;
  parse_catalog(value).map_err(|e| ApiError::new("invalid_catalog", e))
}

/// `$0.0123/hour`.
pub fn format_hourly(usd: f64) -> String {
  format!("${usd:.4}/hour")
}

/// A row of `get_usage_summary`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UsageRow {
  pub hour: String,
  pub seconds: i64,
  #[serde(deserialize_with = "number")]
  pub billed_usd: f64,
  /// The part billed for workspaces on the user's own provider account.
  #[serde(default, deserialize_with = "number")]
  pub byos_billed_usd: f64,
}

/// What a period of usage adds up to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageTotals {
  pub billed_usd: f64,
  pub hours: f64,
  /// The part of `billed_usd` for BYOS workspaces (5% of provider cost).
  pub byos_billed_usd: f64,
}

impl UsageTotals {
  pub fn of(rows: &[UsageRow]) -> Self {
    Self {
      billed_usd: rows.iter().map(|r| r.billed_usd).sum(),
      byos_billed_usd: rows.iter().map(|r| r.byos_billed_usd).sum(),
      hours: rows.iter().map(|r| r.seconds as f64).sum::<f64>() / 3600.,
    }
  }
}

/// The first instant of `now`'s UTC month, and of the next.
pub fn month_bounds(
  now: chrono::DateTime<chrono::Utc>,
) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
  use chrono::{Datelike, TimeZone};
  let start = chrono::Utc
    .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
    .unwrap();
  let (year, month) = if now.month() == 12 {
    (now.year() + 1, 1)
  } else {
    (now.year(), now.month() + 1)
  };
  let end = chrono::Utc
    .with_ymd_and_hms(year, month, 1, 0, 0, 0)
    .unwrap();
  (start, end)
}

/// This month's billed usage.
pub async fn fetch_month_usage(supabase: &Supabase) -> Result<UsageTotals, ApiError> {
  let (from, to) = month_bounds(chrono::Utc::now());
  let rows: Vec<UsageRow> = supabase
    .rpc(
      "get_usage_summary",
      &json!({ "p_from": from.to_rfc3339(), "p_to": to.to_rfc3339() }),
    )
    .await?;
  Ok(UsageTotals::of(&rows))
}

/// Where this month's spend stands against the cap (`get_spend_status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpendState {
  Ok,
  /// 80% of the cap or more.
  Warning,
  /// At the cap: workspaces are stopped and cannot start.
  Capped,
}

/// The monthly spend cap and this calendar month's (UTC) spend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendStatus {
  #[serde(deserialize_with = "number")]
  pub cap_usd: f64,
  #[serde(deserialize_with = "number")]
  pub spent_usd: f64,
  pub month_start: String,
  pub state: SpendState,
}

/// A stored notice the app shows until dismissed (`list_notifications`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
  pub id: String,
  pub kind: String,
  pub title: String,
  pub body: String,
  #[serde(default)]
  pub created_at: String,
}

// -- Bring your own sandbox (BYOS) ----------------------------------------

/// The providers a user can bring their own key for: kind and name.
pub const BYOS_PROVIDERS: [(&str, &str); 3] = [
  ("boat", "Boat"),
  ("daytona", "Daytona"),
  ("fly_machines", "Fly Machines"),
];

/// A provider's display name.
pub fn provider_name(kind: &str) -> String {
  BYOS_PROVIDERS
    .iter()
    .find(|(k, _)| *k == kind)
    .map(|(_, name)| name.to_string())
    .unwrap_or_else(|| kind.to_string())
}

/// A saved provider key, as `list_provider_credentials` lists it. The key
/// itself never comes back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderKey {
  pub id: String,
  pub provider_kind: String,
  #[serde(default)]
  pub label: Option<String>,
  #[serde(default)]
  pub validated_at: Option<String>,
  /// The provider refused it (401/403) since it was saved.
  #[serde(default)]
  pub invalid_at: Option<String>,
}

/// One tier on a provider the user has a key for (`get_byos_catalog`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ByosMachineType {
  pub provider_kind: String,
  pub tier_id: String,
  /// What treq bills: 5% of the provider's price.
  #[serde(deserialize_with = "number")]
  pub hourly_usd: f64,
  /// The provider's list price, which the provider bills the user.
  #[serde(deserialize_with = "number")]
  pub provider_hourly_usd: f64,
}

pub async fn fetch_spend_status(supabase: &Supabase) -> Result<SpendStatus, ApiError> {
  supabase.rpc("get_spend_status", &json!({})).await
}

/// Sets the cap; the server refuses under $5 or over its maximum.
pub async fn set_spend_cap(supabase: &Supabase, cap_usd: f64) -> Result<SpendStatus, ApiError> {
  supabase
    .rpc("set_spend_cap", &json!({ "p_cap_usd": cap_usd }))
    .await
}

pub async fn fetch_notifications(supabase: &Supabase) -> Result<Vec<Notification>, ApiError> {
  supabase.rpc("list_notifications", &json!({})).await
}

pub async fn dismiss_notification(supabase: &Supabase, id: &str) -> Result<(), ApiError> {
  supabase
    .rpc::<Value>("dismiss_notification", &json!({ "p_id": id }))
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn catalog() -> Value {
    json!([
      {"id": "s", "label": "Small", "vcpu": 2, "ram_gib": 4, "disk_gb": 10, "regions": [
        {"region": "eu_west", "machine_type_id": "boat:small", "hourly_usd": 0.0216},
        {"region": "us_east", "machine_type_id": "daytona:s", "hourly_usd": "0.030000"}
      ]},
      {"id": "xl", "label": "Extra large", "vcpu": 16, "ram_gib": 32.5, "disk_gb": 100, "regions": []}
    ])
  }

  #[test]
  fn parses_the_catalog_with_numeric_prices_as_numbers_or_strings() {
    let tiers = parse_catalog(catalog()).unwrap();
    assert_eq!(tiers.len(), 2);
    assert_eq!(tiers[0].id, "s");
    assert_eq!(tiers[0].vcpu, 2);
    assert_eq!(tiers[0].disk_gb, 10);
    assert_eq!(tiers[0].regions[1].hourly_usd, 0.03);
    assert_eq!(tiers[1].ram_gib, 32.5);
    assert!(tiers[1].regions.is_empty());
  }

  #[test]
  fn rejects_a_catalog_that_is_not_an_array_of_tiers() {
    assert!(parse_catalog(json!({"id": "s"})).is_err());
    assert!(parse_catalog(json!([{"id": "s"}])).is_err());
  }

  #[test]
  fn prices_a_tier_in_the_instance_region_or_the_first_listed() {
    let tiers = parse_catalog(catalog()).unwrap();
    assert_eq!(tiers[0].hourly_usd(Some("us_east")), Some(0.03));
    assert_eq!(tiers[0].hourly_usd(Some("ap_southeast")), Some(0.0216));
    assert_eq!(tiers[0].hourly_usd(None), Some(0.0216));
    assert_eq!(tiers[1].hourly_usd(None), None);
  }

  #[test]
  fn formats_an_hourly_price_to_four_decimals() {
    assert_eq!(format_hourly(0.0216), "$0.0216/hour");
    assert_eq!(format_hourly(1.5), "$1.5000/hour");
  }

  #[test]
  fn sums_the_byos_part_of_billed_usage() {
    let rows: Vec<UsageRow> = serde_json::from_value(json!([
      {"hour": "2026-10-01T00:00:00+00:00", "seconds": 3600, "billed_usd": 0.03, "byos_billed_usd": "0.001"},
      {"hour": "2026-10-01T01:00:00+00:00", "seconds": 3600, "billed_usd": 0.02}
    ]))
    .unwrap();
    let totals = UsageTotals::of(&rows);
    assert!((totals.byos_billed_usd - 0.001).abs() < 1e-9);
  }

  #[test]
  fn sums_usage_rows_into_billed_total_and_hours() {
    let rows: Vec<UsageRow> = serde_json::from_value(json!([
      {"hour": "2026-10-01T00:00:00+00:00", "seconds": 3600, "billed_usd": 0.03},
      {"hour": "2026-10-01T01:00:00+00:00", "seconds": 1800, "billed_usd": "0.015"}
    ]))
    .unwrap();
    let totals = UsageTotals::of(&rows);
    assert!((totals.billed_usd - 0.045).abs() < 1e-9);
    assert_eq!(totals.hours, 1.5);
  }

  #[test]
  fn bounds_the_month_including_december() {
    use chrono::TimeZone;
    let (from, to) = month_bounds(chrono::Utc.with_ymd_and_hms(2026, 12, 15, 8, 0, 0).unwrap());
    assert_eq!(from.to_rfc3339(), "2026-12-01T00:00:00+00:00");
    assert_eq!(to.to_rfc3339(), "2027-01-01T00:00:00+00:00");
  }

  #[tokio::test]
  async fn fetches_the_catalog_and_usage_through_rpcs() {
    use wiremock::{
      matchers::{method, path},
      Mock, MockServer, ResponseTemplate,
    };
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/get_machine_catalog"))
      .respond_with(ResponseTemplate::new(200).set_body_json(catalog()))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/get_usage_summary"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([
        {"hour": "2026-10-01T00:00:00+00:00", "seconds": 7200, "billed_usd": 0.06}
      ])))
      .mount(&server)
      .await;
    let (supabase, _) = crate::core::supabase::test_support::client(&server.uri());
    assert_eq!(fetch_catalog(&supabase).await.unwrap().len(), 2);
    let usage = fetch_month_usage(&supabase).await.unwrap();
    assert_eq!(usage.hours, 2.);
    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
    assert!(body["p_from"]
      .as_str()
      .unwrap()
      .ends_with("-01T00:00:00+00:00"));
  }

  #[test]
  fn parses_the_spend_status() {
    let status: SpendStatus = serde_json::from_value(json!({
      "cap_usd": "50.00", "spent_usd": 41.5, "month_start": "2026-10-01T00:00:00+00:00",
      "state": "warning"
    }))
    .unwrap();
    assert_eq!(status.cap_usd, 50.);
    assert_eq!(status.spent_usd, 41.5);
    assert_eq!(status.state, SpendState::Warning);
  }

  #[tokio::test]
  async fn reads_and_sets_the_spend_cap_and_notifications_through_rpcs() {
    use wiremock::{
      matchers::{body_json, method, path},
      Mock, MockServer, ResponseTemplate,
    };
    let server = MockServer::start().await;
    let status = |cap: f64, state: &str| json!({"cap_usd": cap, "spent_usd": 60, "month_start": "2026-10-01T00:00:00+00:00", "state": state});
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/get_spend_status"))
      .respond_with(ResponseTemplate::new(200).set_body_json(status(50., "capped")))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/set_spend_cap"))
      .and(body_json(json!({"p_cap_usd": 100.0})))
      .respond_with(ResponseTemplate::new(200).set_body_json(status(100., "ok")))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/list_notifications"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
        "id": "n1", "kind": "spend_cap_reached", "period": "2026-10",
        "title": "Cloud spend cap reached", "body": "Stopped.",
        "created_at": "2026-10-28T12:00:00+00:00"
      }])))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/dismiss_notification"))
      .and(body_json(json!({"p_id": "n1"})))
      .respond_with(ResponseTemplate::new(204))
      .mount(&server)
      .await;
    let (supabase, _) = crate::core::supabase::test_support::client(&server.uri());
    assert_eq!(
      fetch_spend_status(&supabase).await.unwrap().state,
      SpendState::Capped
    );
    let raised = set_spend_cap(&supabase, 100.).await.unwrap();
    assert_eq!((raised.cap_usd, raised.state), (100., SpendState::Ok));
    let notes = fetch_notifications(&supabase).await.unwrap();
    assert_eq!(notes[0].title, "Cloud spend cap reached");
    dismiss_notification(&supabase, "n1").await.unwrap();
  }
}
