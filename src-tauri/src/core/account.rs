//! What the signed-in account reads from Supabase: its subscription, the
//! GitHub repositories the Treq app can see, Linear OAuth, and the merge
//! queue (`subscriptions`, `github_repositories`, the merge queue RPCs and
//! the `enqueue-workspace` and `create-linear-oauth-intent` functions), and
//! deleting the account (`delete-account`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::supabase::{ApiError, Supabase};

/// A row of `public.subscriptions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
  /// `canceled` means set to cancel at the period end, still Pro until then.
  pub status: String,
  pub plan: String,
  #[serde(default)]
  pub current_period_end: Option<String>,
}

impl Subscription {
  /// `isProSubscription`: statuses that still carry Pro.
  pub fn is_pro(&self) -> bool {
    self.plan == "pro"
      && ["trialing", "active", "past_due", "canceled"].contains(&self.status.as_str())
  }
}

/// The account's subscription; `None` without a row.
pub async fn subscription(supabase: &Supabase) -> Result<Option<Subscription>, ApiError> {
  supabase.select_single("subscriptions", "select=*").await
}

/// A row of `public.github_repositories`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GithubRepository {
  pub id: i64,
  pub full_name: String,
  pub private: bool,
}

pub async fn github_repositories(supabase: &Supabase) -> Result<Vec<GithubRepository>, ApiError> {
  supabase
    .select(
      "github_repositories",
      "select=id,full_name,private,default_branch,installation_id",
    )
    .await
}

/// Where Linear's OAuth page for this account is.
pub async fn linear_oauth_url(supabase: &Supabase) -> Result<String, ApiError> {
  let data: Value = supabase
    .invoke("create-linear-oauth-intent", &json!({}))
    .await?;
  data["authorization_url"]
    .as_str()
    .filter(|url| !url.is_empty())
    .map(str::to_string)
    .ok_or_else(|| ApiError::new("no_url", "No authorization URL returned"))
}

/// Deletes the signed-in account (`delete-account`): its subscriptions are
/// cancelled and its Treq Cloud machines destroyed first. A 409 carries the
/// server's sentence on what to do first, such as handing over an
/// organization.
pub async fn delete_account(supabase: &Supabase) -> Result<(), ApiError> {
  supabase
    .invoke::<Value>("delete-account", &json!({}))
    .await
    .map(drop)
}

/// A queued branch (`get_repo_branch_queue_statuses`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
  pub branch_name: String,
  pub pr_number: Option<u64>,
  #[serde(default)]
  pub pr_title: Option<String>,
  pub status: String,
  pub position: i64,
  pub target_branch: String,
  #[serde(default)]
  pub insertions: Option<u32>,
  #[serde(default)]
  pub deletions: Option<u32>,
}

/// The repository's merge queue, by position then branch name.
pub async fn queue_entries(
  supabase: &Supabase,
  full_name: &str,
) -> Result<Vec<QueueEntry>, ApiError> {
  let entries: Option<Vec<QueueEntry>> = supabase
    .rpc(
      "get_repo_branch_queue_statuses",
      &json!({ "p_repo_full_name": full_name }),
    )
    .await?;
  let mut entries = entries.unwrap_or_default();
  entries.sort_by(|a, b| {
    a.position
      .cmp(&b.position)
      .then_with(|| a.branch_name.cmp(&b.branch_name))
  });
  Ok(entries)
}

/// Whether the repository opted in to the merge queue.
pub async fn queue_enabled(supabase: &Supabase, full_name: &str) -> Result<bool, ApiError> {
  let enabled: Value = supabase
    .rpc(
      "get_merge_queue_enabled",
      &json!({ "p_repo_full_name": full_name }),
    )
    .await?;
  Ok(enabled == Value::Bool(true))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueAction {
  Enqueue,
  Dequeue,
}

/// Adds the branch to, or removes it from, the repository's merge queue.
pub async fn enqueue(
  supabase: &Supabase,
  full_name: &str,
  branch: &str,
  action: QueueAction,
) -> Result<(), ApiError> {
  let action = match action {
    QueueAction::Enqueue => "enqueue",
    QueueAction::Dequeue => "dequeue",
  };
  supabase
    .invoke::<Value>(
      "enqueue-workspace",
      &json!({ "repo_full_name": full_name, "branch_name": branch, "action": action }),
    )
    .await
    .map(drop)
}

/// `useDequeueBranches`: removes a stack's branches top first, so no
/// branch is left stacked on a removed one.
pub async fn dequeue_branches(
  supabase: &Supabase,
  full_name: &str,
  branches: &[String],
) -> Result<(), ApiError> {
  for branch in branches.iter().rev() {
    enqueue(supabase, full_name, branch, QueueAction::Dequeue).await?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use wiremock::{
    matchers::{body_json, method, path},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;
  use crate::core::supabase::test_support::client;

  #[test]
  fn pro_covers_every_paid_status() {
    let sub = |plan: &str, status: &str| Subscription {
      plan: plan.into(),
      status: status.into(),
      current_period_end: None,
    };
    assert!(sub("pro", "canceled").is_pro());
    assert!(!sub("pro", "inactive").is_pro());
    assert!(!sub("free", "active").is_pro());
  }

  fn entry(branch: &str, position: i64) -> Value {
    json!({"branch_name": branch, "pr_number": 1, "status": "queued", "position": position, "target_branch": "main"})
  }

  #[tokio::test]
  async fn reads_the_queue_in_position_order() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/get_repo_branch_queue_statuses"))
      .and(body_json(json!({"p_repo_full_name": "o/r"})))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([
        entry("b", 2),
        entry("z", 1),
        entry("a", 2)
      ])))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/rest/v1/rpc/get_merge_queue_enabled"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!(true)))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let branches: Vec<_> = queue_entries(&supabase, "o/r")
      .await
      .unwrap()
      .into_iter()
      .map(|e| e.branch_name)
      .collect();
    assert_eq!(branches, ["z", "a", "b"]);
    assert!(queue_enabled(&supabase, "o/r").await.unwrap());
  }

  #[tokio::test]
  async fn dequeues_a_stack_top_first() {
    let server = MockServer::start().await;
    for branch in ["base", "top"] {
      Mock::given(method("POST"))
        .and(path("/functions/v1/enqueue-workspace"))
        .and(body_json(
          json!({"repo_full_name": "o/r", "branch_name": branch, "action": "dequeue"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;
    }
    let (supabase, _) = client(&server.uri());
    dequeue_branches(&supabase, "o/r", &["base".into(), "top".into()])
      .await
      .unwrap();
    let order: Vec<String> = server
      .received_requests()
      .await
      .unwrap()
      .iter()
      .map(|r| serde_json::from_slice::<Value>(&r.body).unwrap()["branch_name"].to_string())
      .collect();
    assert_eq!(order, ["\"top\"", "\"base\""]);
  }

  #[tokio::test]
  async fn deletes_the_account_or_passes_on_the_refusal() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/delete-account"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({"deleted": true})))
      .up_to_n_times(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/delete-account"))
      .respond_with(ResponseTemplate::new(409).set_body_json(json!({
        "error": "You are the only owner of Acme, which has other members.",
        "code": "sole_owner"
      })))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    delete_account(&supabase).await.unwrap();
    let refused = delete_account(&supabase).await.unwrap_err();
    assert_eq!(refused.status, 409);
    assert_eq!(refused.code, "sole_owner");
    assert_eq!(
      refused.message,
      "You are the only owner of Acme, which has other members."
    );
  }

  #[tokio::test]
  async fn reads_the_linear_oauth_url_and_github_repositories() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/create-linear-oauth-intent"))
      .respond_with(
        ResponseTemplate::new(200)
          .set_body_json(json!({"authorization_url": "https://linear.app/oauth"})),
      )
      .mount(&server)
      .await;
    Mock::given(method("GET"))
      .and(path("/rest/v1/github_repositories"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([
        {"id": 7, "full_name": "o/r", "private": true, "default_branch": "main", "installation_id": 1}
      ])))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    assert_eq!(
      linear_oauth_url(&supabase).await.unwrap(),
      "https://linear.app/oauth"
    );
    assert_eq!(
      github_repositories(&supabase).await.unwrap(),
      vec![GithubRepository {
        id: 7,
        full_name: "o/r".into(),
        private: true
      }]
    );
  }
}
