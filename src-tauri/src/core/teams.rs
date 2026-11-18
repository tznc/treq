//! Organizations and Team, as the web dashboard's Teams section
//! (`TeamTab`, `loadTeamData`) reads them. Reads go through RLS; every
//! change goes through the `organizations` Edge Function, whose SQL
//! enforces the rules.

use serde::Deserialize;
use serde_json::Value;

use super::supabase::{ApiError, Supabase};

/// Members a Team subscription includes (`TEAM_INCLUDED_SEATS`).
pub const TEAM_INCLUDED_SEATS: usize = 5;
/// Each member beyond those adds this much per month (`TEAM_EXTRA_SEAT_USD`).
pub const TEAM_EXTRA_SEAT_USD: u32 = 8;

/// A row of `organization_members`, with its organization's name.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Membership {
  pub org_id: String,
  pub user_id: String,
  /// `owner`, `admin` or `member`.
  pub role: String,
  #[serde(default)]
  pub organizations: Option<OrgName>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OrgName {
  pub id: String,
  pub name: String,
}

/// A member's `profiles` row.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Profile {
  pub id: String,
  #[serde(default)]
  pub email: Option<String>,
  #[serde(default)]
  pub full_name: Option<String>,
}

/// An invite not yet accepted, revoked or expired.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Invite {
  pub id: String,
  pub org_id: String,
  pub email: String,
  pub expires_at: String,
}

/// A live Team subscription of an organization.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TeamSubscription {
  pub owner_id: String,
  pub status: String,
  #[serde(default)]
  pub current_period_end: Option<String>,
  #[serde(default)]
  pub cancel_at_period_end: bool,
}

/// What the Teams section draws, for the signed-in user `user_id`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TeamData {
  pub user_id: String,
  pub memberships: Vec<Membership>,
  pub profiles: Vec<Profile>,
  pub invites: Vec<Invite>,
  pub subscriptions: Vec<TeamSubscription>,
}

/// `loadTeamData`: the user's organizations, their members and profiles,
/// pending invites and Team subscriptions.
pub async fn load(supabase: &Supabase, user_id: &str) -> Result<TeamData, ApiError> {
  let memberships: Vec<Membership> = supabase
    .select(
      "organization_members",
      "select=org_id,user_id,role,created_at,organizations(id,name)&order=created_at",
    )
    .await?;
  let mut ids: Vec<&str> = memberships.iter().map(|m| m.user_id.as_str()).collect();
  ids.sort_unstable();
  ids.dedup();
  let profiles = async {
    if ids.is_empty() {
      return Ok(Vec::new());
    }
    let query = format!("select=id,email,full_name&id=in.({})", ids.join(","));
    supabase.select("profiles", &query).await
  };
  let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
  let invites_query = format!(
    "select=id,org_id,email,expires_at&accepted_at=is.null&revoked_at=is.null&expires_at=gt.{now}&order=created_at"
  );
  let invites = supabase.select("organization_invites", &invites_query);
  let subscriptions = supabase.select(
    "billing_subscriptions",
    "select=owner_id,status,current_period_end,cancel_at_period_end&owner_type=eq.organization&status=in.(trialing,active,past_due)",
  );
  let (profiles, invites, subscriptions) = tokio::try_join!(profiles, invites, subscriptions)?;
  Ok(TeamData {
    user_id: user_id.to_string(),
    memberships,
    profiles,
    invites,
    subscriptions,
  })
}

/// One `organizations` action (`OrganizationAction`), as in
/// `{"action": "invite", "organization_id": ..., "email": ...}`.
pub async fn organization_action(supabase: &Supabase, body: &Value) -> Result<Value, ApiError> {
  supabase.invoke("organizations", body).await
}

#[cfg(test)]
mod tests {
  use serde_json::json;
  use wiremock::{
    matchers::{body_json, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;
  use crate::core::supabase::test_support::client;

  async fn get(server: &MockServer, table: &str, rows: Value) {
    Mock::given(method("GET"))
      .and(path(format!("/rest/v1/{table}")))
      .respond_with(ResponseTemplate::new(200).set_body_json(rows))
      .mount(server)
      .await;
  }

  #[tokio::test]
  async fn loads_organizations_members_invites_and_team_subscriptions() {
    let server = MockServer::start().await;
    get(
      &server,
      "organization_members",
      json!([
        {"org_id": "o1", "user_id": "u1", "role": "owner", "created_at": "t",
         "organizations": {"id": "o1", "name": "Acme"}},
        {"org_id": "o1", "user_id": "u2", "role": "member", "created_at": "t",
         "organizations": {"id": "o1", "name": "Acme"}},
      ]),
    )
    .await;
    Mock::given(method("GET"))
      .and(path("/rest/v1/profiles"))
      .and(query_param("id", "in.(u1,u2)"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([
        {"id": "u1", "email": "ada@acme.dev", "full_name": "Ada"},
        {"id": "u2", "email": "bob@acme.dev", "full_name": null},
      ])))
      .mount(&server)
      .await;
    get(
      &server,
      "organization_invites",
      json!([{"id": "i1", "org_id": "o1", "email": "cy@acme.dev", "expires_at": "2026-11-16T00:00:00Z"}]),
    )
    .await;
    get(
      &server,
      "billing_subscriptions",
      json!([{"owner_id": "o1", "status": "active", "current_period_end": null, "cancel_at_period_end": false}]),
    )
    .await;
    let (supabase, _) = client(&server.uri());
    let data = load(&supabase, "u1").await.unwrap();
    assert_eq!(data.user_id, "u1");
    assert_eq!(data.memberships.len(), 2);
    assert_eq!(
      data.memberships[0]
        .organizations
        .as_ref()
        .map(|o| o.name.as_str()),
      Some("Acme")
    );
    assert_eq!(data.profiles[1].email.as_deref(), Some("bob@acme.dev"));
    assert_eq!(data.invites[0].email, "cy@acme.dev");
    assert_eq!(data.subscriptions[0].owner_id, "o1");
  }

  #[tokio::test]
  async fn loads_nothing_more_without_organizations() {
    let server = MockServer::start().await;
    get(&server, "organization_members", json!([])).await;
    get(&server, "organization_invites", json!([])).await;
    get(&server, "billing_subscriptions", json!([])).await;
    let (supabase, _) = client(&server.uri());
    let data = load(&supabase, "u1").await.unwrap();
    assert!(data.memberships.is_empty() && data.profiles.is_empty());
  }

  #[tokio::test]
  async fn sends_the_action_and_reads_the_refusal() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/organizations"))
      .and(body_json(
        json!({"action": "leave", "organization_id": "o1"}),
      ))
      .respond_with(
        ResponseTemplate::new(400).set_body_json(json!({"error": "The last owner cannot leave."})),
      )
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let error = organization_action(
      &supabase,
      &json!({"action": "leave", "organization_id": "o1"}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.message, "The last owner cannot leave.");
  }
}
