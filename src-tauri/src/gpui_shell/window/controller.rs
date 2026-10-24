//! `WindowController`: a shell window's [`Model`] and [`Data`] at work. It
//! reads the backend through a [`WindowHost`] on SWR's intervals and on
//! every invalidation, runs the [`Effect`]s the model asks for, and hands
//! the shell what to draw ([`Presentation`]) whenever that changes.

use std::{collections::HashMap, rc::Rc, sync::Arc};

use futures::StreamExt;

use tauri_plugin_gpui::gpui::{prelude::*, App, Context, Task};

use super::{
  actions::{Menu, Shortcut},
  data::{Data, Read, RepoRead, POLL, SESSIONS_POLL},
  host::{read_repo, FileWatch, WindowHost},
  model::{self, Confirm, Effect, FolderPurpose, Model},
  remote::{self, Opened, RemoteEffect},
  sidebar, snapshots,
};
use crate::{
  core::{
    agent_launch::LaunchRequest, cloud::ManagedEndpointEvent, supporting_repos::SupportingRepo,
  },
  gpui_shell::{
    confirm::Question,
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
  /// The confirmation over everything ([`crate::gpui_shell::confirm`]).
  pub confirm: Option<Question>,
  /// Cmd+P's "Jump to File", over the whole window.
  pub file_picker: Option<Scope>,
  /// `ui_zoom`, in percent.
  pub zoom: u32,
  pub dark: bool,
}

/// Draws a presentation; called only when it changed.
pub type Presenter = Rc<dyn Fn(Presentation, &mut App)>;

pub struct WindowController {
  host: Arc<dyn WindowHost>,
  model: Model,
  data: Data,
  present: Presenter,
  shown: Option<Presentation>,
  /// A read in flight, and what to read once it returns.
  reading: bool,
  queued: Option<Read>,
  /// `prepare_repo` finished while a read was in flight: once it returns,
  /// read again if it found no branch.
  recheck_branch: bool,
  /// The checkout watched for edits on disk, and the task that hears them.
  watching: Option<(String, Task<()>)>,
  _pollers: Vec<Task<()>>,
}

impl WindowController {
  /// A window on `repo` (`None`: onboarding, or the last remote
  /// repository once it restores). Reads the settings and the repository,
  /// then keeps reading on SWR's intervals.
  pub fn new(
    host: Arc<dyn WindowHost>,
    repo: Option<String>,
    present: Presenter,
    cx: &mut Context<Self>,
  ) -> Self {
    let repo = repo.filter(|r| !r.is_empty());
    // A saved folder that is no longer a repository opens onboarding, and
    // opens the folder as the picker does, which offers to initialize it,
    // before a read writes `.treq/` into it.
    let refused = repo.clone().filter(|r| !host.is_repo(r));
    if refused.is_some() {
      host.save_setting("last_opened_repo_path", "");
    }
    let repo = repo.filter(|_| refused.is_none());
    let pollers = vec![
      Self::poll(POLL, Read::POLL, cx),
      Self::poll(SESSIONS_POLL, Read::SESSIONS, cx),
    ];
    let mut this = Self {
      host,
      model: Model::new(repo.clone()),
      data: Data::default(),
      present,
      shown: None,
      reading: false,
      queued: None,
      recheck_branch: false,
      watching: None,
      _pollers: pollers,
    };
    let host = this.host.clone();
    cx.background_spawn(async move { host.detect_editors() })
      .detach();
    let mut effects = vec![Effect::ReadPrefs, Effect::Refresh { revision: false }];
    match repo {
      // The first read races `prepare_repo`'s jj init: a branch read before
      // `.jj` exists fails, and only a refresh reads the branch again. So
      // once the repository is prepared, read again if the branch is still
      // unknown, waiting for a read in flight to tell.
      Some(repo) => this.background(
        move |host| host.prepare_repo(&repo),
        |this, (), cx| {
          if this.reading {
            this.recheck_branch = true;
          } else {
            this.reread_missing_branch(cx);
          }
        },
        cx,
      ),
      None => effects.push(Effect::from(RemoteEffect::RestoreLast)),
    }
    if let Some(path) = refused {
      effects.push(Effect::OpenRepoAt(path));
    }
    this.run(effects, cx);
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

  #[cfg(test)]
  pub fn model(&self) -> &Model {
    &self.model
  }

  #[cfg(test)]
  pub fn data(&self) -> &Data {
    &self.data
  }

  /// A sidebar action; false when it is not the model's (a menu or drag `sidebar_actions` runs).
  pub fn sidebar_action(&mut self, action: &SidebarAction, cx: &mut Context<Self>) -> bool {
    self.handled(|model, data| model.sidebar_action(action, data), cx)
  }

  /// A screen action; false when the model has nothing for it.
  pub fn screen_action(&mut self, action: &ScreenAction, cx: &mut Context<Self>) -> bool {
    self.handled(|model, data| model.screen_action(action, data), cx)
  }

  /// An overlay action; false when the model has nothing for it.
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

  /// The confirmation answered: `yes` for its verb, false for Cancel.
  pub fn answer(&mut self, yes: bool, cx: &mut Context<Self>) {
    let effects = self.model.answer(yes);
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

  /// The watched checkout changed on disk: the Changes tab reads again.
  pub fn files_changed(&mut self, cx: &mut Context<Self>) {
    self.model.view.changes_revision += 1;
    self.present(cx);
  }

  /// The checkout the Changes tab shows: the selected workspace's, else
  /// the repository's. Remote ones change on another machine.
  fn watch_target(&self) -> Option<String> {
    if !self.model.has_repo() || self.model.is_remote() {
      return None;
    }
    Some(
      self
        .model
        .selected_workspace(&self.data)
        .map(model::full_path)
        .unwrap_or_else(|| self.model.repo().to_string()),
    )
  }

  /// Follows the selection: watches its checkout, dropping the last one.
  fn watch(&mut self, cx: &mut Context<Self>) {
    let target = self.watch_target();
    if self.watching.as_ref().map(|(path, _)| path) == target.as_ref() {
      return;
    }
    self.watching = target.map(|path| {
      let (host, at) = (self.host.clone(), path.clone());
      let task = cx.spawn(async move |this, cx| {
        let watch = cx
          .background_spawn(async move { host.watch_files(&at) })
          .await;
        let Some(FileWatch {
          mut changes,
          _watcher,
        }) = watch
        else {
          return;
        };
        while changes.next().await.is_some() {
          if this.update(cx, |this, cx| this.files_changed(cx)).is_err() {
            break;
          }
        }
      });
      (path, task)
    });
  }

  /// `pr-statuses-updated`: the PR poller's cache for `repo_path` changed.
  /// Windows that show other repositories have nothing to refresh.
  pub fn prs_changed(&mut self, repo_path: &str, cx: &mut Context<Self>) {
    let shown = self.model.main_repo == repo_path
      || self.data.supporting().iter().any(|r| r.path == repo_path);
    if shown {
      self.read(Read::default(), cx);
    }
  }

  /// `listenManagedEndpoint`: the cloud workspace connected, or its
  /// certificate renewed.
  pub fn managed_endpoint(&mut self, event: ManagedEndpointEvent, cx: &mut Context<Self>) {
    let effects = match event {
      ManagedEndpointEvent::Connected {
        endpoint,
        generation,
        open_repositories,
      } => self
        .model
        .remote
        .managed_connected(endpoint, generation, open_repositories),
      ManagedEndpointEvent::Renewed { endpoint } => self.model.remote.managed_renewed(endpoint),
    };
    self.run(effects.into_iter().map(Effect::from).collect(), cx);
  }

  /// `remoteCutoffStore`: an endpoint was cut off (`Some(reason)`) or
  /// reauthenticated (`None`).
  pub fn cutoff_changed(
    &mut self,
    endpoint_id: &str,
    reason: Option<String>,
    cx: &mut Context<Self>,
  ) {
    let cutoffs = &mut self.model.remote.cutoffs;
    match reason {
      Some(reason) => {
        cutoffs.insert(endpoint_id.to_string(), reason);
      }
      None => {
        if cutoffs.remove(endpoint_id).is_some() && self.model.is_remote() {
          // Back from a cutoff: what was refused reads again.
          return self.run(vec![Effect::Refresh { revision: true }], cx);
        }
      }
    }
    self.present(cx);
  }

  /// The account changed: the connection banner follows the cloud
  /// workspace's state.
  pub fn account_changed(&mut self, instance_state: Option<String>, cx: &mut Context<Self>) {
    if self.model.remote.instance_state != instance_state {
      self.model.remote.instance_state = instance_state;
      self.present(cx);
    }
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
              this.run(
                vec![toast(ToastTone::Error, "Failed to start session", &err)],
                cx,
              );
            }
          },
          cx,
        );
      }
      Effect::OpenRepoAt(path) => self.open_folder_at(path, false, cx),
      Effect::OpenInNewWindow(path) => self.open_folder_at(path, true, cx),
      Effect::InitRepo { path, new_window } => self.background(
        move |host| {
          host.init_repo(&path)?;
          match new_window {
            true => host.open_in_new_window(&path),
            false => host.open_repo_at(&path),
          }
          Ok(())
        },
        |this, result: Result<(), String>, cx| {
          if let Err(err) = result {
            this.run(
              vec![toast(ToastTone::Error, "Failed to open repository", &err)],
              cx,
            );
          }
        },
        cx,
      ),
      Effect::ReadPrefs => {
        let keys = settings_keys(&main);
        self.background(
          move |host| {
            let values: HashMap<String, Option<String>> = keys
              .into_iter()
              .map(|k| (k.clone(), host.setting(&k)))
              .collect();
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
          let result = host.remove_supporting(&main, &path);
          (path, result)
        },
        |this, (path, result), cx| {
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
        |host| host.factory_reset(),
        |this, (), cx| {
          this.model.reset();
          this.data.clear();
          this.host.show_remote(None);
          this.run(
            vec![toast(ToastTone::Success, "Factory reset complete", "")],
            cx,
          );
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
      Effect::OpenUrl(url) => self.host.open_url(&url),
      Effect::Remote(effect) => self.remote_effect(*effect, cx),
    }
  }

  /// Runs what the remote side asks: the SSH flows off the main thread.
  fn remote_effect(&mut self, effect: RemoteEffect, cx: &mut Context<Self>) {
    match effect {
      RemoteEffect::LoadSaved {
        endpoint_id,
        generation,
      } => self.background(
        move |host| {
          let saved = host.saved_repositories(&endpoint_id, generation);
          (endpoint_id, saved)
        },
        move |this, (endpoint_id, saved), cx| {
          this
            .model
            .remote
            .saved_listed(&endpoint_id, generation, saved);
          this.present(cx);
        },
        cx,
      ),
      RemoteEffect::RegisterUserManaged(values) => self.background(
        move |host| host.register_user_managed(&values),
        |this, endpoint, cx| {
          let effects = this.model.remote.registered(endpoint);
          this.run(effects.into_iter().map(Effect::from).collect(), cx);
        },
        cx,
      ),
      RemoteEffect::Browse { endpoint, path } => {
        let endpoint_id = endpoint.id.clone();
        self.background(
          move |host| remote::list_directories(host, &endpoint, &path),
          move |this, result, cx| {
            this.model.remote.browsed(&endpoint_id, result);
            this.present(cx);
          },
          cx,
        )
      }
      RemoteEffect::Open {
        endpoint,
        generation,
        path,
        how,
      } => self.background(
        move |host| remote::open(host, &endpoint, generation, &path, &how),
        |this, result, cx| match result {
          Ok(opened) => this.remote_opened(&opened, cx),
          Err((probe, error)) => {
            this.model.remote.failed(probe, error);
            this.present(cx);
          }
        },
        cx,
      ),
      RemoteEffect::Restore {
        endpoint,
        generation,
        descriptor,
        transition,
      } => self.background(
        move |host| {
          host
            .restore_remote(&descriptor, &endpoint, generation, transition)
            .map(|(descriptor, inspection)| Opened {
              descriptor,
              inspection,
              endpoint,
            })
        },
        |this, result, cx| match result {
          Ok(opened) => this.remote_opened(&opened, cx),
          Err(failure) => {
            this.model.remote.restore_failed(failure);
            this.present(cx);
          }
        },
        cx,
      ),
      RemoteEffect::RestoreLast => self.background(
        |host| remote::restore_last(host),
        |this, opened, cx| {
          // A repository opened meanwhile keeps the window.
          if let Some(opened) = opened.filter(|_| !this.model.has_repo()) {
            this.remote_opened(&opened, cx);
          }
        },
        cx,
      ),
      RemoteEffect::Connect { endpoint_id } => self.background(
        move |host| host.machine_endpoint(&endpoint_id),
        |this, endpoint, cx| {
          let effects = match endpoint {
            Some((endpoint, generation)) => this.model.remote.connect(endpoint, generation),
            None => {
              this.model.open_remote_setup();
              Vec::new()
            }
          };
          this.run(effects.into_iter().map(Effect::from).collect(), cx);
          this.present(cx);
        },
        cx,
      ),
      RemoteEffect::OpenSaved { id } => self.background(
        move |host| remote::open_saved(host, &id),
        |this, result, cx| match result {
          Ok(opened) => this.remote_opened(&opened, cx),
          Err(Some(failure)) => {
            this.model.remote.restore_failed(failure);
            this.present(cx);
          }
          Err(None) => {
            this.model.open_remote_setup();
            this.present(cx);
          }
        },
        cx,
      ),
      RemoteEffect::Cloud(action) => self.host.cloud_action(action),
      RemoteEffect::Show(repository) => self.host.show_remote(repository),
    }
  }

  fn remote_opened(&mut self, opened: &Opened, cx: &mut Context<Self>) {
    // A cutoff another window saw holds here too.
    let id = &opened.endpoint.id;
    if let Some(cutoff) = crate::gpui_shell::remote_repo::cutoff(id) {
      self
        .model
        .remote
        .cutoffs
        .insert(id.clone(), cutoff.id().to_string());
    }
    let effects = self.model.remote_opened(opened);
    self.run(effects, cx);
  }

  /// Opens the folder at `path`, here or in a new window; one that is not
  /// a repository first asks to initialize one.
  fn open_folder_at(&mut self, path: String, new_window: bool, cx: &mut Context<Self>) {
    self.background(
      move |host| {
        let repo = host.is_repo(&path);
        match (repo, new_window) {
          (false, _) => {}
          (true, true) => host.open_in_new_window(&path),
          (true, false) => host.open_repo_at(&path),
        }
        (repo, path)
      },
      move |this, (repo, path), cx| {
        if !repo {
          this.model.ask(Confirm::init_repo(&path, new_window));
          this.present(cx);
        }
      },
      cx,
    );
  }

  /// A folder the user picked for `purpose`.
  fn picked(&mut self, purpose: FolderPurpose, path: String, cx: &mut Context<Self>) {
    let main = self.model.main_repo.clone();
    match purpose {
      FolderPurpose::OpenRepo => self.run(vec![Effect::OpenRepoAt(path)], cx),
      FolderPurpose::OpenInNewWindow => self.run(vec![Effect::OpenInNewWindow(path)], cx),
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
                toast(ToastTone::Success, &format!("Added {name}"), "")
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

  /// Reads everything again when the main repository's branch is unknown.
  fn reread_missing_branch(&mut self, cx: &mut Context<Self>) {
    if self.data.home_ref(&self.model.main_repo).is_none() {
      self.read(Read::ALL, cx);
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
      // A read that panics must not leave `reading` set, or the window
      // never reads again.
      move |host| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          read_all(host, &main, supporting, read, &known)
        }))
      },
      move |this, result, cx| {
        this.reading = false;
        match result {
          // A repository opened while reading the one before.
          Ok((main, supporting, reads)) if main == this.model.main_repo => {
            this.apply(supporting, reads, cx);
            if std::mem::take(&mut this.recheck_branch) {
              this.reread_missing_branch(cx);
            }
          }
          Ok(_) => {}
          Err(_) => log::error!("reading the window's repositories panicked"),
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
      if !self
        .data
        .supporting()
        .iter()
        .any(|r| r.exists && &r.path == path)
      {
        self.model.supporting = None;
        self.model.selected = None;
      }
    }
    self.model.resolve_pending(&self.data);
    self.present(cx);
  }

  /// Builds what the window shows and hands it on if it changed.
  fn present(&mut self, cx: &mut Context<Self>) {
    self.watch(cx);
    let presentation = Presentation {
      sidebar: sidebar::snapshot(&self.model, &self.data),
      screen: snapshots::screen(&self.model, &self.data),
      modal: snapshots::modal(&self.model, &self.data),
      overlay: snapshots::overlay(&self.model, &self.data),
      confirm: self.model.confirm.as_ref().map(|c| c.question.clone()),
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
  [
    "theme",
    "ui_zoom",
    "workspace_sidebar_width",
    "default_agent",
  ]
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
