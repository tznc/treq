//! Team project sync: projects owned by a Team organization are shared with
//! its members through `public.organization_projects`. Personal projects
//! never leave the machine.
//!
//! One pass: push every local team project, then pull every remote one the
//! user can see, then drop local team projects of organizations the user
//! has left. Repos travel as remote references (`owner/name` or a clone
//! URL); each machine maps them to its own clone folder.

use crate::core::projects;
use async_trait::async_trait;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// Runs the sync loop only while signed in: started by sign-in (a token
/// refresh leaves the running loop alone), stopped by sign-out.
#[derive(Default)]
pub struct Runner(std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>);

impl Runner {
  pub fn signed_in<F, Fut>(&self, start: F)
  where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
  {
    let mut task = self
      .0
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner);
    if task.is_none() {
      *task = Some(tauri::async_runtime::spawn(start()));
    }
  }

  pub fn signed_out(&self) {
    if let Some(task) = self
      .0
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .take()
    {
      task.abort();
    }
  }

  pub fn is_running(&self) -> bool {
    self
      .0
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .is_some()
  }
}

static WAKE: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();

fn wake() -> &'static tokio::sync::Notify {
  WAKE.get_or_init(tokio::sync::Notify::new)
}

/// Asks the sync loop to run a pass now, as after a project moves to a team.
pub fn request() {
  wake().notify_one();
}

/// Waits `period`, or less when [`request`] is called.
pub async fn wait(period: std::time::Duration) {
  tokio::select! {
    _ = tokio::time::sleep(period) => {}
    _ = wake().notified() => {}
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteProject {
  pub id: String,
  pub org_id: String,
  pub name: String,
  pub repos: Vec<String>,
  #[serde(default)]
  pub prompt: String,
  #[serde(default)]
  pub context_paths: Vec<String>,
}

/// Where team projects are stored.
#[async_trait]
pub trait ProjectTransport: Send + Sync {
  /// Every team project the signed-in user can see.
  async fn list(&self) -> Result<Vec<RemoteProject>, String>;
  async fn upsert(&self, project: &RemoteProject) -> Result<(), String>;
}

/// Maps repos between this machine and the shared table.
pub trait RepoMapper {
  /// The remote reference of a local clone, `None` when it has none.
  fn to_remote(&self, local: &str) -> Option<String>;
  /// The folder on this machine for a remote reference.
  fn to_local(&self, remote: &str) -> Option<String>;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
  pub pushed: usize,
  pub pulled: usize,
  pub dropped: usize,
}

/// One sync pass for a user who belongs to `org_ids`.
pub async fn sync(
  conn: &Connection,
  transport: &dyn ProjectTransport,
  mapper: &dyn RepoMapper,
  org_ids: &[String],
) -> Result<SyncReport, String> {
  let mut report = SyncReport::default();
  let local = projects::list(conn)?;

  for project in &local {
    let Some(org_id) = project.org_id.as_ref().filter(|o| org_ids.contains(o)) else {
      continue;
    };
    let repos: Vec<String> = project
      .repos
      .iter()
      .filter_map(|r| mapper.to_remote(r))
      .collect();
    if repos.is_empty() {
      continue;
    }
    transport
      .upsert(&RemoteProject {
        id: project.id.clone(),
        org_id: org_id.clone(),
        name: project.name.clone(),
        repos,
        prompt: project.prompt.clone(),
        context_paths: project.context_paths.clone(),
      })
      .await?;
    report.pushed += 1;
  }

  for remote in transport.list().await? {
    if !org_ids.contains(&remote.org_id) {
      continue;
    }
    let repos: Vec<String> = remote
      .repos
      .iter()
      .filter_map(|r| mapper.to_local(r))
      .collect();
    if repos.is_empty() {
      continue;
    }
    projects::put_team_project(
      conn,
      &remote.id,
      &remote.org_id,
      &remote.name,
      &repos,
      &remote.prompt,
      &remote.context_paths,
    )?;
    report.pulled += 1;
  }

  for project in local {
    if project
      .org_id
      .as_ref()
      .is_some_and(|o| !org_ids.contains(o))
    {
      projects::delete(conn, &project.id)?;
      report.dropped += 1;
    }
  }
  Ok(report)
}

/// The Supabase table, through RLS as the signed-in user.
pub struct SupabaseTransport(pub Arc<crate::core::supabase::Supabase>);

#[async_trait]
impl ProjectTransport for SupabaseTransport {
  async fn list(&self) -> Result<Vec<RemoteProject>, String> {
    self
      .0
      .select(
        "organization_projects",
        "select=id,org_id,name,repos,prompt,context_paths",
      )
      .await
      .map_err(|e| e.to_string())
  }

  async fn upsert(&self, project: &RemoteProject) -> Result<(), String> {
    let args = json!({
      "p_id": project.id, "p_org_id": project.org_id, "p_name": project.name,
      "p_repos": project.repos, "p_prompt": project.prompt,
      "p_context_paths": project.context_paths,
    });
    self
      .0
      .rpc::<serde_json::Value>("put_organization_project", &args)
      .await
      .map(drop)
      .map_err(|e| e.to_string())
  }
}

/// Maps through each clone's GitHub `origin` and Treq's managed clones root.
pub struct GitMapper {
  /// Local repos already known, checked first so an existing clone is reused.
  pub known: Vec<String>,
  pub clones_root: std::path::PathBuf,
}

impl RepoMapper for GitMapper {
  fn to_remote(&self, local: &str) -> Option<String> {
    crate::github::get_git_remote_url_impl(local)
      .ok()
      .flatten()
      .map(|r| r.full_name)
  }

  fn to_local(&self, remote: &str) -> Option<String> {
    self
      .known
      .iter()
      .find(|l| self.to_remote(l).as_deref() == Some(remote))
      .cloned()
      .or_else(|| {
        projects::managed_clone_path(&self.clones_root, remote)
          .map(|p| p.to_string_lossy().to_string())
      })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;
  use std::sync::Mutex;

  #[derive(Default)]
  struct Fake {
    rows: Mutex<HashMap<String, RemoteProject>>,
  }

  #[async_trait]
  impl ProjectTransport for Fake {
    async fn list(&self) -> Result<Vec<RemoteProject>, String> {
      let mut rows: Vec<_> = self.rows.lock().unwrap().values().cloned().collect();
      rows.sort_by(|a, b| a.id.cmp(&b.id));
      Ok(rows)
    }
    async fn upsert(&self, p: &RemoteProject) -> Result<(), String> {
      self.rows.lock().unwrap().insert(p.id.clone(), p.clone());
      Ok(())
    }
  }

  /// `/local/<name>` ↔ `acme/<name>`.
  struct Map;
  impl RepoMapper for Map {
    fn to_remote(&self, local: &str) -> Option<String> {
      local.strip_prefix("/local/").map(|n| format!("acme/{n}"))
    }
    fn to_local(&self, remote: &str) -> Option<String> {
      remote.strip_prefix("acme/").map(|n| format!("/local/{n}"))
    }
  }

  fn conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    projects::init(&conn).unwrap();
    conn
  }

  fn orgs(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
  }

  fn remote(id: &str, org: &str, repos: &[&str]) -> RemoteProject {
    RemoteProject {
      id: id.into(),
      org_id: org.into(),
      name: format!("R {id}"),
      repos: repos.iter().map(|s| s.to_string()).collect(),
      prompt: "p".into(),
      context_paths: vec![],
    }
  }

  #[tokio::test]
  async fn runner_runs_only_while_signed_in() {
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let runner = Runner::default();
    let start = |runs: Arc<std::sync::atomic::AtomicUsize>| {
      move || async move {
        runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::future::pending::<()>().await;
      }
    };
    assert!(!runner.is_running(), "nothing runs before sign-in");
    runner.signed_in(start(runs.clone()));
    // A token refresh signs in again: still one loop.
    runner.signed_in(start(runs.clone()));
    // The loop runs on the app runtime: give it a moment to start.
    for _ in 0..100 {
      if runs.load(std::sync::atomic::Ordering::SeqCst) > 0 {
        break;
      }
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(runner.is_running());
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
    runner.signed_out();
    assert!(!runner.is_running(), "signing out stops it");
  }

  #[tokio::test]
  async fn request_wakes_a_waiting_sync_early() {
    let waiter = tokio::spawn(wait(std::time::Duration::from_secs(3600)));
    tokio::task::yield_now().await;
    request();
    let woke = tokio::time::timeout(std::time::Duration::from_secs(2), waiter).await;
    assert!(woke.is_ok(), "a request ends the wait");
  }

  #[tokio::test]
  async fn pushes_team_projects_as_remote_refs_and_keeps_personal_local() {
    let conn = conn();
    let fake = Fake::default();
    projects::save(&conn, "Mine", &["/local/solo".into()]).unwrap();
    let team = projects::save(&conn, "Team", &["/local/web".into(), "/tmp/x".into()]).unwrap();
    projects::set_owner(&conn, &team.id, Some("o1")).unwrap();

    let report = sync(&conn, &fake, &Map, &orgs(&["o1"])).await.unwrap();

    let rows = fake.list().await.unwrap();
    assert_eq!(rows.len(), 1, "personal project never pushed");
    assert_eq!(rows[0].id, team.id);
    assert_eq!(
      rows[0].repos,
      vec!["acme/web".to_string()],
      "unmappable repo dropped"
    );
    assert_eq!(report.pushed, 1);
  }

  #[tokio::test]
  async fn pulls_teammate_projects_mapped_to_local_clones() {
    let conn = conn();
    let fake = Fake::default();
    fake
      .upsert(&remote("r1", "o1", &["acme/api"]))
      .await
      .unwrap();

    sync(&conn, &fake, &Map, &orgs(&["o1"])).await.unwrap();

    let got = projects::get(&conn, "r1").unwrap().expect("pulled");
    assert_eq!(got.org_id.as_deref(), Some("o1"));
    assert_eq!(got.repos, vec!["/local/api".to_string()]);
    assert_eq!(got.prompt, "p");
  }

  #[tokio::test]
  async fn pull_updates_an_existing_team_project() {
    let conn = conn();
    let fake = Fake::default();
    fake
      .upsert(&remote("r1", "o1", &["acme/api"]))
      .await
      .unwrap();
    sync(&conn, &fake, &Map, &orgs(&["o1"])).await.unwrap();
    let mut renamed = remote("r1", "o1", &["acme/api", "acme/web"]);
    renamed.name = "Renamed".into();
    fake.upsert(&renamed).await.unwrap();
    // Simulate a teammate's change arriving without a local edit.
    projects::delete(&conn, "r1").unwrap();

    sync(&conn, &fake, &Map, &orgs(&["o1"])).await.unwrap();

    let got = projects::get(&conn, "r1").unwrap().unwrap();
    assert_eq!(got.name, "Renamed");
    assert_eq!(got.repos.len(), 2);
  }

  #[tokio::test]
  async fn drops_projects_of_teams_the_user_left() {
    let conn = conn();
    let fake = Fake::default();
    let p = projects::save(&conn, "Old team", &["/local/web".into()]).unwrap();
    projects::set_owner(&conn, &p.id, Some("gone")).unwrap();

    let report = sync(&conn, &fake, &Map, &orgs(&["o1"])).await.unwrap();

    assert!(projects::get(&conn, &p.id).unwrap().is_none());
    assert_eq!(report.dropped, 1);
    assert!(
      fake.list().await.unwrap().is_empty(),
      "not pushed to a team the user left"
    );
  }
}
