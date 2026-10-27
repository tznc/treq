//! The screen, dialog and overlay a window shows, built from its model as
//! Dashboard built them (`useGpuiWorkspaceScreen`, `useGpuiAgentModals`,
//! `useGpuiWorkspaceDialogs`, `useGpuiOverlays`).
//!
//! Each is written as the JSON React pushed and read back into the typed
//! snapshot, so the screens see exactly what React would have sent.

use serde_json::{json, Value};

use super::{
  data::Data,
  model::{full_path, GithubTab, Model, Page, Tab},
  remote::{self, MERGE_REASON},
  sidebar,
};
use crate::{
  gpui_shell::{screen::Modal, OverlaySnapshot, ScreenSnapshot},
  local_db::Workspace,
};

/// `HOME_MOVE_ENDPOINT`: the home repository as a move target.
const HOME_MOVE_ENDPOINT: &str = ".";

fn read<T: serde::de::DeserializeOwned>(value: Value, what: &str) -> Option<T> {
  match serde_json::from_value(value) {
    Ok(snapshot) => Some(snapshot),
    Err(err) => {
      log::warn!("failed to build the window's {what}: {err}");
      None
    }
  }
}

/// The screen the window shows.
pub fn screen(model: &Model, data: &Data) -> Option<ScreenSnapshot> {
  let mut value = view(model, data);
  value["dark"] = model.prefs.dark().into();
  if let Some(modal) = workspace_modal(model, data) {
    value["modal"] = modal;
  }
  value["remote"] = remote_dialogs(model, data);
  if let Some(repository) = &model.remote.repository {
    value["repository"] = serde_json::to_value(repository).unwrap_or_default();
  }
  read(value, "screen")
}

/// The remote dialogs open over the screen (`gpuiRemoteDialogs`).
fn remote_dialogs(model: &Model, data: &Data) -> Value {
  let remote = &model.remote;
  let setup = remote.setup.then(|| {
    json!({
      "allowUserManaged": data.flags.remote_ssh,
      "screen": remote.setup_screen,
      "registering": remote.registering,
    })
  });
  // The terminal dialog needs a connected endpoint and an open repository.
  let terminal = remote
    .endpoint
    .as_ref()
    .zip(remote.repository.as_ref())
    .filter(|_| remote.terminal)
    .map(|(endpoint, repository)| {
      let workspace = model.selected_workspace(data);
      let (workspace_id, _) = remote::terminal_target(
        &repository.canonical_path,
        workspace.map(|w| (w.workspace_name.as_str(), "")),
      );
      json!({
        "endpoint": endpoint,
        "repositoryId": repository.canonical_path,
        "workspaceId": workspace_id,
      })
    });
  json!({
    "setup": setup,
    "terminal": terminal,
    "ambiguous": remote.ambiguous,
  })
}

/// `RemoteStatusBanner`, the strip and the capability notice around a
/// remote repository's workspace view.
fn remote_chrome(model: &Model) -> Value {
  let remote = &model.remote;
  let Some(repository) = &remote.repository else {
    return Value::Null;
  };
  let cut_off = remote.cutoff().is_some();
  let state = if cut_off {
    "cutoff"
  } else {
    remote::connection_state(remote.instance_state.as_deref(), remote.endpoint.is_some())
  };
  json!({
    "displayName": repository.display_name,
    "terminal": remote.endpoint.is_some(),
    "state": state,
    "cutOff": cut_off,
    "notices": [MERGE_REASON],
  })
}

/// Which page shows, as `GpuiScreenView`.
fn view(model: &Model, data: &Data) -> Value {
  if !model.has_repo() {
    // A skill link still asks to install app-wide, which needs no repository.
    if let (Page::Settings { tab }, Some(skill)) = (&model.page, &model.skill_install) {
      return json!({
        "kind": "settings",
        "tab": tab,
        "repoPath": null,
        "currentBranch": null,
        "installRequest": skill,
      });
    }
    return json!({"kind": "onboarding", "remoteRepo": model.remote.form_json()});
  }
  let repo = model.repo();
  match &model.page {
    Page::Settings { tab } => json!({
      "kind": "settings",
      "tab": tab,
      "repoPath": repo,
      "currentBranch": model.default_branch(data),
      "installRequest": model.skill_install,
    }),
    Page::Github(github) => match github.review {
      Some(number) => json!({"kind": "prReview", "repoPath": repo, "prNumber": number}),
      None => json!({
        "kind": "github",
        "repoPath": repo,
        "tab": if github.tab == GithubTab::Issues { "issues" } else { "prs" },
        "filter": github.filter,
        "selected": github.selected,
        "creating": github.creating,
        "runningChecks": [],
        "reload": github.reload,
      }),
    },
    Page::Linear if data.flags.linear => json!({"kind": "linear", "repoPath": repo}),
    Page::MergePreview { workspace_id } => {
      match data.workspaces(repo).iter().find(|w| w.id == *workspace_id) {
        Some(workspace) => json!({
          "kind": "mergePreview",
          "repoPath": workspace.repo_path,
          "workspaceId": workspace.id,
        }),
        None => workspace_view(model, data),
      }
    }
    _ => workspace_view(model, data),
  }
}

/// `ShowWorkspace`'s route, with GPUI drawing every tab and the terminal.
fn workspace_view(model: &Model, data: &Data) -> Value {
  let repo = model.repo();
  let workspace = model.selected_workspace(data);
  let home_branch = model.default_branch(data);
  let view = &model.view;
  // `defaultTargetBranch`: the home branch, which has nothing committed.
  let committed_toggle = workspace.is_some_and(|w| w.branch_name != home_branch);
  let source = workspace.map_or(HOME_MOVE_ENDPOINT, |w| w.branch_name.as_str());
  let move_targets: Vec<Value> = std::iter::once(json!({
    "branch": HOME_MOVE_ENDPOINT,
    "label": data.home_ref(repo).filter(|r| !r.is_empty()).unwrap_or_else(|| "home".into()),
  }))
  .chain(
    data
      .workspaces(repo)
      .iter()
      .map(|w| json!({"branch": w.branch_name, "label": w.branch_name})),
  )
  .filter(|t| t["branch"] != source)
  .collect();
  let file_browser = view
    .file_browser
    .as_ref()
    .filter(|_| view.tab == Tab::Overview)
    .map(|b| json!({"selectedFile": b.selected_file, "expandedDir": b.expanded_dir}));
  json!({
    "kind": "workspace",
    "repoPath": repo,
    "workspaceId": workspace.map(|w| w.id),
    "homeBranch": home_branch,
    "tab": view.tab.value(),
    "retargeting": false,
    "creatingPr": false,
    "runningChecks": [],
    "runningWorkflows": [],
    "setupRerunning": false,
    "revision": data.revision,
    "scrollToCommit": view.scroll_to_commit.as_ref().filter(|_| view.tab == Tab::Commits),
    "showCommitted": committed_toggle.then_some(view.show_committed),
    "moveTargets": move_targets,
    "changesRevision": view.changes_revision,
    "logsSource": (view.tab == Tab::Logs).then_some(&view.logs_source),
    "logsKey": view.logs_key,
    "terminal": {
      "activeSessionId": model.active_session,
      "pending": [],
      "sessionsRevision": data.sessions_revision,
      "commands": model.commands,
      "remoteShell": model.is_remote(),
      "mainRepoPath": model.main_repo,
    },
    "fileBrowser": file_browser,
    "remoteChrome": remote_chrome(model),
    "workspace": workspace,
  })
}

fn dialog_workspace(workspace: &Workspace) -> Value {
  json!({
    "id": workspace.id,
    "branchName": workspace.branch_name,
    "workspacePath": workspace.workspace_path,
  })
}

/// The workspace dialog open over the screen (`GpuiWorkspaceModal`).
fn workspace_modal(model: &Model, data: &Data) -> Option<Value> {
  let repo = model.repo();
  let dialogs = &model.dialogs;
  if let Some((key, defaults)) = &dialogs.workspace_dialog {
    let mut defaults = defaults.clone();
    if defaults.get("source").is_none() {
      defaults["source"] = json!({"kind": "create"});
    }
    return Some(json!({
      "kind": "workspaceDialog",
      "repoPath": repo,
      "key": key,
      "defaults": defaults,
    }));
  }
  if let (Some(stack_on), false) = (&dialogs.open_workspaces, repo.is_empty()) {
    // Every repository the sidebar shows, whose links the dialog opens.
    let repos: Vec<&str> = std::iter::once(repo)
      .chain(
        data
          .supporting()
          .iter()
          .filter(|r| r.exists && r.path != repo)
          .map(|r| r.path.as_str()),
      )
      .collect();
    return Some(json!({
      "kind": "openWorkspaces",
      "repoPath": repo,
      "repos": repos,
      "linear": data.flags.linear,
      "stackOn": stack_on,
    }));
  }
  if let Some((repo_path, id, branch)) = &dialogs.rename {
    return Some(json!({
      "kind": "renameWorkspace",
      "repoPath": repo_path,
      "workspaceId": id,
      "branchName": branch,
    }));
  }
  if dialogs.deletion && !repo.is_empty() {
    return Some(json!({
      "kind": "workspaceDeletion",
      "repoPath": repo,
      "workspaces": data.workspaces(repo).iter().map(dialog_workspace).collect::<Vec<_>>(),
      "currentWorkspace": model.selected_workspace(data).map(dialog_workspace),
    }));
  }
  dialogs.change_move.as_ref().map(|change| {
    json!({
      "kind": "confirmChangeMove",
      "repoPath": repo,
      "files": change.files,
      "sourceBranch": change.source_branch,
      "destinationBranch": change.destination_branch,
      "destinationLabel": change.destination_label,
    })
  })
}

/// The agent prompt, prompt history or stash dialog.
pub fn modal(model: &Model, data: &Data) -> Option<Modal> {
  if !model.has_repo() {
    return None;
  }
  let repo = model.repo();
  let dialogs = &model.dialogs;
  let workspaces = data.workspaces(repo);
  let value = if let Some(prompt) = &dialogs.agent_prompt {
    let initial = prompt
      .workspace_id
      .filter(|id| workspaces.iter().any(|w| w.id == *id));
    json!({
      "kind": "agentPrompt",
      "repoPath": repo,
      "defaultBranch": model.default_branch(data),
      "workspaces": workspaces.iter().map(|w| json!({
        "id": w.id,
        "branchName": w.branch_name,
        "path": full_path(w),
      })).collect::<Vec<_>>(),
      "initialPrompt": prompt.prompt,
      "initialWorkspaceId": initial,
      "issue": prompt.issue,
      "starting": false,
    })
  } else if dialogs.stash {
    let targets: Vec<Value> = sidebar::visible(data, repo)
      .iter()
      .map(|w| {
        let title = w.title.trim();
        json!({
          "branch": w.branch_name,
          "label": if title.is_empty() { w.branch_name.as_str() } else { title },
        })
      })
      .collect();
    json!({"kind": "stash", "repoPath": repo, "workspaces": targets})
  } else {
    let focus = dialogs.prompt_history?;
    json!({"kind": "promptHistory", "repoPath": repo, "initialSelectedId": focus})
  };
  read(value, "dialog")
}

/// The window-wide dialog on top: the workspace picker, the shortcuts,
/// the branch switcher, then the palette.
pub fn overlay(model: &Model, data: &Data) -> Option<OverlaySnapshot> {
  let repo = model.repo();
  let dialogs = &model.dialogs;
  let has_repo = !repo.is_empty();
  let mut value = if dialogs.workspace_picker && has_repo {
    json!({"kind": "workspacePicker", "repoPath": repo})
  } else if dialogs.shortcuts {
    json!({"kind": "keyboardShortcuts"})
  } else if dialogs.branch_switcher && has_repo {
    json!({"kind": "branchSwitcher", "repoPath": repo})
  } else if dialogs.palette {
    let workspace = model.selected_workspace(data);
    json!({
      "kind": "commandPalette",
      "repoPath": repo,
      "stash": true,
      "hasSelectedWorkspace": workspace.is_some(),
      "workspace": workspace.map(|w| json!({
        "id": w.id,
        "repoPath": w.repo_path,
        "branch": w.branch_name,
      })),
    })
  } else {
    return None;
  };
  value["dark"] = model.prefs.dark().into();
  read(value, "overlay")
}

/// Which overlay is on top, so Escape and "overlay.close" close it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Top {
  WorkspacePicker,
  Shortcuts,
  BranchSwitcher,
  Palette,
}

pub fn top_overlay(model: &Model) -> Option<Top> {
  let dialogs = &model.dialogs;
  let has_repo = model.has_repo();
  if dialogs.workspace_picker && has_repo {
    Some(Top::WorkspacePicker)
  } else if dialogs.shortcuts {
    Some(Top::Shortcuts)
  } else if dialogs.branch_switcher && has_repo {
    Some(Top::BranchSwitcher)
  } else if dialogs.palette {
    Some(Top::Palette)
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use super::{
    super::{
      data::RepoRead,
      model::{AgentPrompt, ChangeMove},
      tree::tests::workspace,
    },
    *,
  };
  use crate::{
    core::repo::RepoBranch,
    gpui_shell::{overlay::OverlayView, screen::View},
  };

  fn data() -> Data {
    let mut data = Data::default();
    data.apply(
      "/repo",
      RepoRead {
        workspaces: Some(vec![
          workspace(1, "login", Some("main")),
          workspace(2, "fix", Some("login")),
        ]),
        branch: Some(Some(RepoBranch {
          current_branch: Some("main".into()),
          display_ref: "main".into(),
          is_detached: false,
        })),
        has_changes: Some(HashMap::new()),
        ..RepoRead::default()
      },
    );
    data
  }

  #[test]
  fn shows_onboarding_without_a_repository() {
    let snapshot = screen(&Model::new(None), &Data::default()).unwrap();
    assert!(matches!(snapshot.view, View::Onboarding(_)));
  }

  #[test]
  fn shows_the_selected_workspace_with_gpui_drawing_every_tab() {
    let mut model = Model::new(Some("/repo".into()));
    model.selected = Some(2);
    model.view.tab = Tab::Commits;
    model.view.scroll_to_commit = Some("abc".into());
    model.terminal("newShell", json!({"dir": "/repo"}));
    let snapshot = screen(&model, &data()).unwrap();
    let View::Workspace(route) = snapshot.view else {
      panic!("not the workspace view");
    };
    assert_eq!(route.workspace_id, Some(2));
    assert_eq!(route.home_branch, "main");
    assert_eq!(route.scroll_to_commit.as_deref(), Some("abc"));
    assert_eq!(route.show_committed, Some(true));
    let targets: Vec<_> = route
      .move_targets
      .iter()
      .map(|t| t.branch.as_str())
      .collect();
    assert_eq!(targets, [".", "login"], "everything but the source");
    let terminal = route.terminal.unwrap();
    assert_eq!(terminal.commands.len(), 1);
    assert_eq!(terminal.commands[0].kind, "newShell");
  }

  #[test]
  fn the_home_repository_offers_no_committed_toggle() {
    let model = Model::new(Some("/repo".into()));
    let View::Workspace(route) = screen(&model, &data()).unwrap().view else {
      panic!("not the workspace view");
    };
    assert_eq!(route.workspace_id, None);
    assert_eq!(route.show_committed, None);
  }

  #[test]
  fn settings_route_carries_a_skill_install_request() {
    let mut model = Model::new(Some("/repo".into()));
    model.open_skill_install("anthropic/pdf".into());
    let View::Settings(route) = screen(&model, &data()).unwrap().view else {
      panic!("not settings");
    };
    assert_eq!(route.install_request.as_deref(), Some("anthropic/pdf"));
  }

  #[test]
  fn skill_link_shows_settings_even_without_a_repository() {
    let mut model = Model::new(None);
    model.open_skill_install("anthropic/pdf".into());
    let View::Settings(route) = screen(&model, &data()).unwrap().view else {
      panic!("not settings");
    };
    assert_eq!(route.repo_path, None);
    assert_eq!(route.install_request.as_deref(), Some("anthropic/pdf"));
  }

  #[test]
  fn routes_each_page() {
    let mut model = Model::new(Some("/repo".into()));
    model.open_settings();
    assert!(matches!(
      screen(&model, &data()).unwrap().view,
      View::Settings(_)
    ));
    model.open_github_pr(7, "MERGED");
    let View::Github(route) = screen(&model, &data()).unwrap().view else {
      panic!("not GitHub");
    };
    assert_eq!((route.selected, route.reload), (Some(7), 0));
    if let Page::Github(github) = &mut model.page {
      github.reload = 2;
    }
    let View::Github(route) = screen(&model, &data()).unwrap().view else {
      panic!("not GitHub");
    };
    assert_eq!(route.reload, 2);
    if let Page::Github(github) = &mut model.page {
      github.review = Some(7);
    }
    assert!(matches!(
      screen(&model, &data()).unwrap().view,
      View::PrReview(_)
    ));
    model.selected = Some(1);
    model.open_merge_preview();
    assert!(matches!(
      screen(&model, &data()).unwrap().view,
      View::MergePreview(_)
    ));
    model.open_linear();
    assert!(
      matches!(screen(&model, &data()).unwrap().view, View::Workspace(_)),
      "Linear needs its preview feature"
    );
  }

  #[test]
  fn builds_each_workspace_dialog() {
    let mut model = Model::new(Some("/repo".into()));
    let data = data();
    model.open_workspace_dialog(json!({}));
    assert!(screen(&model, &data).unwrap().modal.is_some());
    model.dialogs.workspace_dialog = None;
    model.dialogs.rename = Some(("/repo".into(), 1, "login".into()));
    assert!(screen(&model, &data).unwrap().modal.is_some());
    model.dialogs.rename = None;
    model.dialogs.deletion = true;
    assert!(screen(&model, &data).unwrap().modal.is_some());
    model.dialogs.deletion = false;
    model.dialogs.change_move = Some(ChangeMove {
      files: vec!["a".into()],
      source_branch: "login".into(),
      destination_branch: ".".into(),
      destination_label: "main".into(),
    });
    assert!(screen(&model, &data).unwrap().modal.is_some());
  }

  #[test]
  fn builds_the_agent_dialogs() {
    let mut model = Model::new(Some("/repo".into()));
    let data = data();
    assert_eq!(modal(&model, &data), None);
    model.dialogs.prompt_history = Some(Some(3));
    assert!(matches!(
      modal(&model, &data),
      Some(Modal::PromptHistory(_))
    ));
    model.dialogs.stash = true;
    assert!(matches!(modal(&model, &data), Some(Modal::Stash(_))));
    model.dialogs.agent_prompt = Some(AgentPrompt {
      workspace_id: Some(99),
      ..AgentPrompt::default()
    });
    let Some(Modal::AgentPrompt(route)) = modal(&model, &data) else {
      panic!("not the prompt dialog");
    };
    assert_eq!(route.initial_workspace_id, None, "an unknown workspace");
    assert_eq!(route.workspaces.len(), 2);
  }

  fn remote_model() -> Model {
    use crate::gpui_shell::remote_repo::tests::repository;
    let mut model = Model::new(None);
    let repository = repository();
    model.main_repo = repository.canonical_path.clone();
    model.remote.endpoint = repository.endpoint.clone();
    model.remote.repository = Some(repository);
    model
  }

  #[test]
  fn shows_the_remote_repository_with_its_chrome_and_remote_shells() {
    let model = remote_model();
    let snapshot = screen(&model, &Data::default()).unwrap();
    assert!(snapshot.repository.is_some());
    let View::Workspace(route) = snapshot.view else {
      panic!("not the workspace view");
    };
    let chrome = route.remote_chrome.unwrap();
    assert_eq!(chrome.state, "online");
    assert!(chrome.terminal);
    assert_eq!(chrome.notices, [MERGE_REASON]);
    assert!(route.terminal.unwrap().remote_shell);
  }

  #[test]
  fn a_cut_off_endpoint_shows_the_cutoff_banner() {
    let mut model = remote_model();
    let id = model.remote.endpoint.clone().unwrap().id;
    model.remote.cutoffs.insert(id, "session_ended".into());
    let View::Workspace(route) = screen(&model, &Data::default()).unwrap().view else {
      panic!("not the workspace view");
    };
    let chrome = route.remote_chrome.unwrap();
    assert_eq!(chrome.state, "cutoff");
    assert!(chrome.cut_off);
  }

  #[test]
  fn routes_the_remote_dialogs() {
    let mut model = remote_model();
    model.remote.setup = true;
    model.remote.terminal = true;
    model.remote.ambiguous = Some("lost".into());
    let remote = screen(&model, &Data::default()).unwrap().remote;
    assert!(remote.setup.is_some());
    let terminal = remote.terminal.unwrap();
    assert_eq!(terminal.workspace_id, "root");
    assert_eq!(remote.ambiguous.as_deref(), Some("lost"));
  }

  #[test]
  fn shows_the_repository_form_under_onboarding_while_an_endpoint_is_connected() {
    let mut model = remote_model();
    model.main_repo.clear();
    model.remote.repository = None;
    let View::Onboarding(route) = screen(&model, &Data::default()).unwrap().view else {
      panic!("not onboarding");
    };
    let form = route.remote_repo.unwrap();
    assert!(form.selector);
    assert_eq!(form.path, remote::HOME);
    assert!(
      matches!(screen(&Model::new(None), &Data::default()).unwrap().view, View::Onboarding(r) if r.remote_repo.is_none())
    );
  }

  #[test]
  fn stacks_overlays_as_dashboard_does() {
    let mut model = Model::new(Some("/repo".into()));
    let data = data();
    assert!(overlay(&model, &data).is_none());
    model.dialogs.palette = true;
    model.dialogs.branch_switcher = true;
    let snapshot = overlay(&model, &data).unwrap();
    assert!(matches!(snapshot.view, OverlayView::BranchSwitcher { .. }));
    assert_eq!(top_overlay(&model), Some(Top::BranchSwitcher));
    model.dialogs.branch_switcher = false;
    let snapshot = overlay(&model, &data).unwrap();
    assert!(matches!(snapshot.view, OverlayView::CommandPalette(_)));
  }
}
