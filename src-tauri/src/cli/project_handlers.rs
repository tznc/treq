//! `treq project`: the saved projects that hold the current repository, so
//! an agent can see and reach every repository of its project. Logic lives in
//! `core::projects`; this file parses and prints.

use super::{detect_repo_path, get_arg_value, Matches, OutputFormat};
use crate::core::{self, projects};
use crate::db::Database;
use std::io::Write;

fn open_db(repo_path: &str) -> Result<Database, String> {
  let db = Database::new(core::resolve_app_db_path(repo_path)).map_err(|e| e.to_string())?;
  db.init().map_err(|e| e.to_string())?;
  Ok(db)
}

/// The one saved project holding the current repository.
fn current_project(db: &Database, repo_path: &str) -> Result<projects::Project, String> {
  let mut found = projects::projects_with_repo(db.conn(), repo_path)?;
  match found.len() {
    0 => Err(format!(
      "not_found: {repo_path} is not in a saved project. Save it from the project menu in the sidebar."
    )),
    1 => Ok(found.remove(0)),
    _ => Err(format!(
      "invalid_arguments: {repo_path} is in several projects ({}); open the window of the one to use",
      found.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
    )),
  }
}

pub(super) fn handle_project_command(
  matches: &Matches,
  format: OutputFormat,
) -> Result<(), String> {
  let action = get_arg_value(matches, "action")
    .ok_or_else(|| "invalid_arguments: project action is required".to_string())?;
  let repo_path = detect_repo_path()?;
  let db = open_db(&repo_path)?;
  let value = get_arg_value(matches, "value");
  let mut out = std::io::stdout().lock();
  run(
    &db,
    &repo_path,
    &action,
    value.as_deref(),
    format,
    &projects::clones_root(),
    &mut out,
  )
}

/// The host name of SSH machine `endpoint_id` when this device saved it;
/// otherwise the id.
fn host_name(db: &Database, endpoint_id: &str) -> String {
  crate::core::remote_records::user_managed_endpoints(db)
    .into_iter()
    .find(|e| e.id == endpoint_id)
    .map_or_else(|| endpoint_id.to_string(), |e| e.hostname)
}

/// A project repository as a person reads it: a local path, or a remote
/// path and its host.
fn repo_label(db: &Database, repo: &str) -> String {
  match projects::remote_repo(repo) {
    Some(remote) => format!("{} on {}", remote.path, host_name(db, &remote.endpoint_id)),
    None => repo.to_string(),
  }
}

fn write_json<T: serde::Serialize>(out: &mut dyn Write, value: &T) -> Result<(), String> {
  let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
  writeln!(out, "{json}").map_err(|e| e.to_string())
}

/// `treq project <action>` for the repository at `repo_path`, printing to
/// `out`. Clones land under `clones_root`.
fn run(
  db: &Database,
  repo_path: &str,
  action: &str,
  value: Option<&str>,
  format: OutputFormat,
  clones_root: &std::path::Path,
  out: &mut dyn Write,
) -> Result<(), String> {
  let line = |out: &mut dyn Write, text: String| writeln!(out, "{text}").map_err(|e| e.to_string());
  match action {
    "list" => {
      let all = projects::list(db.conn())?;
      match format {
        OutputFormat::Json => write_json(out, &all),
        OutputFormat::Human => {
          for p in all {
            let repos: Vec<String> = p.repos.iter().map(|r| repo_label(db, r)).collect();
            line(out, format!("{}  ({})", p.name, repos.join(", ")))?;
          }
          Ok(())
        }
      }
    }
    "status" => {
      let project = current_project(db, repo_path)?;
      let mut repos = projects::overview(&project);
      for remote in repos.iter_mut().filter_map(|r| r.remote.as_mut()) {
        remote.host = host_name(db, &remote.host);
      }
      match format {
        OutputFormat::Json => write_json(
          out,
          &serde_json::json!({
            "project": project.name,
            "prompt": project.prompt,
            "contextPaths": project.context_paths,
            "repos": repos,
          }),
        ),
        OutputFormat::Human => {
          line(out, format!("Project: {}", project.name))?;
          for repo in repos {
            if let Some(remote) = &repo.remote {
              line(out, format!("{} on {} (remote)", remote.path, remote.host))?;
              continue;
            }
            let missing = if repo.exists {
              ""
            } else {
              " (not on this machine)"
            };
            line(out, format!("{}{missing}", repo.path))?;
            for w in repo.workspaces {
              line(out, format!("  {}  {}", w.branch, w.title))?;
            }
          }
          Ok(())
        }
      }
    }
    "add-repo" => {
      let spec = value.ok_or_else(|| {
        "invalid_arguments: add-repo needs a folder, URL or owner/name".to_string()
      })?;
      let project = current_project(db, repo_path)?;
      let (path, cloned) = projects::ensure_repo(spec, clones_root)?;
      core::init(&path).map_err(|e| e.to_string())?;
      let added = projects::add_repo(db.conn(), &project.id, &path)?;
      match format {
        OutputFormat::Json => write_json(
          out,
          &serde_json::json!({
            "project": project.name, "path": path, "cloned": cloned,
            "status": if added { "added" } else { "already_present" },
          }),
        ),
        OutputFormat::Human if !added => line(out, format!("Already in {}: {path}", project.name)),
        OutputFormat::Human => {
          let how = if cloned { "Cloned and added" } else { "Added" };
          line(out, format!("{how} {path} to {}", project.name))
        }
      }
    }
    other => Err(format!(
      "invalid_arguments: unknown project action '{other}' (list, status, add-repo)"
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_repo::TestRepo;
  use serde_json::Value;
  use tempfile::TempDir;

  struct Env {
    _dir: TempDir,
    db: Database,
    clones: TempDir,
  }

  fn env() -> Env {
    let dir = TempDir::new().unwrap();
    let db = Database::new(dir.path().join("app.db")).unwrap();
    db.init().unwrap();
    Env {
      _dir: dir,
      db,
      clones: TempDir::new().unwrap(),
    }
  }

  impl Env {
    fn run(
      &self,
      repo: &str,
      action: &str,
      value: Option<&str>,
      format: OutputFormat,
    ) -> Result<String, String> {
      let mut out = Vec::new();
      run(
        &self.db,
        repo,
        action,
        value,
        format,
        self.clones.path(),
        &mut out,
      )?;
      Ok(String::from_utf8(out).unwrap())
    }

    fn json(&self, repo: &str, action: &str, value: Option<&str>) -> Value {
      serde_json::from_str(&self.run(repo, action, value, OutputFormat::Json).unwrap()).unwrap()
    }

    fn save(&self, name: &str, repos: &[&str]) -> projects::Project {
      let repos: Vec<String> = repos.iter().map(|r| r.to_string()).collect();
      projects::save(self.db.conn(), name, &repos).unwrap()
    }
  }

  #[test]
  fn list_prints_empty_json_array_without_projects() {
    let env = env();
    assert_eq!(env.json("/r/web", "list", None), serde_json::json!([]));
  }

  #[test]
  fn list_prints_nothing_human_without_projects() {
    let env = env();
    assert_eq!(
      env
        .run("/r/web", "list", None, OutputFormat::Human)
        .unwrap(),
      ""
    );
  }

  #[test]
  fn list_prints_each_project_with_its_repos() {
    let env = env();
    env.save("Checkout", &["/r/web", "/r/api"]);
    let human = env.run("/r/x", "list", None, OutputFormat::Human).unwrap();
    assert_eq!(human, "Checkout  (/r/web, /r/api)\n");
    let json = env.json("/r/x", "list", None);
    assert_eq!(json[0]["name"], "Checkout");
    assert_eq!(json[0]["repos"], serde_json::json!(["/r/web", "/r/api"]));
  }

  #[test]
  fn status_fails_not_found_outside_a_project() {
    let env = env();
    env.save("Other", &["/r/other"]);
    let err = env
      .run("/r/web", "status", None, OutputFormat::Json)
      .unwrap_err();
    assert!(err.starts_with("not_found:"), "{err}");
    assert!(err.contains("/r/web"));
  }

  #[test]
  fn status_fails_invalid_arguments_when_repo_is_in_several_projects() {
    let env = env();
    env.save("One", &["/r/web"]);
    env.save("Two", &["/r/web", "/r/api"]);
    let err = env
      .run("/r/web", "status", None, OutputFormat::Human)
      .unwrap_err();
    assert!(err.starts_with("invalid_arguments:"), "{err}");
    assert!(err.contains("One") && err.contains("Two"), "{err}");
  }

  #[test]
  fn status_json_reports_project_context_and_repos() {
    let env = env();
    let repo = TestRepo::new().unwrap();
    let p = env.save("Checkout", &[&repo.repo_path, "/definitely/missing"]);
    projects::set_context(env.db.conn(), &p.id, "Use npm", &["/docs/a.md".to_string()]).unwrap();
    let json = env.json(&repo.repo_path, "status", None);
    assert_eq!(json["project"], "Checkout");
    assert_eq!(json["prompt"], "Use npm");
    assert_eq!(json["contextPaths"], serde_json::json!(["/docs/a.md"]));
    assert_eq!(json["repos"][0]["path"], repo.repo_path.as_str());
    assert_eq!(json["repos"][0]["exists"], true);
    assert_eq!(json["repos"][1]["exists"], false);
  }

  #[test]
  fn status_human_marks_repos_not_on_this_machine() {
    let env = env();
    let repo = TestRepo::new().unwrap();
    env.save("Checkout", &[&repo.repo_path, "/definitely/missing"]);
    let human = env
      .run(&repo.repo_path, "status", None, OutputFormat::Human)
      .unwrap();
    let lines: Vec<&str> = human.lines().collect();
    assert_eq!(lines[0], "Project: Checkout");
    assert_eq!(lines[1], repo.repo_path);
    assert_eq!(lines[2], "/definitely/missing (not on this machine)");
  }

  #[test]
  fn add_repo_requires_a_value() {
    let env = env();
    env.save("Checkout", &["/r/web"]);
    let err = env
      .run("/r/web", "add-repo", None, OutputFormat::Json)
      .unwrap_err();
    assert!(err.starts_with("invalid_arguments:"), "{err}");
  }

  #[test]
  fn add_repo_fails_not_found_outside_a_project() {
    let env = env();
    let other = TestRepo::new().unwrap();
    let err = env
      .run(
        "/r/web",
        "add-repo",
        Some(&other.repo_path),
        OutputFormat::Json,
      )
      .unwrap_err();
    assert!(err.starts_with("not_found:"), "{err}");
  }

  #[test]
  fn add_repo_adds_a_local_folder_once() {
    let env = env();
    let p = env.save("Checkout", &["/r/web"]);
    let other = TestRepo::new().unwrap();
    let canonical = std::fs::canonicalize(&other.repo_path)
      .unwrap()
      .to_string_lossy()
      .to_string();
    let json = env.json("/r/web", "add-repo", Some(&other.repo_path));
    assert_eq!(json["project"], "Checkout");
    assert_eq!(json["path"], canonical.as_str());
    assert_eq!(json["cloned"], false);
    assert_eq!(json["status"], "added");
    // A second add is accepted, says so, and keeps one entry.
    let again = env.json("/r/web", "add-repo", Some(&other.repo_path));
    assert_eq!(again["status"], "already_present");
    let human = env
      .run(
        "/r/web",
        "add-repo",
        Some(&other.repo_path),
        OutputFormat::Human,
      )
      .unwrap();
    assert_eq!(human, format!("Already in Checkout: {canonical}\n"));
    let repos = projects::get(env.db.conn(), &p.id).unwrap().unwrap().repos;
    assert_eq!(repos, vec!["/r/web".to_string(), canonical]);
  }

  #[test]
  fn status_and_list_show_a_remote_repository_with_its_host() {
    let env = env();
    let local = TestRepo::new().unwrap();
    env.save("Checkout", &[&local.repo_path, "ssh:ep-1:gen0:/srv/app"]);
    let status = env
      .run(&local.repo_path, "status", None, OutputFormat::Human)
      .unwrap();
    assert!(status.contains("/srv/app on ep-1 (remote)"), "{status}");
    assert!(!status.contains("not on this machine"), "{status}");
    let json = env.json(&local.repo_path, "status", None);
    assert_eq!(json["repos"][1]["remote"]["host"], "ep-1");
    assert_eq!(json["repos"][1]["remote"]["path"], "/srv/app");
    let list = env
      .run(&local.repo_path, "list", None, OutputFormat::Human)
      .unwrap();
    assert!(list.contains("/srv/app on ep-1"), "{list}");
  }

  #[test]
  fn add_repo_rejects_what_is_not_a_folder_or_url() {
    let env = env();
    env.save("Checkout", &["/r/web"]);
    assert!(env
      .run("/r/web", "add-repo", Some("not a repo"), OutputFormat::Json)
      .is_err());
  }

  #[test]
  fn rejects_unknown_action() {
    let env = env();
    let err = env
      .run("/r/web", "remove", None, OutputFormat::Json)
      .unwrap_err();
    assert!(err.starts_with("invalid_arguments:"), "{err}");
    assert!(err.contains("'remove'"));
  }
}
