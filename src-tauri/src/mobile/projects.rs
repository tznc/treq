//! Projects on mobile: the user's personal and team projects, read from
//! `public.organization_projects`, grouped by owner for the home page, and
//! opened on a machine Treq picks from what is in use.

use serde::Deserialize;

/// A project as the server stores it: repos are remote references
/// (`owner/name` or a clone URL), not paths on any machine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Project {
  pub id: String,
  pub name: String,
  /// The owning Team organization; `None` for personal.
  #[serde(default)]
  pub org_id: Option<String>,
  #[serde(default)]
  pub repos: Vec<String>,
}

/// One home-page section: a heading and its projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
  pub title: String,
  pub projects: Vec<Project>,
}

/// Personal first, then one section per team in `teams` order (id, name).
/// Empty sections are left out; a team project whose team is unknown goes
/// under "Team".
pub fn group(projects: &[Project], teams: &[(String, String)]) -> Vec<Section> {
  let mut sections = vec![Section {
    title: "Personal".into(),
    projects: projects
      .iter()
      .filter(|p| p.org_id.is_none())
      .cloned()
      .collect(),
  }];
  for (id, name) in teams {
    sections.push(Section {
      title: name.clone(),
      projects: projects
        .iter()
        .filter(|p| p.org_id.as_deref() == Some(id.as_str()))
        .cloned()
        .collect(),
    });
  }
  sections.push(Section {
    title: "Team".into(),
    projects: projects
      .iter()
      .filter(|p| {
        p.org_id
          .as_ref()
          .is_some_and(|o| !teams.iter().any(|(id, _)| id == o))
      })
      .cloned()
      .collect(),
  });
  sections.retain(|s| !s.projects.is_empty());
  sections
}

/// What Treq knows about one machine when picking where to open a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
  pub id: String,
  /// Connected and checked.
  pub ready: bool,
}

/// The machine to open a project on: the active connection if it is ready,
/// else any ready machine, else the last one used, else the first
/// candidate (the cloud machine is listed first when signed in).
pub fn pick_machine(
  candidates: &[Candidate],
  active: Option<&str>,
  last_used: Option<&str>,
) -> Option<String> {
  let ready = |id: &str| candidates.iter().any(|c| c.id == id && c.ready);
  let known = |id: &str| candidates.iter().any(|c| c.id == id);
  active
    .filter(|id| ready(id))
    .map(str::to_string)
    .or_else(|| candidates.iter().find(|c| c.ready).map(|c| c.id.clone()))
    .or_else(|| last_used.filter(|id| known(id)).map(str::to_string))
    .or_else(|| candidates.first().map(|c| c.id.clone()))
}

/// Where a project repo lives on a machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
  /// A repository already saved on the machine, at this path.
  Existing(String),
  /// Not there yet: clone `url` into `destination`.
  Clone { url: String, destination: String },
}

/// Where Treq clones repositories on a remote machine.
pub const REMOTE_CLONES_ROOT: &str = "~/.treq/repos";

/// Finds `remote` (`owner/name` or a clone URL) among the machine's saved
/// repository paths by their last two folders, else plans a clone under
/// [`REMOTE_CLONES_ROOT`].
pub fn locate(remote: &str, saved: &[String]) -> Option<Target> {
  let url = crate::core::projects::clone_url(remote);
  let destination =
    crate::core::projects::managed_clone_path(std::path::Path::new(REMOTE_CLONES_ROOT), &url)?;
  let destination = destination.to_string_lossy().replace('\\', "/");
  // owner/name: the last two folders of the planned clone.
  let tail: Vec<&str> = destination.rsplitn(3, '/').take(2).collect();
  let found = saved.iter().find(|path| {
    let mut parts = path.trim_end_matches('/').rsplit('/');
    parts.next() == Some(tail[0]) && parts.next() == Some(tail[1])
  });
  Some(match found {
    Some(path) => Target::Existing(path.clone()),
    None => Target::Clone { url, destination },
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn project(id: &str, org: Option<&str>) -> Project {
    Project {
      id: id.into(),
      name: format!("P {id}"),
      org_id: org.map(Into::into),
      repos: vec!["acme/web".into()],
    }
  }

  fn teams() -> Vec<(String, String)> {
    vec![("o1".into(), "Acme".into()), ("o2".into(), "Globex".into())]
  }

  #[test]
  fn groups_personal_first_then_each_team_in_order() {
    let sections = group(
      &[
        project("g", Some("o2")),
        project("a", Some("o1")),
        project("p", None),
      ],
      &teams(),
    );
    let shape: Vec<(&str, Vec<&str>)> = sections
      .iter()
      .map(|s| {
        (
          s.title.as_str(),
          s.projects.iter().map(|p| p.id.as_str()).collect(),
        )
      })
      .collect();
    assert_eq!(
      shape,
      [
        ("Personal", vec!["p"]),
        ("Acme", vec!["a"]),
        ("Globex", vec!["g"])
      ]
    );
  }

  #[test]
  fn leaves_out_empty_sections_and_names_unknown_teams() {
    let sections = group(&[project("x", Some("gone"))], &teams());
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].title, "Team");
  }

  fn c(id: &str, ready: bool) -> Candidate {
    Candidate {
      id: id.into(),
      ready,
    }
  }

  #[test]
  fn picks_the_active_ready_machine_first() {
    let all = [c("managed", true), c("h1", true)];
    assert_eq!(
      pick_machine(&all, Some("h1"), Some("managed")).as_deref(),
      Some("h1")
    );
  }

  #[test]
  fn picks_any_ready_machine_before_the_last_used() {
    let all = [c("managed", false), c("h1", true)];
    assert_eq!(
      pick_machine(&all, None, Some("managed")).as_deref(),
      Some("h1")
    );
  }

  #[test]
  fn falls_back_to_the_last_used_then_the_first() {
    let all = [c("managed", false), c("h1", false)];
    assert_eq!(pick_machine(&all, None, Some("h1")).as_deref(), Some("h1"));
    assert_eq!(
      pick_machine(&all, None, Some("gone")).as_deref(),
      Some("managed")
    );
    assert_eq!(pick_machine(&[], None, None), None);
  }

  #[test]
  fn locates_a_saved_clone_by_owner_and_name() {
    let saved = vec![
      "/home/me/code/other".to_string(),
      "/home/me/src/acme/web".to_string(),
    ];
    assert_eq!(
      locate("acme/web", &saved),
      Some(Target::Existing("/home/me/src/acme/web".into()))
    );
    assert_eq!(
      locate("git@github.com:acme/web.git", &saved),
      Some(Target::Existing("/home/me/src/acme/web".into()))
    );
  }

  #[test]
  fn plans_a_clone_when_the_machine_has_none() {
    assert_eq!(
      locate("acme/api", &[]),
      Some(Target::Clone {
        url: "https://github.com/acme/api.git".into(),
        destination: "~/.treq/repos/github.com/acme/api".into(),
      })
    );
  }
}
