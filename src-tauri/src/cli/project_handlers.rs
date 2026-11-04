//! `treq project`: the saved projects that hold the current repository, so
//! an agent can see and reach every repository of its project. Logic lives in
//! `core::projects`; this file parses and prints.

use super::{detect_repo_path, get_arg_value, print_json, Matches, OutputFormat};
use crate::core::{self, projects};
use crate::db::Database;

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
  match action.as_str() {
    "list" => {
      let all = projects::list(db.conn())?;
      match format {
        OutputFormat::Json => print_json(&all),
        OutputFormat::Human => {
          for p in all {
            println!("{}  ({})", p.name, p.repos.join(", "));
          }
          Ok(())
        }
      }
    }
    "status" => {
      let project = current_project(&db, &repo_path)?;
      let repos = projects::overview(&project);
      match format {
        OutputFormat::Json => print_json(&serde_json::json!({
          "project": project.name,
          "prompt": project.prompt,
          "contextPaths": project.context_paths,
          "repos": repos,
        })),
        OutputFormat::Human => {
          println!("Project: {}", project.name);
          for repo in repos {
            let missing = if repo.exists {
              ""
            } else {
              " (not on this machine)"
            };
            println!("{}{missing}", repo.path);
            for w in repo.workspaces {
              println!("  {}  {}", w.branch, w.title);
            }
          }
          Ok(())
        }
      }
    }
    "add-repo" => {
      let spec = get_arg_value(matches, "value").ok_or_else(|| {
        "invalid_arguments: add-repo needs a folder, URL or owner/name".to_string()
      })?;
      let project = current_project(&db, &repo_path)?;
      let (path, cloned) = projects::ensure_repo(&spec, &projects::clones_root())?;
      core::init(&path).map_err(|e| e.to_string())?;
      projects::add_repo(db.conn(), &project.id, &path)?;
      match format {
        OutputFormat::Json => print_json(&serde_json::json!({
          "project": project.name, "path": path, "cloned": cloned,
        })),
        OutputFormat::Human => {
          let how = if cloned { "Cloned and added" } else { "Added" };
          println!("{how} {path} to {}", project.name);
          Ok(())
        }
      }
    }
    other => Err(format!(
      "invalid_arguments: unknown project action '{other}' (list, status, add-repo)"
    )),
  }
}
