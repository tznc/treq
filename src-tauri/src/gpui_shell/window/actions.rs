//! What the window does for each action its sidebar, screens, dialogs and
//! overlays send, and for each keyboard shortcut: Dashboard's handlers
//! (`useGpuiSidebar`, `useGpuiWorkspaceScreen`, `useGpuiWorkspaceDialogs`,
//! `useGpuiAgentModals`, `useGpuiOverlays`, `useGpuiGithubScreen` and the
//! panels' own), as changes to the [`Model`] and the [`Effect`]s they need.
//!
//! Each returns `None` for an action that is not the window's: a menu or
//! drag `sidebar_actions` runs, or one a screen runs itself. The remote
//! ones are in `remote_actions`.

use serde_json::{json, Value};

use super::{
  data::Data,
  model::{
    self, workspace_source, ChangeMove, Confirm, Effect, FolderPurpose, GithubTab, Model, Page,
    PendingSelect, Tab, TerminalSummary,
  },
  sidebar, snapshots, tree,
};
use crate::{
  core::agent_launch::pane_session_id,
  gpui_shell::{
    native::{NativeEvent, OpenRequest, ToastTone},
    screen::ScreenAction,
    sidebar::SidebarAction,
  },
  local_db::Workspace,
};

/// A keyboard shortcut the window handles (Dashboard's
/// `useKeyboardShortcut`s, the terminal pane's and the zoom keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
  /// Cmd+N.
  NewWorkspace,
  /// Cmd+Shift+N.
  Stack,
  /// Cmd+K.
  Palette,
  /// Cmd+I.
  AgentPrompt,
  /// Cmd+P.
  FilePicker,
  /// `?`.
  ToggleShortcuts,
  Escape,
  /// Cmd+J.
  ToggleTerminal,
  /// Cmd+Ctrl+J.
  MaximizeTerminal,
  /// Cmd+].
  NewAgent,
  /// Cmd+\.
  NewShell,
  ZoomIn,
  ZoomOut,
}

/// What a handler did: changed the model with these effects, or left the
/// action to `sidebar_actions` (a menu or drag) or to nobody.
pub type Handled = Option<Vec<Effect>>;

fn done() -> Handled {
  Some(Vec::new())
}

fn with(effect: Effect) -> Handled {
  Some(vec![effect])
}

impl Model {
  /// A keyboard shortcut.
  pub fn shortcut(&mut self, shortcut: Shortcut, data: &Data) -> Vec<Effect> {
    // A confirmation takes every key until it is answered.
    if self.confirm.is_some() {
      return match shortcut {
        Shortcut::Escape => self.answer(false),
        _ => Vec::new(),
      };
    }
    let repo = self.has_repo();
    // One dialog at a time: Escape only reaches the one on top.
    let free = !self.dialogs.any_open();
    match shortcut {
      Shortcut::NewWorkspace if repo && free => self.new_workspaces(None),
      Shortcut::Stack if repo && free => return self.stack(data),
      Shortcut::Palette if free => self.dialogs.palette = true,
      Shortcut::AgentPrompt if repo && free => self.open_agent_prompt(),
      Shortcut::FilePicker if repo && free => self.dialogs.file_picker = true,
      Shortcut::ToggleShortcuts if free || self.dialogs.shortcuts => {
        self.dialogs.shortcuts = !self.dialogs.shortcuts;
      }
      Shortcut::Escape => {
        self.dialogs.palette = false;
        self.dialogs.file_picker = false;
        self.dialogs.shortcuts = false;
        self.remote.setup = false;
      }
      Shortcut::ToggleTerminal if self.shows_terminal() => {
        self.terminal("toggleCollapse", json!({}));
      }
      Shortcut::MaximizeTerminal if self.shows_terminal() => {
        self.terminal("toggleMaximize", json!({}));
      }
      Shortcut::NewAgent if self.shows_terminal() => return self.new_agent(self.selected, data),
      Shortcut::NewShell if self.shows_terminal() => self.terminal("newShell", json!({})),
      Shortcut::ZoomIn => return vec![self.zoom(super::model::ZOOM_STEP as i32)],
      Shortcut::ZoomOut => return vec![self.zoom(-(super::model::ZOOM_STEP as i32))],
      _ => {}
    }
    Vec::new()
  }

  /// The agent prompt dialog, on the open workspace (main from the
  /// repository's home), unless it is open already.
  fn open_agent_prompt(&mut self) {
    let workspace_id = self.selected;
    self
      .dialogs
      .agent_prompt
      .get_or_insert_with(|| super::model::AgentPrompt {
        workspace_id,
        ..Default::default()
      });
  }

  /// Whether the workspace view, and so the terminal pane, shows.
  fn shows_terminal(&self) -> bool {
    self.has_repo() && matches!(self.page, Page::Workspace)
  }

  /// `useGpuiSidebar`'s action handler.
  pub fn sidebar_action(&mut self, action: &SidebarAction, data: &Data) -> Handled {
    let repo = self.repo().to_string();
    let find = |id: Option<i64>| id.and_then(|id| self.workspace_in(&repo, id, data));
    match action {
      SidebarAction::SelectHome { repo_path } => {
        if *repo_path != repo {
          let missing = data
            .supporting()
            .iter()
            .any(|r| &r.path == repo_path && !r.exists);
          if !missing {
            self.select_repo_home(repo_path, data);
          }
        } else if !self.multi.is_empty() {
          self.clear_multi();
        } else {
          self.select_workspace(None, data);
        }
        done()
      }
      SidebarAction::SelectWorkspace { id, shift, toggle } => {
        let rows = sidebar::visible(data, &repo);
        self.click_workspace(&rows, *id, *shift, *toggle, data);
        done()
      }
      SidebarAction::SelectStack { id, descendants } => {
        let rows = sidebar::visible(data, &repo);
        if let Some(workspace) = rows.iter().find(|w| w.id == *id) {
          let stack: Vec<&Workspace> = if *descendants {
            std::iter::once(workspace)
              .chain(tree::descendants(&rows, &workspace.branch_name))
              .collect()
          } else {
            tree::entire_stack(&rows, &workspace.branch_name)
          };
          self.multi = stack.iter().map(|w| w.id).collect();
        }
        done()
      }
      SidebarAction::ClearSelection => {
        if !self.multi.is_empty() {
          self.clear_multi();
        }
        done()
      }
      SidebarAction::OpenSettings => {
        self.open_settings();
        done()
      }
      SidebarAction::OpenCommandPalette => {
        self.dialogs.palette = true;
        done()
      }
      SidebarAction::OpenGithub => {
        self.open_github();
        done()
      }
      SidebarAction::OpenWorkspaces => {
        self.new_workspaces(None);
        done()
      }
      SidebarAction::OpenLinear => {
        self.open_linear();
        done()
      }
      SidebarAction::StartAgent { id } => {
        if id.is_some() && find(*id).is_none() {
          return done();
        }
        Some(self.new_agent(*id, data))
      }
      SidebarAction::OpenShell { id } => {
        let workspace = find(*id);
        if id.is_none() || workspace.is_some() {
          self.open_shell(workspace.as_ref());
        }
        done()
      }
      SidebarAction::Stack { id } => match (id, find(*id)) {
        (None, _) => {
          self.stack_home(data);
          done()
        }
        (Some(_), Some(workspace)) => {
          self.stack_on(&workspace);
          done()
        }
        _ => done(),
      },
      SidebarAction::DismissModal => done(),
      SidebarAction::ResizeSidebar { width } => with(self.resize_sidebar(Some(*width))),
      SidebarAction::ResetSidebarWidth => with(self.resize_sidebar(None)),
      SidebarAction::OpenRepoWorkspace { repo_path, id } => {
        if let Some(workspace) = self.workspace_in(repo_path, *id, data) {
          self.select_workspace(Some(&workspace), data);
        }
        done()
      }
      SidebarAction::ToggleRepo { repo_path } => with(self.toggle_repo(repo_path)),
      // The menus, Bulk Archive and drags run in `sidebar_actions`.
      _ => None,
    }
  }

  /// A screen's or dialog's action (`ScreenAction`).
  pub fn screen_action(&mut self, action: &ScreenAction, data: &Data) -> Handled {
    match action {
      ScreenAction::Press { id } => match self.remote_press(id) {
        Some(effects) => Some(effects),
        None => self.press(id, data),
      },
      ScreenAction::Field { field, value } => match self.remote_field(field, value, data) {
        Some(effects) => Some(effects),
        None => self.field(field, value, data),
      },
      ScreenAction::OpenUrl { url } => with(Effect::OpenUrl(url.clone())),
      // The onboarding screen opens a local repository itself.
      ScreenAction::OpenRepo => with(Effect::PickFolder(FolderPurpose::OpenRepo)),
      ScreenAction::RecheckPrerequisites => done(),
      // A slider let go: the screens that have one save it themselves.
      ScreenAction::Commit { .. } => done(),
    }
  }

  fn press(&mut self, id: &str, data: &Data) -> Handled {
    let (name, arg) = id.split_once(':').unwrap_or((id, ""));
    match name {
      // The workspace view.
      "workspace.branchSwitcher" => self.dialogs.branch_switcher = true,
      "workspace.stack" => return Some(self.stack(data)),
      "workspace.merge" => self.open_merge_preview(),
      "code.filePicker" => self.dialogs.file_picker = true,
      "code.agentPrompt" => self.open_agent_prompt(),
      "fileBrowser.back" => self.view.file_browser = None,
      // The dialogs.
      "workspaceDialog.close" => self.dialogs.workspace_dialog = None,
      "renameWorkspace.close" => self.dialogs.rename = None,
      "openWorkspaces.close" => self.dialogs.open_workspaces = None,
      "workspaceDeletion.close" => self.dialogs.deletion = false,
      "changeMove.done" => {
        self.dialogs.change_move = None;
        self.view.changes_revision += 1;
        return with(Effect::Refresh { revision: true });
      }
      "changeMove.cancel" => self.dialogs.change_move = None,
      "agentPrompt.close" => self.dialogs.agent_prompt = None,
      "promptHistory.close" => self.dialogs.prompt_history = None,
      "stash.close" => self.dialogs.stash = false,
      "filePicker.close" => self.dialogs.file_picker = false,
      // Pages.
      "settings.close" => self.close_settings(),
      "skills.installRequest.close" => self.skill_install = None,
      "linear.openSettings" => {
        self.open_settings();
        if let Page::Settings { tab } = &mut self.page {
          *tab = "integrations".into();
        }
      }
      "mergePreview.cancel" => self.page = Page::Workspace,
      "mergePreview.merged" => {
        self.page = Page::Workspace;
        self.return_home();
        return with(Effect::Refresh { revision: true });
      }
      "prReview.close" | "prReview.submitted" => {
        if let Page::Github(github) = &mut self.page {
          let number = github.review.take();
          github.tab = GithubTab::Prs;
          github.selected = number.or(github.selected);
          github.creating = false;
          if name == "prReview.submitted" {
            github.reload += 1;
          }
        }
      }
      name if name.starts_with("github.") => return self.github_press(name, arg, data),
      _ => return None,
    }
    done()
  }

  /// `useGpuiGithubScreen`'s presses, which navigate within the view.
  fn github_press(&mut self, name: &str, arg: &str, data: &Data) -> Handled {
    let repo = self.repo().to_string();
    let number = arg.parse::<i64>().ok();
    match name {
      "github.openWorkspace" | "github.openLogs" => {
        let Some(id) = number else { return done() };
        let logs = name == "github.openLogs" && data.flags.logs;
        return Some(self.select_when_listed(
          PendingSelect {
            repo_path: repo,
            id,
            page: true,
            logs,
          },
          data,
        ));
      }
      _ => {}
    }
    let Page::Github(github) = &mut self.page else {
      return done();
    };
    let number = arg.parse::<u64>().ok();
    match name {
      "github.tab" => {
        github.tab = match arg {
          "issues" => GithubTab::Issues,
          "prs" => GithubTab::Prs,
          // Any other tab name is ignored.
          _ => return done(),
        };
        github.filter = "open".into();
        github.selected = None;
        github.creating = false;
      }
      "github.filter" => {
        github.filter = arg.into();
        github.selected = None;
        github.creating = false;
      }
      "github.select" | "github.created" => {
        github.selected = number;
        github.creating = false;
      }
      "github.new" => {
        github.selected = None;
        github.creating = true;
      }
      "github.close" => {
        github.selected = None;
        github.creating = false;
      }
      "github.review" => github.review = number,
      _ => return None,
    }
    done()
  }

  fn field(&mut self, field: &str, value: &Value, data: &Data) -> Handled {
    let repo = self.repo().to_string();
    match field {
      "workspace.tab" => {
        if let Some(tab) = value.as_str().and_then(Tab::parse) {
          self.view.tab = tab;
        }
      }
      "workspace.openLogs" => {
        if let Some(source) = value.as_str().filter(|_| data.flags.logs) {
          self.open_logs(source);
        }
      }
      "workspace.viewFullPrompt" => {
        if let Some(id) = value.as_i64() {
          self.view_full_prompt(id);
        }
      }
      "workspace.contentHeight" | "changes.files" | "changes.subView" => {}
      "code.open" => {
        let name = value["name"].as_str().unwrap_or_default();
        let directory = value["isDirectory"].as_bool().unwrap_or(false);
        let dir = self.working_directory(data);
        self.open_entry(&dir, name, directory);
      }
      "code.commit" => {
        self.view.scroll_to_commit = value.as_str().map(str::to_string);
        self.view.tab = Tab::Commits;
      }
      "code.stack" => {
        if let Ok(workspace) = serde_json::from_value::<Workspace>(value.clone()) {
          self.select_workspace(Some(&workspace), data);
        }
      }
      // The backend reports the session it started (`AgentSessionStarted`).
      "code.sessionCreated" => {}
      "commits.scrolled" => self.view.scroll_to_commit = None,
      "commits.moveToNew" | "commits.moveToExisting" => {
        let workspace = self.selected_workspace(data).cloned();
        let mut defaults = self.dialog_from(workspace.as_ref());
        defaults["preSelectedCommits"] = json!([value["change_id"]]);
        defaults["activeTab"] = "commits".into();
        let first = value["description"]
          .as_str()
          .and_then(|d| d.lines().next())
          .filter(|line| !line.is_empty());
        if let (Some(first), "commits.moveToNew") = (first, field) {
          defaults["description"] = first.into();
        }
        self.open_workspace_dialog(defaults);
      }
      "commits.stashed" => self.dialogs.stash = true,
      "changes.moveFiles" => {
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_string();
        let files: Vec<String> = serde_json::from_value(value["files"].clone()).unwrap_or_default();
        let (source, destination) = (text("sourceBranch"), text("destinationBranch"));
        if !files.is_empty() && source != destination {
          self.dialogs.change_move = Some(ChangeMove {
            files,
            source_branch: source,
            destination_branch: destination,
            destination_label: text("destinationLabel"),
          });
        }
      }
      "changes.moveToNew" => {
        let workspace = self.selected_workspace(data).cloned();
        let mut defaults = self.dialog_from(workspace.as_ref());
        defaults["preSelectedFiles"] = value.clone();
        defaults["activeTab"] = "changes".into();
        self.open_workspace_dialog(defaults);
      }
      "changes.showCommitted" => self.view.show_committed = value.as_bool().unwrap_or(true),
      field if field.starts_with("terminal.") => return self.terminal_field(field, value, data),
      // The dialogs.
      "openWorkspaces.opened" => {
        self.dialogs.open_workspaces = None;
        let (Some(repo_path), Some(id)) = (value["repoPath"].as_str(), value["id"].as_i64()) else {
          return done();
        };
        return Some(self.select_when_listed(
          PendingSelect {
            repo_path: repo_path.to_string(),
            id,
            page: false,
            logs: false,
          },
          data,
        ));
      }
      "workspaceDialog.created" => {
        self.dialogs.workspace_dialog = None;
        let Some(id) = value.as_i64() else {
          return done();
        };
        return Some(self.select_when_listed(
          PendingSelect {
            repo_path: repo,
            id,
            page: false,
            logs: false,
          },
          data,
        ));
      }
      "promptHistory.run" => {
        let prompt = value["prompt"].as_str().unwrap_or_default().to_string();
        self.run_prompt(prompt, value["workspaceId"].as_i64());
      }
      "stash.applyToNew" => self.apply_stash_to_new(value, data),
      "filePicker.select" => {
        self.dialogs.file_picker = false;
        if let Some(path) = value.as_str() {
          self.open_file_in(path, data);
        }
      }
      // Pages.
      "settings.tab" => {
        if let (Page::Settings { tab }, Some(value)) = (&mut self.page, value.as_str()) {
          *tab = value.to_string();
        }
      }
      "github.startPrompt" => self.start_prompt_from_issue(json!({
        "source": "github",
        "id": value["number"].to_string(),
        "key": format!("#{}", value["number"]),
        "url": value["url"],
        "title": value["title"],
        "includeSubItems": false,
        "subItemIds": [],
      })),
      "linear.kickoff" => self.start_prompt_from_issue(json!({
        "source": "linear",
        "id": value["id"],
        "key": value["identifier"],
        "url": value["url"],
        "title": value["title"],
        "includeSubItems": value["includeSubissues"].as_bool().unwrap_or(false),
        "subItemIds": value.get("sub_issue_ids").cloned().unwrap_or_else(|| json!([])),
      })),
      // Customizing a skill edits its copy in the home repository.
      "skills.startAgent" => {
        self.page = Page::Workspace;
        return with(Effect::StartAgent {
          repo_path: repo,
          workspace_id: None,
          agent: None,
          prompt: value["prompt"].as_str().map(str::to_string),
          mode: None,
          name: Some("Customize skill".into()),
          select: true,
        });
      }
      "reviewPrompt.tune" => {
        self.page = Page::Workspace;
        return with(Effect::StartAgent {
          repo_path: repo,
          workspace_id: None,
          agent: None,
          prompt: value["prompt"].as_str().map(str::to_string),
          mode: Some("plan".into()),
          name: Some("Tune review prompt".into()),
          select: true,
        });
      }
      // A review of text not tied to a workspace runs in the home repository.
      "linear.startReview" | "prReview.startAgentReview" => {
        return with(Effect::StartAgent {
          repo_path: repo,
          workspace_id: None,
          agent: value["agent"].as_str().map(str::to_string),
          prompt: value["prompt"].as_str().map(str::to_string),
          mode: Some("acceptEdits".into()),
          name: Some("AI Review".into()),
          select: true,
        })
      }
      _ => return None,
    }
    done()
  }

  /// The selected workspace's checkout, else the repository's.
  fn working_directory(&self, data: &Data) -> String {
    self
      .selected_workspace(data)
      .map(super::model::full_path)
      .unwrap_or_else(|| self.repo().to_string())
  }

  /// Cmd+P's pick, relative to the checkout it searched.
  fn open_file_in(&mut self, path: &str, data: &Data) {
    let dir = self.working_directory(data);
    self.view.tab = Tab::Overview;
    let full = if path.starts_with('/') {
      path.to_string()
    } else {
      format!("{}/{path}", dir.trim_end_matches('/'))
    };
    self.open_file(&full);
  }

  /// A workspace dialog's defaults from the workspace on screen.
  fn dialog_from(&self, workspace: Option<&Workspace>) -> Value {
    match workspace {
      Some(workspace) => json!({
        "targetBranch": workspace.branch_name,
        "source": workspace_source(workspace),
      }),
      None => json!({"source": {"kind": "home"}}),
    }
  }

  /// The repositories the terminal pane's sessions belong to: the main
  /// one, then the supporting ones (`linkedRepoSessions`).
  fn linked_repos(&self, data: &Data) -> Vec<String> {
    if !self.has_repo() {
      return Vec::new();
    }
    std::iter::once(self.main_repo.clone())
      .chain(
        data
          .supporting()
          .iter()
          .filter(|r| r.exists)
          .map(|r| r.path.clone()),
      )
      .collect()
  }

  /// `handleGpuiTerminalField`.
  fn terminal_field(&mut self, field: &str, value: &Value, data: &Data) -> Handled {
    match field {
      "terminal.activeSession" => {
        let Some(pane) = value.as_i64() else {
          self.active_session = None;
          return done();
        };
        self.active_session = Some(pane);
        for repo in self.linked_repos(data) {
          let Some(session) = data
            .sessions(&repo)
            .iter()
            .find(|s| pane_session_id(&repo, &self.main_repo, s.id) == pane)
            .cloned()
          else {
            continue;
          };
          self.activate_repo(&repo, data);
          self.page = Page::Workspace;
          if let Some(id) = session.workspace_id {
            if let Some(workspace) = self.workspace_in(&repo, id, data) {
              self.set_selected(Some(workspace.id));
            }
          }
          break;
        }
      }
      "terminal.closeSession" => {
        if value.as_i64().is_some() && value.as_i64() == self.active_session {
          self.active_session = None;
        }
      }
      "terminal.newAgent" => {
        let Some(dir) = value.as_str().filter(|d| !d.is_empty()) else {
          return Some(self.new_agent(self.selected, data));
        };
        if self.is_remote() {
          let workspace = data
            .workspaces(self.repo())
            .iter()
            .find(|w| super::model::full_path(w) == dir)
            .map(|w| w.id);
          return Some(self.start_remote_agent(workspace, data));
        }
        let workspace = self.linked_repos(data).iter().find_map(|repo| {
          data
            .workspaces(repo)
            .iter()
            .find(|w| super::model::full_path(w) == dir)
            .cloned()
        });
        return match workspace {
          Some(workspace) if workspace.repo_path != self.repo() => with(Effect::StartAgent {
            repo_path: workspace.repo_path.clone(),
            workspace_id: Some(workspace.id),
            agent: None,
            prompt: None,
            mode: None,
            name: None,
            select: true,
          }),
          workspace => with(self.start_agent(workspace.map(|w| w.id), data)),
        };
      }
      "terminal.navigate" => {
        let key = value["key"].as_str().unwrap_or_default();
        if value["isMainRepo"].as_bool().unwrap_or(false) {
          if self.linked_repos(data).iter().any(|r| r == key) {
            self.select_repo_home(key, data);
          } else {
            self.select_workspace(None, data);
          }
        } else {
          let workspace = self.linked_repos(data).iter().find_map(|repo| {
            data
              .workspaces(repo)
              .iter()
              .find(|w| w.workspace_path == key || super::model::full_path(w) == key)
              .cloned()
          });
          if let Some(workspace) = workspace {
            self.select_workspace(Some(&workspace), data);
          }
        }
      }
      "terminal.summaries" => {
        let streaming = |terminals: &[TerminalSummary]| {
          terminals.iter().filter(|t| t.agent && t.streaming).count()
        };
        let was = streaming(&self.terminals);
        self.terminals = value
          .as_array()
          .map(|list| {
            list
              .iter()
              .map(|s| TerminalSummary {
                agent: s["kind"] == "agent",
                branch: s["branchName"].as_str().map(str::to_string),
                repo_path: s["repoPath"].as_str().map(str::to_string),
                streaming: s["isStreaming"].as_bool().unwrap_or(false),
              })
              .collect()
          })
          .unwrap_or_default();
        // An agent's turn ended: it may have rewritten history outside
        // treq, so the tab counts and the Changes tab read again.
        if streaming(&self.terminals) < was {
          self.view.changes_revision += 1;
        }
      }
      "terminal.dismissAsset" => {
        if let Some(id) = value.as_str() {
          return with(Effect::DismissSendAsset(id.to_string()));
        }
      }
      // The app's shortcuts, pressed while the terminal has focus.
      "terminal.shortcut" => {
        let shortcut = match value.as_str() {
          Some("k") => Shortcut::Palette,
          Some("n") => Shortcut::NewWorkspace,
          Some("p") => Shortcut::FilePicker,
          _ => return done(),
        };
        return Some(self.shortcut(shortcut, data));
      }
      _ => return None,
    }
    done()
  }

  /// `handleCreateSessionFromSidebar`: an agent session, on the host for a
  /// remote repository.
  fn new_agent(&mut self, workspace_id: Option<i64>, data: &Data) -> Vec<Effect> {
    if self.is_remote() {
      return self.start_remote_agent(workspace_id, data);
    }
    vec![self.start_agent(workspace_id, data)]
  }

  /// `useGpuiOverlays`' action handler.
  pub fn overlay_action(&mut self, action: &ScreenAction, data: &Data) -> Handled {
    match action {
      ScreenAction::Press { id } if id == "overlay.commandPalette" => {
        self.dialogs.palette = true;
        done()
      }
      ScreenAction::Press { id } if id == "overlay.close" => {
        self.close_top_overlay();
        done()
      }
      ScreenAction::Press { id } => {
        let command = id.strip_prefix("palette.")?;
        let effects = self.palette_command(command, data)?;
        self.dialogs.palette = false;
        Some(effects)
      }
      ScreenAction::Field { field, value } => match field.as_str() {
        "palette.navigate" => {
          self.dialogs.palette = false;
          match value.as_str().filter(|url| !url.is_empty()) {
            Some(url) => with(Effect::OpenUrl(url.to_string())),
            None => done(),
          }
        }
        "workspacePicker.select" => {
          let repo = self.repo().to_string();
          if let Some(workspace) = value
            .as_i64()
            .and_then(|id| self.workspace_in(&repo, id, data))
          {
            self.set_selected(Some(workspace.id));
          }
          self.dialogs.workspace_picker = false;
          done()
        }
        "branchSwitcher.changed" => {
          self.dialogs.branch_switcher = false;
          self.view.changes_revision += 1;
          with(Effect::Refresh { revision: true })
        }
        _ => None,
      },
      _ => None,
    }
  }

  fn close_top_overlay(&mut self) {
    match snapshots::top_overlay(self) {
      Some(snapshots::Top::WorkspacePicker) => self.dialogs.workspace_picker = false,
      Some(snapshots::Top::Shortcuts) => self.dialogs.shortcuts = false,
      Some(snapshots::Top::BranchSwitcher) => self.dialogs.branch_switcher = false,
      Some(snapshots::Top::Palette) => self.dialogs.palette = false,
      None => {}
    }
  }

  /// Dashboard's `paletteCommands`, by `CommandPalette` item id.
  fn palette_command(&mut self, command: &str, data: &Data) -> Handled {
    let effects = match command {
      "dashboard" => {
        self.return_home();
        Vec::new()
      }
      "settings" => {
        self.open_settings();
        Vec::new()
      }
      "open-repository" => vec![Effect::PickFolder(FolderPurpose::OpenRepo)],
      "open-repository-new-window" => vec![Effect::PickFolder(FolderPurpose::OpenInNewWindow)],
      "switch-branch" => {
        self.dialogs.branch_switcher = true;
        Vec::new()
      }
      "search-files" => {
        self.dialogs.file_picker = true;
        Vec::new()
      }
      "go-to-workspace" => {
        self.dialogs.workspace_picker = true;
        Vec::new()
      }
      "create-workspace" => self.stack(data),
      "delete-workspace" => {
        self.dialogs.deletion = true;
        Vec::new()
      }
      "start-agent-with-prompt" => {
        self.open_agent_prompt();
        Vec::new()
      }
      "start-agent-terminal" => self.new_agent(self.selected, data),
      "open-prompt-history" => {
        self.dialogs.prompt_history = Some(None);
        Vec::new()
      }
      "open-stash" => {
        self.dialogs.stash = true;
        Vec::new()
      }
      "toggle-terminal" => {
        self.terminal("toggleCollapse", json!({}));
        Vec::new()
      }
      "maximize-terminal" => {
        self.terminal("toggleMaximize", json!({}));
        Vec::new()
      }
      "new-shell-terminal" => {
        self.terminal("newShell", json!({}));
        Vec::new()
      }
      _ => return None,
    };
    Some(effects)
  }

  /// What a screen reported changing (`useGpuiNativeEvents`), so the
  /// window reads again and follows it.
  pub fn native_event(&mut self, event: &NativeEvent, data: &Data) -> Vec<Effect> {
    match event {
      NativeEvent::RepoOpened { path } => {
        let mut effects = self.open_repo(path);
        effects.push(Effect::ReadPrefs);
        effects
      }
      NativeEvent::SettingsChanged => vec![Effect::ReadPrefs, Effect::Refresh { revision: true }],
      NativeEvent::RepoSettingsChanged { .. } | NativeEvent::WorkspaceChanged { .. } => {
        vec![Effect::Refresh { revision: true }]
      }
      NativeEvent::AgentSessionStarted {
        session,
        pane_session_id,
        select,
      } => {
        self.active_session = Some(*pane_session_id);
        if *select {
          self.page = Page::Workspace;
          self.activate_repo(&session.repo_path, data);
          match session.workspace_id {
            Some(id) => {
              let pending = PendingSelect {
                repo_path: session.repo_path.clone(),
                id,
                page: true,
                logs: false,
              };
              return self.select_when_listed(pending, data);
            }
            None => self.set_selected(None),
          }
        }
        vec![Effect::Refresh { revision: true }]
      }
      NativeEvent::WorkspacesRemoving { repo_path, ids } => {
        self.leave_removed(repo_path, ids, data);
        Vec::new()
      }
      NativeEvent::WorkspacesRemoved { repo_path, ids } => {
        self.leave_removed(repo_path, ids, data);
        vec![Effect::Refresh { revision: true }]
      }
      NativeEvent::WorkspacesRestored { .. } => vec![Effect::Refresh { revision: true }],
      NativeEvent::Open { what } => self.open_request(what, data),
      NativeEvent::RemoteMutation { status, reason } => {
        self.remote_mutation(status, reason.as_deref())
      }
      NativeEvent::RemoteCutoff {
        endpoint_id,
        reason,
      } => {
        self
          .remote
          .cutoffs
          .insert(endpoint_id.clone(), reason.clone());
        Vec::new()
      }
      NativeEvent::Toast { .. } => Vec::new(),
    }
  }

  /// `openGpuiRequest`: what a GPUI menu or button asks to open.
  fn open_request(&mut self, what: &OpenRequest, data: &Data) -> Vec<Effect> {
    match what {
      OpenRequest::BranchSwitcher => self.dialogs.branch_switcher = true,
      OpenRequest::AddRepository if self.has_repo() => {
        return vec![Effect::PickFolder(FolderPurpose::AddSupporting)]
      }
      OpenRequest::AddRepository => {}
      OpenRequest::RepoSettings { repo_path } => {
        self.activate_repo(repo_path, data);
        self.set_selected(None);
        self.open_settings();
      }
      OpenRequest::LocateRepo { repo_path } => {
        return vec![Effect::PickFolder(FolderPurpose::LocateSupporting(
          repo_path.clone(),
        ))]
      }
      OpenRequest::RemoveRepo { repo_path } => self.ask(Confirm::remove_supporting(repo_path)),
      OpenRequest::RenameWorkspace {
        repo_path,
        workspace_id,
      } => {
        if let Some(workspace) = self.workspace_in(repo_path, *workspace_id, data) {
          self.dialogs.rename = Some((repo_path.clone(), workspace.id, workspace.branch_name));
        }
      }
      OpenRequest::PrReview { number, state } => self.open_github_pr(*number, state),
    }
    Vec::new()
  }

  /// A menu bar item (`lib.rs`'s menu events).
  pub fn menu(&mut self, item: Menu, data: &Data) -> Vec<Effect> {
    match item {
      Menu::Dashboard => self.return_home(),
      Menu::Settings => self.open_settings(),
      Menu::AddRepository if self.has_repo() => {
        return vec![Effect::PickFolder(FolderPurpose::AddSupporting)]
      }
      Menu::AddRepository => {}
      Menu::OpenSsh => self.open_remote_setup(),
      // With no repository there is no Logs tab: show the log folder.
      Menu::ViewLogs if !self.has_repo() || !data.flags.logs => return vec![Effect::OpenLogDir],
      Menu::ViewLogs => {
        self.page = Page::Workspace;
        self.open_logs("system");
      }
      Menu::FactoryReset => self.ask(Confirm::factory_reset()),
      Menu::ForceRebase => {
        let Some(workspace) = self.selected_workspace(data).cloned() else {
          return vec![Effect::Toast {
            tone: ToastTone::Warning,
            title: "No workspace selected".into(),
            description: "Select a workspace first, then force rebase.".into(),
          }];
        };
        let branch = workspace
          .target_branch
          .clone()
          .filter(|b| !b.is_empty())
          .unwrap_or_else(|| self.default_branch(data));
        return vec![Effect::ForceRebase {
          repo_path: self.repo().to_string(),
          workspace_id: workspace.id,
          branch,
        }];
      }
      Menu::OpenInNewWindowInvalid => return vec![model::not_a_repository()],
    }
    Vec::new()
  }
}

/// The menu bar items a window handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
  Dashboard,
  Settings,
  AddRepository,
  OpenSsh,
  ViewLogs,
  FactoryReset,
  ForceRebase,
  OpenInNewWindowInvalid,
}

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use super::{
    super::{data::RepoRead, model::Dialogs, tree::tests::workspace},
    *,
  };
  use crate::local_db::Session;

  fn data() -> Data {
    let mut data = Data::default();
    data.apply(
      "/repo",
      RepoRead {
        workspaces: Some(vec![
          workspace(1, "a", Some("main")),
          workspace(2, "b", Some("a")),
          workspace(3, "c", Some("b")),
          workspace(4, "x", Some("main")),
        ]),
        has_changes: Some(HashMap::new()),
        ..RepoRead::default()
      },
    );
    data
  }

  fn model() -> Model {
    Model::new(Some("/repo".into()))
  }

  fn press(id: &str) -> ScreenAction {
    ScreenAction::Press { id: id.into() }
  }

  fn field(name: &str, value: Value) -> ScreenAction {
    ScreenAction::Field {
      field: name.into(),
      value,
    }
  }

  #[test]
  fn selects_workspaces_and_ranges_from_the_sidebar() {
    let (mut model, data) = (model(), data());
    let click = |id, shift, toggle| SidebarAction::SelectWorkspace { id, shift, toggle };
    model.sidebar_action(&click(2, false, false), &data);
    assert_eq!(model.selected, Some(2));
    assert!(model.multi.is_empty());
    model.sidebar_action(&click(3, false, true), &data);
    assert_eq!(
      model.multi.iter().copied().collect::<Vec<_>>(),
      [2, 3],
      "the open workspace joins the selection"
    );
    model.sidebar_action(&click(2, true, false), &data);
    assert_eq!(model.multi.len(), 2, "a range from the last click");
    model.sidebar_action(&SidebarAction::ClearSelection, &data);
    assert!(model.multi.is_empty());
    model.sidebar_action(
      &SidebarAction::SelectStack {
        id: 2,
        descendants: false,
      },
      &data,
    );
    assert_eq!(model.multi.iter().copied().collect::<Vec<_>>(), [1, 2, 3]);
    model.sidebar_action(
      &SidebarAction::SelectHome {
        repo_path: "/repo".into(),
      },
      &data,
    );
    assert!(model.multi.is_empty(), "home first drops the selection");
    assert_eq!(model.selected, Some(2));
    model.sidebar_action(
      &SidebarAction::SelectHome {
        repo_path: "/repo".into(),
      },
      &data,
    );
    assert_eq!(model.selected, None);
  }

  #[test]
  fn leaves_menus_and_drags_to_their_own_handlers() {
    let (mut model, data) = (model(), data());
    assert_eq!(
      model.sidebar_action(&SidebarAction::BulkArchive, &data),
      None
    );
  }

  #[test]
  fn settings_return_to_the_page_before() {
    let (mut model, data) = (model(), data());
    model.sidebar_action(&SidebarAction::OpenGithub, &data);
    model.sidebar_action(&SidebarAction::OpenSettings, &data);
    model.screen_action(&field("settings.tab", "application".into()), &data);
    assert_eq!(
      model.page,
      Page::Settings {
        tab: "application".into()
      }
    );
    model.screen_action(&press("settings.close"), &data);
    assert!(matches!(model.page, Page::Github(_)));
  }

  #[test]
  fn linear_opens_the_integrations_settings() {
    let (mut model, data) = (model(), data());
    model.sidebar_action(&SidebarAction::OpenLinear, &data);
    model.screen_action(&press("linear.openSettings"), &data);
    assert_eq!(
      model.page,
      Page::Settings {
        tab: "integrations".into()
      }
    );
    model.screen_action(&press("settings.close"), &data);
    assert!(matches!(model.page, Page::Linear));
  }

  #[test]
  fn skill_link_opens_the_skills_tab_until_answered() {
    let (mut model, data) = (model(), data());
    model.sidebar_action(&SidebarAction::OpenGithub, &data);
    model.open_skill_install("anthropic/pdf".into());
    assert_eq!(
      model.page,
      Page::Settings {
        tab: "skills".into()
      }
    );
    assert_eq!(model.skill_install.as_deref(), Some("anthropic/pdf"));
    model.screen_action(&press("skills.installRequest.close"), &data);
    assert_eq!(model.skill_install, None);
    model.screen_action(&press("settings.close"), &data);
    assert!(matches!(model.page, Page::Github(_)));
  }

  #[test]
  fn skill_customize_starts_an_agent_in_the_home_repository() {
    let (mut model, data) = (model(), data());
    model.open_settings();
    let effects = model
      .screen_action(
        &field("skills.startAgent", json!({"prompt": "Edit the skill"})),
        &data,
      )
      .unwrap_or_default();
    assert_eq!(model.page, Page::Workspace);
    assert!(effects.iter().any(|effect| matches!(
      effect,
      Effect::StartAgent {
        workspace_id: None,
        prompt: Some(prompt),
        select: true,
        ..
      } if prompt == "Edit the skill"
    )));
  }

  #[test]
  fn tuning_the_review_prompt_starts_an_agent_in_the_home_repository() {
    let (mut model, data) = (model(), data());
    model.open_settings();
    let effects = model
      .screen_action(
        &field("reviewPrompt.tune", json!({"prompt": "Tune it"})),
        &data,
      )
      .unwrap_or_default();
    assert_eq!(model.page, Page::Workspace);
    assert!(effects.iter().any(|effect| matches!(
      effect,
      Effect::StartAgent {
        workspace_id: None,
        prompt: Some(prompt),
        name: Some(name),
        ..
      } if prompt == "Tune it" && name == "Tune review prompt"
    )));
  }

  #[test]
  fn navigates_the_github_view() {
    let (mut model, data) = (model(), data());
    model.open_github();
    model.screen_action(&press("github.tab:prs"), &data);
    model.screen_action(&press("github.filter:closed"), &data);
    model.screen_action(&press("github.select:12"), &data);
    model.screen_action(&press("github.review:12"), &data);
    let Page::Github(github) = &model.page else {
      panic!("not GitHub");
    };
    assert_eq!(
      (
        github.tab,
        github.filter.as_str(),
        github.selected,
        github.review
      ),
      (GithubTab::Prs, "closed", Some(12), Some(12))
    );
    model.screen_action(&press("prReview.close"), &data);
    let Page::Github(github) = &model.page else {
      panic!("not GitHub");
    };
    assert_eq!((github.selected, github.review), (Some(12), None));
    model.screen_action(&press("github.review:12"), &data);
    model.screen_action(&press("prReview.submitted"), &data);
    let Page::Github(github) = &model.page else {
      panic!("not GitHub");
    };
    assert_eq!(
      (github.selected, github.review, github.reload),
      (Some(12), None, 1),
      "back on the PR, which reads it again"
    );
    model.screen_action(&press("github.openWorkspace:3"), &data);
    assert_eq!((model.selected, &model.page), (Some(3), &Page::Workspace));
  }

  #[test]
  fn a_new_workspace_shows_once_listed() {
    let (mut model, mut data) = (model(), data());
    model.open_workspace_dialog(json!({}));
    let effects = model
      .screen_action(&field("workspaceDialog.created", 9.into()), &data)
      .unwrap();
    assert_eq!(effects, [Effect::Refresh { revision: true }]);
    assert!(model.dialogs.workspace_dialog.is_none());
    assert_eq!(model.selected, None, "not listed yet");
    data.apply(
      "/repo",
      RepoRead {
        workspaces: Some(vec![workspace(9, "new", Some("main"))]),
        ..RepoRead::default()
      },
    );
    assert!(model.resolve_pending(&data));
    assert_eq!(model.selected, Some(9));
  }

  #[test]
  fn moves_a_commit_to_a_new_workspace() {
    let (mut model, data) = (model(), data());
    model.selected = Some(2);
    model.screen_action(
      &field(
        "commits.moveToNew",
        json!({"change_id": "zz", "description": "Fix it\n\nbody"}),
      ),
      &data,
    );
    let (_, defaults) = model.dialogs.workspace_dialog.clone().unwrap();
    assert_eq!(defaults["targetBranch"], "b");
    assert_eq!(defaults["preSelectedCommits"], json!(["zz"]));
    assert_eq!(defaults["description"], "Fix it");
    assert_eq!(defaults["source"]["kind"], "workspace");
  }

  #[test]
  fn asks_before_moving_dropped_files() {
    let (mut model, data) = (model(), data());
    let request = |dest: &str| {
      field(
        "changes.moveFiles",
        json!({"files": ["a.rs"], "sourceBranch": "a", "destinationBranch": dest, "destinationLabel": dest}),
      )
    };
    model.screen_action(&request("a"), &data);
    assert!(model.dialogs.change_move.is_none(), "onto itself");
    model.screen_action(&request("b"), &data);
    assert!(model.dialogs.change_move.is_some());
    model.screen_action(&press("changeMove.done"), &data);
    assert!(model.dialogs.change_move.is_none());
  }

  #[test]
  fn runs_the_palette_command_table() {
    let (mut model, data) = (model(), data());
    model.dialogs.palette = true;
    model.overlay_action(&press("palette.go-to-workspace"), &data);
    assert!(model.dialogs.workspace_picker && !model.dialogs.palette);
    model.overlay_action(&field("workspacePicker.select", 4.into()), &data);
    assert_eq!(model.selected, Some(4));
    assert!(!model.dialogs.workspace_picker);
    model.dialogs.palette = true;
    let effects = model
      .overlay_action(&press("palette.open-repository"), &data)
      .unwrap();
    assert_eq!(effects, [Effect::PickFolder(FolderPurpose::OpenRepo)]);
    assert_eq!(model.overlay_action(&press("palette.unknown"), &data), None);
    model.dialogs.shortcuts = true;
    model.dialogs.palette = true;
    model.overlay_action(&press("overlay.close"), &data);
    assert!(
      !model.dialogs.shortcuts && model.dialogs.palette,
      "the top one"
    );
  }

  #[test]
  fn follows_the_terminal_to_its_workspace() {
    let (mut model, mut data) = (model(), data());
    data.apply(
      "/repo",
      RepoRead {
        sessions: Some(vec![Session {
          id: 5,
          workspace_id: Some(3),
          name: "Claude 1".into(),
          created_at: String::new(),
          last_accessed: String::new(),
          model: None,
        }]),
        ..RepoRead::default()
      },
    );
    model.screen_action(&field("terminal.activeSession", 5.into()), &data);
    assert_eq!((model.active_session, model.selected), (Some(5), Some(3)));
    model.screen_action(&field("terminal.closeSession", 5.into()), &data);
    assert_eq!(model.active_session, None);
    model.screen_action(
      &field(
        "terminal.navigate",
        json!({"key": ".treq/workspaces/a", "isMainRepo": false}),
      ),
      &data,
    );
    assert_eq!(model.selected, Some(1));
    model.screen_action(&field("terminal.shortcut", "k".into()), &data);
    assert!(model.dialogs.palette);
  }

  /// An agent's turn ends when its terminal stops streaming: it may have
  /// rewritten history, so the tab counts and the Changes tab read again.
  #[test]
  fn an_agent_turn_ending_reads_the_counts_again() {
    let (mut model, data) = (model(), data());
    let summary = |kind: &str, streaming: bool| json!([{"kind": kind, "branchName": "a", "repoPath": "/repo", "isStreaming": streaming}]);
    let before = model.view.changes_revision;
    model.screen_action(&field("terminal.summaries", summary("agent", true)), &data);
    assert_eq!(model.view.changes_revision, before, "a turn running");
    model.screen_action(&field("terminal.summaries", summary("agent", false)), &data);
    assert_eq!(model.view.changes_revision, before + 1, "the turn ended");
    model.screen_action(&field("terminal.summaries", summary("agent", false)), &data);
    assert_eq!(model.view.changes_revision, before + 1, "still idle");
    model.screen_action(&field("terminal.summaries", summary("shell", true)), &data);
    model.screen_action(&field("terminal.summaries", summary("shell", false)), &data);
    assert_eq!(
      model.view.changes_revision,
      before + 1,
      "a shell is no agent"
    );
  }

  #[test]
  fn workspace_open_logs_starts_the_logs_tab_on_its_source() {
    let (mut model, mut data) = (model(), data());
    data.flags.logs = true;
    let key = model.view.logs_key;
    model.screen_action(&field("workspace.openLogs", "setup-script".into()), &data);
    assert_eq!(model.view.tab, Tab::Logs);
    assert_eq!(model.view.logs_source, "setup-script");
    assert_eq!(model.view.logs_key, key + 1);
  }

  #[test]
  fn workspace_open_logs_does_nothing_without_the_logs_preview() {
    let (mut model, data) = (model(), data());
    let tab = model.view.tab;
    model.screen_action(&field("workspace.openLogs", "setup-script".into()), &data);
    assert_eq!(model.view.tab, tab);
  }

  #[test]
  fn shortcuts_open_dialogs_and_zoom() {
    let (mut model, data) = (model(), data());
    model.shortcut(Shortcut::NewWorkspace, &data);
    assert_eq!(model.dialogs.open_workspaces, Some(None));
    // Stacking opens the same dialog.
    model.dialogs.open_workspaces = None;
    model.shortcut(Shortcut::Stack, &data);
    assert!(model.dialogs.open_workspaces.is_some());
    assert!(model.dialogs.workspace_dialog.is_none());
    model.dialogs.open_workspaces = None;
    model.shortcut(Shortcut::ToggleShortcuts, &data);
    assert!(model.dialogs.shortcuts);
    model.shortcut(Shortcut::Escape, &data);
    assert!(!model.dialogs.shortcuts);
    let effects = model.shortcut(Shortcut::ZoomIn, &data);
    assert_eq!(model.prefs.zoom, 105);
    assert_eq!(
      effects,
      [Effect::SaveSetting {
        key: "ui_zoom".into(),
        value: "105".into()
      }]
    );
    model.shortcut(Shortcut::ToggleTerminal, &data);
    assert_eq!(model.commands.last().unwrap()["kind"], "toggleCollapse");
  }

  #[test]
  fn shortcuts_do_not_open_a_dialog_over_another() {
    let (mut model, data) = (model(), data());
    model.selected = Some(2);
    model.shortcut(Shortcut::Stack, &data);
    let stack = model.dialogs.open_workspaces.clone();
    assert!(stack.is_some());
    for shortcut in [
      Shortcut::AgentPrompt,
      Shortcut::NewWorkspace,
      Shortcut::Stack,
      Shortcut::Palette,
      Shortcut::FilePicker,
      Shortcut::ToggleShortcuts,
    ] {
      model.shortcut(shortcut, &data);
    }
    assert_eq!(model.dialogs.open_workspaces, stack, "the same dialog");
    assert_eq!(
      model.dialogs,
      Dialogs {
        open_workspaces: stack,
        workspace_dialog_key: model.dialogs.workspace_dialog_key,
        ..Dialogs::default()
      },
      "nothing over it"
    );
    model.dialogs.shortcuts = true;
    model.dialogs.open_workspaces = None;
    model.shortcut(Shortcut::ToggleShortcuts, &data);
    assert!(!model.dialogs.shortcuts, "closes its own");
  }

  #[test]
  fn follows_what_screens_report() {
    let (mut model, data) = (model(), data());
    model.selected = Some(3);
    model.native_event(
      &NativeEvent::WorkspacesRemoved {
        repo_path: "/repo".into(),
        ids: vec![3],
      },
      &data,
    );
    assert_eq!(model.selected, Some(2), "onto its stack parent");
    model.native_event(
      &NativeEvent::Open {
        what: OpenRequest::PrReview {
          number: 4,
          state: "OPEN".into(),
        },
      },
      &data,
    );
    assert!(matches!(model.page, Page::Github(_)));
    let effects = model.native_event(
      &NativeEvent::RepoOpened {
        path: "/other".into(),
      },
      &data,
    );
    assert_eq!(model.main_repo, "/other");
    assert!(effects.contains(&Effect::ReadPrefs));
  }

  #[test]
  fn menus_need_a_workspace_to_force_rebase() {
    let (mut model, data) = (model(), data());
    assert!(matches!(
      model.menu(Menu::ForceRebase, &data)[..],
      [Effect::Toast { .. }]
    ));
    model.selected = Some(2);
    assert_eq!(
      model.menu(Menu::ForceRebase, &data),
      [Effect::ForceRebase {
        repo_path: "/repo".into(),
        workspace_id: 2,
        branch: "a".into(),
      }]
    );
    assert_eq!(model.menu(Menu::ViewLogs, &data), [Effect::OpenLogDir]);
  }
}
