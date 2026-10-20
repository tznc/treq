//! `WindowController`: a shell window's [`Model`] and [`Data`] at work. It
//! reads the backend through a [`WindowHost`] on SWR's intervals and on
//! every invalidation, runs the [`Effect`]s the model asks for, and hands
//! the shell what to draw ([`Presentation`]) whenever that changes.

use std::{collections::HashMap, rc::Rc, sync::Arc};

use tauri_plugin_gpui::gpui::{prelude::*, App, Context, Task};

use super::{
  actions::{Menu, Shortcut},
  data::{Data, Read, RepoRead, POLL, SESSIONS_POLL},
  host::{read_repo, WindowHost},
  model::{self, Effect, FolderPurpose, Forward, Model},
  sidebar, snapshots,
};
use crate::{
  core::{agent_launch::LaunchRequest, supporting_repos::SupportingRepo},
  gpui_shell::{
    native::{NativeEvent, ToastTone},
    overlay::OverlaySnapshot,
    screen::{file_picker::Scope, Modal, ScreenAction, ScreenSnapshot},
    sidebar::{SidebarAction, SidebarSnapshot},
  },
};

/// Everything the shell draws for a window the model drives.
#[derive(Debug, Clone, PartialEq)]
pub struct Presentation {
  pub sidebar: Option<SidebarSnapshot>,
  pub screen: Option<ScreenSnapshot>,
  pub modal: Option<Modal>,
  pub overlay: Option<OverlaySnapshot>,
  /// Cmd+P's "Jump to File", over the whole window.
  pub file_picker: Option<Scope>,
  /// `ui_zoom`, in percent.
  pub zoom: u32,
  pub dark: bool,
}

/// Draws a presentation; called only when it changed.
pub type Presenter = Rc<dyn Fn(Presentation, &mut App)>;
/// Hands React what it still runs.
pub type Forwarder = Rc<dyn Fn(Forward, &mut App)>;

pub struct WindowController {
  host: Arc<dyn WindowHost>,
  model: Model,
  data: Data,
  present: Presenter,
  forward: Forwarder,
  shown: Option<Presentation>,
  /// A read in flight, and what to read once it returns.
  reading: bool,
  queued: Option<Read>,
  _pollers: Vec<Task<()>>,
}

impl WindowController {
  /// A window on `repo` (`None`: onboarding). Reads the settings and the
  /// repository, then keeps reading on SWR's intervals.
  pub fn new(
    host: Arc<dyn WindowHost>,
    repo: Option<String>,
    present: Presenter,
    forward: Forwarder,
    cx: &mut Context<Self>,
  ) -> Self {
    let repo = repo.filter(|r| !r.is_empty());
    let pollers = vec![
      Self::poll(POLL, Read::POLL, cx),
      Self::poll(SESSIONS_POLL, Read::SESSIONS, cx),
    ];
    let mut this = Self {
      host,
      model: Model::new(repo.clone()),
      data: Data::default(),
      present,
      forward,
      shown: None,
      reading: false,
      queued: None,
      _pollers: pollers,
    };
    let host = this.host.clone();
    cx.background_spawn(async move { host.detect_editors() })
      .detach();
    if let Some(repo) = repo {
      let host = this.host.clone();
      cx.background_spawn(async move { host.prepare_repo(&repo) })
        .detach();
    }
    this.run(vec![Effect::ReadPrefs, Effect::Refresh { revision: false }], cx);
    this
  }

  fn poll(every: std::time::Duration, read: Read, cx: &mut Context<Self>) -> Task<()> {
    cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(every).await;
      if this.update(cx, |this, cx| this.read(read, cx)).is_err() {
        break;
      }
    })
  }

  pub fn model(&self) -> &Model {
    &self.model
  }

  pub fn data(&self) -> &Data {
    &self.data
  }

  /// A sidebar action; false leaves it to React.
  pub fn sidebar_action(&mut self, action: &SidebarAction, cx: &mut Context<Self>) -> bool {
    self.handled(|model, data| model.sidebar_action(action, data), cx)
  }

  /// A screen action; false leaves it to React.
  pub fn screen_action(&mut self, action: &ScreenAction, cx: &mut Context<Self>) -> bool {
    self.handled(|model, data| model.screen_action(action, data), cx)
  }

  /// An overlay action; false leaves it to React.
  pub fn overlay_action(&mut self, action: &ScreenAction, cx: &mut Context<Self>) -> bool {
    self.handled(|model, data| model.overlay_action(action, data), cx)
  }

  pub fn native_event(&mut self, event: &NativeEvent, cx: &mut Context<Self>) {
    if let NativeEvent::RepoOpened { path } = event {
      let (host, path) = (self.host.clone(), path.clone());
      cx.background_spawn(async move { host.prepare_repo(&path) })
        .detach();
    }
    let effects = self.model.native_event(event, &self.data);
    self.run(effects, cx);
  }

  pub fn shortcut(&mut self, shortcut: Shortcut, cx: &mut Context<Self>) {
    let effects = self.model.shortcut(shortcut, &self.data);
    self.run(effects, cx);
  }

  pub fn menu(&mut self, item: Menu, cx: &mut Context<Self>) {
    let effects = self.model.menu(item, &self.data);
    self.run(effects, cx);
  }

  /// A folder the menu bar's "Open…" picked: opens it in this window.
  pub fn open_folder(&mut self, path: String, cx: &mut Context<Self>) {
    self.run(vec![Effect::OpenRepoAt(path)], cx);
  }

  /// The window gained focus: the Changes tab and file browser read again
  /// (`useChangesRevision`); SWR itself does not revalidate on focus.
  pub fn focused(&mut self, cx: &mut Context<Self>) {
    self.model.view.changes_revision += 1;
    self.present(cx);
  }

  /// `workspace-files-changed`: the Changes tab of that workspace reads
  /// again.
  pub fn files_changed(&mut self, workspace_id: i64, cx: &mut Context<Self>) {
    if self.model.selected == Some(workspace_id) {
      self.model.view.changes_revision += 1;
      self.present(cx);
    }
  }

  /// `pr-statuses-updated`: the PR poller's cache changed.
  pub fn prs_changed(&mut self, cx: &mut Context<Self>) {
    self.read(Read::default(), cx);
  }

  /// The OS switched between light and dark.
  pub fn set_system_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.model.prefs.system_dark != dark {
      self.model.prefs.system_dark = dark;
      self.present(cx);
    }
  }

  fn handled(
    &mut self,
    f: impl FnOnce(&mut Model, &Data) -> super::actions::Handled,
    cx: &mut Context<Self>,
  ) -> bool {
    match f(&mut self.model, &self.data) {
      Some(effects) => {
        self.run(effects, cx);
        true
      }
      None => false,
    }
  }

  /// Runs `effects`, then draws what changed.
  fn run(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
    for effect in effects {
      self.effect(effect, cx);
    }
    self.present(cx);
  }

  /// Runs `f` on the host off the main thread, then `then` with its answer.
  fn background<T: Send + 'static>(
    &self,
    f: impl FnOnce(&dyn WindowHost) -> T + Send + 'static,
    then: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let value = cx.background_spawn(async move { f(&*host) }).await;
      let _ = this.update(cx, |this, cx| then(this, value, cx));
    })
    .detach();
  }

  fn effect(&mut self, effect: Effect, cx: &mut Context<Self>) {
    let main = self.model.main_repo.clone();
    match effect {
      Effect::Refresh { revision } => {
        if revision {
          self.data.revision += 1;
        }
        self.read(Read::ALL, cx);
      }
      Effect::PickFolder(purpose) => self.background(
        |host| host.pick_folder(),
        move |this, path, cx| {
          if let Some(path) = path {
            this.picked(purpose, path, cx);
          }
        },
        cx,
      ),
      Effect::SaveSetting { key, value } => {
        let host = self.host.clone();
        cx.background_spawn(async move { host.save_setting(&key, &value) })
          .detach();
      }
      Effect::StartAgent {
        repo_path,
        workspace_id,
        agent,
        prompt,
        mode,
        name,
        select,
      } => {
        let request = LaunchRequest {
          repo_path,
          workspace_id,
          prompt,
          mode,
          agent,
          session_name: name,
        };
        self.background(
          move |host| host.launch(&request, select),
          |this, result, cx| {
            if let Err(err) = result {
              this.run(vec![toast(ToastTone::Error, "Failed to start session", &err)], cx);
            }
          },
          cx,
        );
      }
      Effect::OpenRepoAt(path) => {
        let host = self.host.clone();
        cx.background_spawn(async move { host.open_repo_at(&path) })
          .detach();
      }
      Effect::ReadPrefs => {
        let keys = settings_keys(&main);
        self.background(
          move |host| {
            let values: HashMap<String, Option<String>> =
              keys.into_iter().map(|k| (k.clone(), host.setting(&k))).collect();
            (values, host.flags())
          },
          |this, (values, flags), cx| {
            this
              .model
              .apply_settings(|key| values.get(key).cloned().flatten());
            this.data.flags = flags;
            this.present(cx);
          },
          cx,
        );
      }
      Effect::DismissSendAsset(id) => {
        let host = self.host.clone();
        cx.background_spawn(async move { host.dismiss_send_asset(&id) })
          .detach();
      }
      Effect::RemoveSupporting(path) => self.background(
        move |host| {
          let message =
            format!("Remove {path} from this window? Its files and workspaces are not deleted.");
          if !host.ask("Remove Repository", &message) {
            return None;
          }
          Some((path.clone(), host.remove_supporting(&main, &path)))
        },
        |this, removed, cx| {
          let Some((path, result)) = removed else {
            return;
          };
          let effects = match result {
            Ok(()) => {
              if this.model.supporting.as_deref() == Some(path.as_str()) {
                this.model.supporting = None;
                this.model.selected = None;
              }
              vec![Effect::Refresh { revision: false }]
            }
            Err(err) => vec![toast(ToastTone::Error, "Failed to remove repository", &err)],
          };
          this.run(effects, cx);
        },
        cx,
      ),
      Effect::OpenLogDir => {
        let host = self.host.clone();
        cx.background_spawn(async move { host.open_log_dir() })
          .detach();
      }
      Effect::FactoryReset => self.background(
        |host| {
          let confirmed = host.ask(
            "Factory Reset",
            "Clear the saved repository path and return to the start screen?",
          );
          if confirmed {
            host.factory_reset();
          }
          confirmed
        },
        |this, confirmed, cx| {
          if confirmed {
            this.model.reset();
            this.data.clear();
            this.run(
              vec![toast(
                ToastTone::Success,
                "Factory Reset Complete",
                "Repository path cleared.",
              )],
              cx,
            );
          }
        },
        cx,
      ),
      Effect::ForceRebase {
        repo_path,
        workspace_id,
        branch,
      } => self.background(
        move |host| host.force_rebase(&repo_path, workspace_id, &branch),
        |this, result, cx| {
          let done = match result {
            Ok(result) => {
              let message = if result.message.is_empty() {
                "Workspace subtree force rebase finished.".to_string()
              } else {
                result.message
              };
              if result.success {
                toast(ToastTone::Success, "Force rebase complete", &message)
              } else {
                toast(
                  ToastTone::Warning,
                  "Force rebase completed with errors",
                  &message,
                )
              }
            }
            Err(err) => toast(ToastTone::Error, "Force rebase failed", &err),
          };
          this.run(vec![done, Effect::Refresh { revision: false }], cx);
        },
        cx,
      ),
      Effect::Toast {
        tone,
        title,
        description,
      } => self.host.toast(tone, &title, &description),
      Effect::React(forward) => (self.forward)(forward, cx),
      Effect::OpenUrl(url) => self.host.open_url(&url),
    }
  }

  /// A folder the user picked for `purpose`.
  fn picked(&mut self, purpose: FolderPurpose, path: String, cx: &mut Context<Self>) {
    let main = self.model.main_repo.clone();
    match purpose {
      FolderPurpose::OpenRepo => self.run(vec![Effect::OpenRepoAt(path)], cx),
      FolderPurpose::OpenInNewWindow => {
        let host = self.host.clone();
        cx.background_spawn(async move { host.open_in_new_window(&path) })
          .detach();
      }
      FolderPurpose::AddSupporting | FolderPurpose::LocateSupporting(_) => {
        let replaced = match purpose {
          FolderPurpose::LocateSupporting(old) => Some(old),
          _ => None,
        };
        self.background(
          move |host| {
            if let Some(old) = replaced {
              host.remove_supporting(&main, &old)?;
            }
            host.add_supporting(&main, &path)
          },
          |this, result, cx| {
            let effect = match result {
              Ok(added) => {
                let name = added
                  .rsplit(['/', '\\'])
                  .find(|s| !s.is_empty())
                  .unwrap_or(&added)
                  .to_string();
                toast(
                  ToastTone::Success,
                  "Repository Added",
                  &format!("{name} is now a supporting repository"),
                )
              }
              Err(err) => toast(ToastTone::Error, "Failed to add repository", &err),
            };
            this.run(vec![effect, Effect::Refresh { revision: false }], cx);
          },
          cx,
        );
      }
    }
  }

  /// Reads what `read` asks of the window's repositories, once any read in
  /// flight returns.
  fn read(&mut self, read: Read, cx: &mut Context<Self>) {
    if !self.model.has_repo() {
      return;
    }
    if self.reading {
      let queued = self.queued.get_or_insert_with(Read::default);
      queued.workspaces |= read.workspaces;
      queued.sessions |= read.sessions;
      queued.detail |= read.detail;
      return;
    }
    self.reading = true;
    let main = self.model.main_repo.clone();
    let supporting = self.data.supporting().to_vec();
    let known: HashMap<String, Vec<i64>> = std::iter::once(main.clone())
      .chain(supporting.iter().map(|r| r.path.clone()))
      .map(|path| {
        let ids = self
          .data
          .repo(&path)
          .map(|r| r.has_changes.keys().copied().collect())
          .unwrap_or_default();
        (path, ids)
      })
      .collect();
    self.background(
      move |host| read_all(host, &main, supporting, read, &known),
      move |this, (main, supporting, reads), cx| {
        this.reading = false;
        // A repository opened while reading the one before.
        if main == this.model.main_repo {
          this.apply(supporting, reads, cx);
        }
        if let Some(next) = this.queued.take() {
          this.read(next, cx);
        }
      },
      cx,
    );
  }

  fn apply(
    &mut self,
    supporting: Option<Vec<SupportingRepo>>,
    reads: Vec<(String, RepoRead)>,
    cx: &mut Context<Self>,
  ) {
    if let Some(supporting) = supporting {
      self.data.set_supporting(supporting);
    }
    let paths: Vec<String> = reads.iter().map(|(path, _)| path.clone()).collect();
    for (path, read) in reads {
      let applied = self.data.apply(&path, read);
      if let Some(before) = applied.before {
        let now = self.data.workspaces(&path).to_vec();
        self.model.workspaces_listed(&path, &before, &now);
      }
    }
    self.data.retain(&paths);
    // A supporting repository removed from elsewhere leaves the screen.
    if let Some(path) = &self.model.supporting {
      if !self.data.supporting().iter().any(|r| r.exists && &r.path == path) {
        self.model.supporting = None;
        self.model.selected = None;
      }
    }
    self.model.resolve_pending(&self.data);
    self.present(cx);
  }

  /// Builds what the window shows and hands it on if it changed.
  fn present(&mut self, cx: &mut Context<Self>) {
    let presentation = Presentation {
      sidebar: sidebar::snapshot(&self.model, &self.data),
      screen: snapshots::screen(&self.model, &self.data),
      modal: snapshots::modal(&self.model, &self.data),
      overlay: snapshots::overlay(&self.model, &self.data),
      file_picker: self.file_picker(),
      zoom: self.model.prefs.zoom,
      dark: self.model.prefs.dark(),
    };
    if self.shown.as_ref() == Some(&presentation) {
      return;
    }
    self.shown = Some(presentation.clone());
    (self.present)(presentation, cx);
  }

  /// Cmd+P's scope: the selected workspace's checkout, or the repository.
  fn file_picker(&self) -> Option<Scope> {
    if !self.model.dialogs.file_picker || !self.model.has_repo() {
      return None;
    }
    let workspace = self.model.selected_workspace(&self.data);
    Some(Scope {
      repo_path: self.model.repo().to_string(),
      workspace_id: workspace.map(|w| w.id),
      workspace_path: workspace
        .map(model::full_path)
        .unwrap_or_else(|| self.model.repo().to_string()),
    })
  }
}

/// The supporting repositories (read again with the detail) and each
/// repository's read, main first.
type ReadAll = (String, Option<Vec<SupportingRepo>>, Vec<(String, RepoRead)>);

fn read_all(
  host: &dyn WindowHost,
  main: &str,
  supporting: Vec<SupportingRepo>,
  read: Read,
  known: &HashMap<String, Vec<i64>>,
) -> ReadAll {
  let listed = read.detail.then(|| host.supporting(main));
  let repos = listed.as_ref().unwrap_or(&supporting);
  if listed.is_some() {
    host.set_supporting_paths(
      repos
        .iter()
        .filter(|r| r.exists)
        .map(|r| r.path.clone())
        .collect(),
    );
  }
  let reads = std::iter::once(main.to_string())
    .chain(repos.iter().filter(|r| r.exists).map(|r| r.path.clone()))
    .map(|path| {
      let ids = known.get(&path).map(Vec::as_slice).unwrap_or_default();
      let repo = read_repo(host, &path, read, ids);
      (path, repo)
    })
    .collect();
  (main.to_string(), listed, reads)
}

/// The app settings the model follows for a window on `main`.
fn settings_keys(main: &str) -> Vec<String> {
  ["theme", "ui_zoom", "workspace_sidebar_width"]
    .into_iter()
    .map(String::from)
    .chain([model::collapsed_key(main)])
    .collect()
}

fn toast(tone: ToastTone, title: &str, description: &str) -> Effect {
  Effect::Toast {
    tone,
    title: title.into(),
    description: description.into(),
  }
}
