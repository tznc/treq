//! The sidebar a window draws, from its model and what it read:
//! `useGpuiSidebar` and `buildGpuiSidebarSnapshot`, with
//! `getWorkspaceStatusIndicator` for each row's dot.

use std::collections::HashMap;

use super::{
  data::Data,
  model::{Model, Page, TerminalSummary},
  tree,
};
use crate::{
  github::PrInfo,
  gpui_shell::screen::display_title,
  gpui_shell::sidebar::{
    PrState, RepoSection, RowStatus, SidebarRow, SidebarSnapshot, StatusColor,
  },
  local_db::Workspace,
};

/// The workspace's title for its sidebar row, or `None` to show the branch:
/// a blank title, or one that repeats the branch, adds nothing.
pub fn row_name(workspace: &Workspace) -> Option<String> {
  let title = display_title(workspace);
  let title = title.trim();
  (!title.is_empty() && title != workspace.branch_name).then(|| title.to_string())
}

/// `getWorkspaceStatusIndicator`: red for conflicts, yellow for changes,
/// spinning while an agent in the workspace streams.
pub fn status_indicator(conflicted: bool, has_changes: bool, streaming: bool) -> Option<RowStatus> {
  let color = if conflicted {
    StatusColor::Red
  } else if has_changes {
    StatusColor::Yellow
  } else if streaming {
    StatusColor::Neutral
  } else {
    return None;
  };
  Some(RowStatus {
    color,
    spin: streaming,
  })
}

fn pr_state(pr: &PrInfo) -> PrState {
  match pr.state.to_uppercase().as_str() {
    "OPEN" if pr.is_draft => PrState::Draft,
    "OPEN" => PrState::Open,
    "MERGED" => PrState::Merged,
    _ => PrState::Closed,
  }
}

/// `repoDisplayLabels`: each repository's directory name, or `parent/name`
/// where two share a name.
pub fn display_labels(paths: &[String]) -> HashMap<String, String> {
  let parts = |path: &str| -> Vec<String> {
    path
      .split(['/', '\\'])
      .filter(|p| !p.is_empty())
      .map(str::to_string)
      .collect()
  };
  let mut counts: HashMap<String, usize> = HashMap::new();
  for path in paths {
    let name = parts(path).last().cloned().unwrap_or_else(|| path.clone());
    *counts.entry(name).or_default() += 1;
  }
  paths
    .iter()
    .map(|path| {
      let parts = parts(path);
      let name = parts.last().cloned().unwrap_or_else(|| path.clone());
      let label = if counts.get(&name).copied().unwrap_or(0) > 1 && parts.len() > 1 {
        parts[parts.len() - 2..].join("/")
      } else {
        name
      };
      (path.clone(), label)
    })
    .collect()
}

/// The repositories the sidebar lists, the main one first; one alone for a
/// window without supporting repositories.
pub fn order(model: &Model, data: &Data) -> Vec<String> {
  if data.supporting().is_empty() {
    return vec![model.repo().to_string()];
  }
  std::iter::once(model.main_repo.clone())
    .chain(data.supporting().iter().map(|r| r.path.clone()))
    .collect()
}

/// The sidebar's rows of repository `path`, in display order.
pub fn nodes(data: &Data, path: &str) -> Vec<tree::Node> {
  let Some(repo) = data.repo(path) else {
    return Vec::new();
  };
  let rows = tree::rows(
    repo.workspaces.as_deref().unwrap_or_default(),
    &repo.statuses,
    &repo.has_changes,
  );
  tree::flatten(&rows, |branch| {
    repo
      .prs
      .get(branch)
      .and_then(Option::as_ref)
      .is_some_and(|pr| pr.state == "MERGED")
  })
}

/// The workspaces of the repository on screen, in the sidebar's order
/// (`visibleWorkspaces`, the rows Shift ranges run over).
pub fn visible(data: &Data, path: &str) -> Vec<Workspace> {
  nodes(data, path).into_iter().map(|n| n.workspace).collect()
}

/// Agents carry their repository; shells belong to the one on screen.
fn streaming_by_branch(
  terminals: &[TerminalSummary],
  path: &str,
  on_screen: &str,
  multi: bool,
) -> HashMap<String, bool> {
  let mut branches = HashMap::new();
  for summary in terminals {
    let ours = !multi
      || match &summary.repo_path {
        Some(repo) => repo == path,
        None => path == on_screen,
      };
    if !ours || !summary.agent {
      continue;
    }
    let Some(branch) = &summary.branch else {
      continue;
    };
    let entry = branches.entry(branch.clone()).or_insert(false);
    *entry |= summary.streaming;
  }
  branches
}

/// `buildGpuiSidebarSnapshot` for the window, or `None` with no
/// repository open.
pub fn snapshot(model: &Model, data: &Data) -> Option<SidebarSnapshot> {
  if !model.has_repo() {
    return None;
  }
  let on_screen = model.repo().to_string();
  let multi = !data.supporting().is_empty();
  let order = order(model, data);
  let labels = multi.then(|| display_labels(&order));
  let page_selects = !matches!(
    model.page,
    Page::Github(_) | Page::Linear | Page::Settings { .. }
  );
  let repos = order
    .iter()
    .map(|path| {
      let missing = data
        .supporting()
        .iter()
        .any(|r| &r.path == path && !r.exists);
      let active = *path == on_screen;
      let repo = data.repo(path);
      let streaming = streaming_by_branch(&model.terminals, path, &on_screen, multi);
      let has_remote = repo.is_some_and(|r| r.remote.is_some());
      let workspaces = if missing {
        Vec::new()
      } else {
        nodes(data, path)
          .into_iter()
          .map(|node| {
            let workspace = &node.workspace;
            let branch = &workspace.branch_name;
            let selected = active
              && (model.multi.contains(&workspace.id)
                || (page_selects && model.selected == Some(workspace.id)));
            let pr = repo
              .and_then(|r| r.prs.get(branch))
              .and_then(Option::as_ref)
              .filter(|_| has_remote)
              .map(pr_state);
            SidebarRow {
              id: workspace.id,
              title: branch.clone(),
              name: row_name(workspace),
              depth: node.depth,
              selected,
              conflicted: node.conflicted,
              status: status_indicator(
                node.conflicted,
                node.has_changes,
                streaming.get(branch).copied().unwrap_or(false),
              ),
              pr,
              queue: None,
              archiving: false,
              exiting: false,
            }
          })
          .collect()
      };
      let loaded = data.loaded(path);
      RepoSection {
        path: path.clone(),
        // The React sidebar shows the branch only for the repository on screen.
        home_ref: if active { data.home_ref(path) } else { None },
        home_selected: page_selects && active && model.selected.is_none(),
        active,
        loading: !missing && !loaded && workspaces.is_empty(),
        workspaces,
        label: labels.as_ref().and_then(|l| l.get(path).cloned()),
        missing,
        collapsed: multi && !missing && model.collapsed.contains(path),
      }
    })
    .collect();
  let main = if model.main_repo.is_empty() {
    on_screen
  } else {
    model.main_repo.clone()
  };
  Some(SidebarSnapshot {
    repo_name: main
      .split('/')
      .rfind(|p| !p.is_empty())
      .unwrap_or("Repository")
      .to_string(),
    settings_open: matches!(model.page, Page::Settings { .. }),
    github: matches!(model.page, Page::Github(_)),
    linear: data
      .flags
      .linear
      .then_some(matches!(model.page, Page::Linear)),
    dark: model.prefs.dark(),
    width: model.prefs.sidebar_width,
    agent_unavailable: None,
    shell_unavailable: None,
    bulk_archive: (!model.multi.is_empty()).then_some(model.multi.len() as u32),
    repos,
    modal: None,
  })
}

#[cfg(test)]
mod tests {
  use super::{
    super::{data::RepoRead, tree::tests::workspace},
    *,
  };
  use crate::core::WorkspaceSidebarStatus;

  fn data_with(workspaces: Vec<Workspace>) -> Data {
    let mut data = Data::default();
    let statuses = workspaces
      .iter()
      .map(|w| WorkspaceSidebarStatus {
        current: w.clone(),
        has_conflicts: w.id == 2,
        last_activity_at: None,
      })
      .collect();
    data.apply(
      "/repo",
      RepoRead {
        workspaces: Some(workspaces),
        statuses: Some(statuses),
        has_changes: Some(HashMap::from([(1, true)])),
        ..RepoRead::default()
      },
    );
    data
  }

  #[test]
  fn indicator_matches_react() {
    assert_eq!(status_indicator(false, false, false), None);
    let red = status_indicator(true, true, true).unwrap();
    assert_eq!((red.color, red.spin), (StatusColor::Red, true));
    let yellow = status_indicator(false, true, false).unwrap();
    assert_eq!((yellow.color, yellow.spin), (StatusColor::Yellow, false));
    let neutral = status_indicator(false, false, true).unwrap();
    assert_eq!((neutral.color, neutral.spin), (StatusColor::Neutral, true));
  }

  #[test]
  fn labels_repositories_that_share_a_name_with_their_parent() {
    let labels = display_labels(&["/a/app".into(), "/b/app".into(), "/c/lib".into()]);
    assert_eq!(labels["/a/app"], "a/app");
    assert_eq!(labels["/b/app"], "b/app");
    assert_eq!(labels["/c/lib"], "lib");
  }

  #[test]
  fn builds_rows_with_selection_and_status() {
    let data = data_with(vec![
      workspace(1, "login", Some("main")),
      workspace(2, "fix", Some("login")),
    ]);
    let mut model = Model::new(Some("/repo".into()));
    model.selected = Some(2);
    model.terminals = vec![TerminalSummary {
      agent: true,
      branch: Some("login".into()),
      repo_path: Some("/repo".into()),
      streaming: true,
    }];
    let snapshot = snapshot(&model, &data).unwrap();
    assert_eq!(snapshot.repo_name, "repo");
    assert_eq!(snapshot.width, 280.);
    let repo = &snapshot.repos[0];
    assert!(repo.active && !repo.home_selected && !repo.loading);
    assert_eq!(repo.label, None, "one repository has no label");
    let rows: Vec<_> = repo
      .workspaces
      .iter()
      .map(|r| (r.id, r.depth, r.selected))
      .collect();
    assert_eq!(rows, [(1, 0, false), (2, 1, true)]);
    let login = repo.workspaces[0].status.as_ref().unwrap();
    assert_eq!((login.color, login.spin), (StatusColor::Yellow, true));
    let fix = repo.workspaces[1].status.as_ref().unwrap();
    assert_eq!(fix.color, StatusColor::Red);
    assert_eq!(snapshot.bulk_archive, None);
  }

  #[test]
  fn other_pages_deselect_the_workspace_but_keep_the_multi_selection() {
    let data = data_with(vec![workspace(1, "a", None), workspace(3, "b", None)]);
    let mut model = Model::new(Some("/repo".into()));
    model.selected = Some(1);
    model.multi.insert(3);
    model.open_settings();
    let snapshot = snapshot(&model, &data).unwrap();
    assert!(snapshot.settings_open);
    let selected: Vec<_> = snapshot.repos[0]
      .workspaces
      .iter()
      .filter(|r| r.selected)
      .map(|r| r.id)
      .collect();
    assert_eq!(selected, [3]);
    assert_eq!(snapshot.bulk_archive, Some(1));
  }

  #[test]
  fn names_a_row_by_its_title_and_falls_back_to_the_branch() {
    let mut ws = workspace(1, "zn/blissful-shannon", None);
    assert_eq!(row_name(&ws), None, "no title shows the branch");
    ws.title = "  ".into();
    assert_eq!(row_name(&ws), None, "a blank title shows the branch");
    ws.title = "zn/blissful-shannon".into();
    assert_eq!(
      row_name(&ws),
      None,
      "a title that repeats the branch adds nothing"
    );
    ws.title = " feat(spool): fail readiness ".into();
    assert_eq!(
      row_name(&ws).as_deref(),
      Some("feat(spool): fail readiness")
    );
    ws.title = String::new();
    ws.metadata = Some(r#"{"title":"From the PR"}"#.into());
    assert_eq!(
      row_name(&ws).as_deref(),
      Some("From the PR"),
      "the metadata's title"
    );
  }

  #[test]
  fn rows_carry_the_workspace_title() {
    let mut titled = workspace(1, "login", None);
    titled.title = "Add login".into();
    let data = data_with(vec![titled, workspace(3, "fix", None)]);
    let snapshot = snapshot(&Model::new(Some("/repo".into())), &data).unwrap();
    let rows: Vec<_> = snapshot.repos[0]
      .workspaces
      .iter()
      .map(|r| (r.title.as_str(), r.name.as_deref()))
      .collect();
    assert_eq!(rows, [("fix", None), ("login", Some("Add login"))]);
  }

  #[test]
  fn waits_for_the_first_read() {
    let model = Model::new(Some("/repo".into()));
    let snapshot = snapshot(&model, &Data::default()).unwrap();
    assert!(snapshot.repos[0].loading);
    assert!(super::snapshot(&Model::new(None), &Data::default()).is_none());
  }
}
