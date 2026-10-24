//! One workspace: its branch and status over three tabs (Terminals,
//! Changes, Commits), with the workspace's actions in the ⋯ menu and a
//! switcher to its siblings on the title.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    tag::Tag,
    v_flex, ActiveTheme, Sizable,
  },
  *,
};
use serde::Deserialize;
use treq_lib::{
  core::{
    remote::TreqCommandRequest,
    remote_control_plane::SshEndpoint,
    workspaces::{WorkspaceStatus, WorkspaceSummary},
  },
  jj::{JjBranch, JjFileChange, JjRevisionDiff},
  local_db::Workspace,
};
use treq_ui::{
  tabs::{Tab, Tabs},
  Tone,
};

use super::{
  commits::{self, CommitsScreen},
  conflicts, diff, mutation_message, on_epoch,
  terminal::Sessions,
  workspaces,
};
use crate::{
  errors,
  remote::{self, IdempotencyKeys, Mutation},
  store::SnapshotWorkspace,
  ui::{self, load, nav, sheet, widgets, Load, Location},
};

/// How often the change marker is polled to notice remote edits.
const MARKER_POLL: Duration = Duration::from_secs(15);

/// The repository and workspace a view works in. `id` is the numeric
/// workspace id the remote CLI expects.
#[derive(Clone)]
pub struct Target {
  pub repo: String,
  pub id: String,
  pub name: String,
}

pub fn open(repo: String, workspace_id: String, name: String, cx: &mut App) {
  place(false, repo, workspace_id, name, cx)
}

/// Shows another workspace in place of this one.
fn replace(repo: String, workspace_id: String, name: String, cx: &mut App) {
  place(true, repo, workspace_id, name, cx)
}

fn place(replace: bool, repo: String, workspace_id: String, name: String, cx: &mut App) {
  let location = Location {
    repo: Some(repo.clone()),
    workspace: Some(SnapshotWorkspace {
      id: workspace_id.clone(),
      name: name.clone(),
    }),
  };
  // The header's actions reach the screen once it exists.
  let screen: Rc<RefCell<Option<WeakEntity<WorkspaceScreen>>>> = Default::default();
  let (switch_repo, switch_id) = (repo.clone(), workspace_id.clone());
  let menu_screen = screen.clone();
  let header = nav::Header {
    on_title: Some(Rc::new(move |_, cx| {
      switcher(switch_repo.clone(), switch_id.clone(), cx)
    })),
    more: Some(Rc::new(move |_, cx| {
      if let Some(screen) = menu_screen.borrow().as_ref() {
        screen.update(cx, |screen, cx| screen.menu(cx)).ok();
      }
    })),
  };
  let build = move |window: &mut Window, cx: &mut App| {
    let entity = cx.new(|cx| WorkspaceScreen::new(repo, workspace_id, name, window, cx));
    *screen.borrow_mut() = Some(entity.downgrade());
    entity
  };
  if replace {
    nav::replace_with(name_of(&location), header, location, cx, build);
  } else {
    nav::push_with(name_of(&location), header, location, cx, build);
  }
}

fn name_of(location: &Location) -> String {
  location
    .workspace
    .as_ref()
    .map(|w| w.name.clone())
    .unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
  Terminals,
  Changes,
  Commits,
}

const VIEWS: [View; 3] = [View::Terminals, View::Changes, View::Commits];

#[derive(Deserialize)]
struct ChangeMarker {
  operation_id: Option<String>,
}

/// `tab` with its count, unless there's nothing to count.
fn counted(tab: Tab, count: usize) -> Tab {
  if count > 0 {
    tab.count(count)
  } else {
    tab
  }
}

fn status_request(target: &Target) -> TreqCommandRequest {
  TreqCommandRequest::InspectWorkspace {
    repo: target.repo.clone(),
    workspace: target.id.clone(),
  }
}

fn diff_request(target: &Target) -> TreqCommandRequest {
  TreqCommandRequest::WorkspaceDiff {
    repo: target.repo.clone(),
    workspace: target.id.clone(),
  }
}

/// Starts reading workspace `id`'s status and changes on `endpoint`, for a
/// screen about to open.
pub fn prefetch(endpoint: &SshEndpoint, repo: &str, id: &str) {
  let target = Target {
    repo: repo.to_string(),
    id: id.to_string(),
    name: String::new(),
  };
  remote::prefetch_on::<WorkspaceStatus>(endpoint.clone(), status_request(&target));
  remote::prefetch_on::<JjRevisionDiff>(endpoint.clone(), diff_request(&target));
}

/// "M" → "Modified", for a changed file's row.
fn status_name(status: &str) -> &str {
  match status {
    "A" => "Added",
    "D" => "Deleted",
    "M" => "Modified",
    "R" => "Renamed",
    "C" => "Copied",
    other => other,
  }
}

/// A changed file's row detail. Line counts show only when the host
/// counted them (uncommitted files come without).
fn change_detail(change: &JjFileChange) -> String {
  let mut detail = status_name(&change.status).to_string();
  if let Some(previous) = &change.previous_path {
    detail.push_str(&format!(" · from {previous}"));
  }
  match change.changed_line_count {
    0 => {}
    1 => detail.push_str(" · 1 line"),
    n => detail.push_str(&format!(" · {n} lines")),
  }
  detail
}

pub struct WorkspaceScreen {
  target: Target,
  view: View,
  status: Load<WorkspaceStatus>,
  diff: Load<JjRevisionDiff>,
  marker: Option<String>,
  sessions: Entity<Sessions>,
  /// Made when the Commits tab is first shown.
  commits: Option<Entity<CommitsScreen>>,
  commit_message: Entity<InputState>,
  /// The action running from this screen, by name.
  busy: Option<&'static str>,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
  /// Set once a change applied here, so the pages below (the workspace
  /// list) refetch when this one closes.
  changed: bool,
  _poll: Task<()>,
}

impl WorkspaceScreen {
  fn new(
    repo: String,
    id: String,
    name: String,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let poll = cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(MARKER_POLL).await;
      if this.update(cx, |this, cx| this.poll_marker(cx)).is_err() {
        break;
      }
    });
    let target = Target { repo, id, name };
    cx.on_release(|this, cx| {
      if this.changed {
        cx.defer(ui::refetch);
      }
    })
    .detach();
    // Shown at once when this workspace was open before; refetched either way.
    let status = remote::last::<WorkspaceStatus>(&status_request(&target));
    // The record names the workspace's sessions. The last status or the
    // list's summary has it, so sessions are listed without waiting for
    // this status (a round trip on the default tab).
    let workspace = status
      .as_ref()
      .map(|s| s.partial.current.clone())
      .or_else(|| {
        remote::last::<Vec<WorkspaceSummary>>(&workspaces::summaries_request(&target.repo))?
          .into_iter()
          .find(|s| s.workspace.id.to_string() == target.id)
          .map(|s| s.workspace)
      });
    let sessions = cx.new(|cx| Sessions::new(target.clone(), workspace, cx));
    cx.observe(&sessions, |_, _, cx| cx.notify()).detach();
    let mut this = Self {
      view: View::Terminals,
      status: status.map_or(Load::Idle, Load::Ready),
      diff: remote::last(&diff_request(&target)).map_or(Load::Idle, Load::Ready),
      target,
      marker: None,
      sessions,
      commits: None,
      commit_message: cx.new(|cx| InputState::new(window, cx).placeholder("Commit message")),
      busy: None,
      message: None,
      keys: IdempotencyKeys::default(),
      changed: false,
      _poll: poll,
    };
    this.fetch(true, cx);
    this.poll_marker(cx);
    this
  }

  fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
    self.poll_marker(cx);
  }

  /// Reads the workspace's status, changes and sessions. `join` as in
  /// [`remote::read_cached`].
  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.status.ready().is_none() {
      self.status = Load::Loading;
    }
    if self.diff.ready().is_none() {
      self.diff = Load::Loading;
    }
    let first = join;
    load::run(
      cx,
      remote::read_cached::<WorkspaceStatus>(status_request(&self.target), join),
      move |this, result, cx| {
        if let Ok(status) = &result {
          let workspace = status.partial.current.clone();
          this
            .sessions
            .update(cx, |sessions, cx| sessions.set_workspace(workspace, cx));
          // Once the tab's own reads are back, the Commits tab's is
          // started so that tab opens on content.
          if first && this.commits.is_none() {
            commits::prefetch(&this.target);
          }
        }
        this.status = Load::from_result(result);
      },
    );
    load::run(
      cx,
      remote::read_cached::<JjRevisionDiff>(diff_request(&self.target), join),
      |this, result, _| this.diff = Load::from_result(result),
    );
    if !join {
      self.sessions.update(cx, |sessions, cx| sessions.reload(cx));
      if let Some(commits) = &self.commits {
        commits.update(cx, |commits, cx| commits.reload(cx));
      }
    }
  }

  /// Refetches when the workspace changed on the host since the last poll.
  fn poll_marker(&mut self, cx: &mut Context<Self>) {
    let Target { repo, id, .. } = self.target.clone();
    load::run(
      cx,
      async move {
        remote::read::<ChangeMarker>(TreqCommandRequest::WorkspaceChangeMarker {
          repo,
          workspace: Some(id),
        })
        .await
      },
      |this, result, cx| {
        if let Ok(marker) = result {
          let changed = this.marker.is_some() && this.marker != marker.operation_id;
          this.marker = marker.operation_id;
          if changed {
            // The marker was just read: refetch the rest only.
            this.fetch(false, cx);
          }
        }
      },
    );
  }

  fn select(&mut self, view: View, cx: &mut Context<Self>) {
    self.view = view;
    if view == View::Commits && self.commits.is_none() {
      let target = self.target.clone();
      self.commits = Some(cx.new(|cx| CommitsScreen::new(target, cx)));
    }
    cx.notify();
  }

  fn workspace(&self) -> Option<&Workspace> {
    self.status.ready().map(|s| &s.partial.current)
  }

  fn target_branch(&self) -> String {
    self
      .workspace()
      .and_then(|w| w.target_branch.clone())
      .unwrap_or_else(|| "main".into())
  }

  /// The ⋯ menu: everything done to the workspace as a whole.
  fn menu(&mut self, cx: &mut Context<Self>) {
    let Some(workspace) = self.workspace().cloned() else {
      return;
    };
    let this = cx.weak_entity();
    let repo = self.target.repo.clone();
    let branch = workspace.branch_name.clone();
    let target = self.target_branch();
    let on = |f: fn(&mut Self, &mut Context<Self>)| {
      let this = this.clone();
      move |_: &mut Window, cx: &mut App| {
        this.update(cx, |screen, cx| f(screen, cx)).ok();
      }
    };
    let (stack_repo, stack_branch) = (repo.clone(), branch.clone());
    let (rename_repo, rename_ws) = (repo.clone(), workspace.clone());
    let reload = this.clone();
    let entries = vec![
      sheet::MenuEntry::new("menu-push", "Push", on(Self::confirm_push)),
      sheet::MenuEntry::new(
        "menu-rebase",
        format!("Rebase onto {target}"),
        on(Self::confirm_rebase),
      ),
      sheet::MenuEntry::new("menu-target", "Change target…", on(Self::change_target)),
      sheet::MenuEntry::new(
        "menu-stack",
        "New workspace stacked on this",
        move |_, cx| {
          workspaces::new_workspace(
            stack_repo.clone(),
            Some(stack_branch.clone()),
            Rc::new(ui::refetch),
            cx,
          )
        },
      ),
      sheet::MenuEntry::new("menu-rename", "Rename…", move |_, cx| {
        let reload = reload.clone();
        workspaces::rename(
          rename_repo.clone(),
          rename_ws.clone(),
          Rc::new(move |cx| {
            reload
              .update(cx, |screen, cx| {
                screen.changed = true;
                screen.reload(cx)
              })
              .ok();
          }),
          cx,
        )
      }),
      sheet::MenuEntry::new("menu-copy", "Copy branch name", move |_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(branch.clone()))
      }),
      sheet::MenuEntry::new("menu-archive", "Archive", on(Self::confirm_archive)),
      sheet::MenuEntry::new("menu-delete", "Delete…", on(Self::confirm_delete)).danger(),
    ];
    sheet::menu(workspaces::title_of(&workspace), entries, cx);
  }

  fn confirm_push(&mut self, cx: &mut Context<Self>) {
    let Some(workspace) = self.workspace() else {
      return;
    };
    let branch = workspace.branch_name.clone();
    let this = cx.weak_entity();
    sheet::confirm(
      format!("Push {branch}?"),
      format!("Pushes {branch} and its commits to the remote."),
      "Push",
      false,
      move |_, cx| {
        this.update(cx, |screen, cx| screen.push(cx)).ok();
      },
      cx,
    );
  }

  fn push(&mut self, cx: &mut Context<Self>) {
    let Target { repo, id, .. } = self.target.clone();
    let key = self.keys.key_for("push", &[&repo, &id]);
    self.mutate(
      "push",
      vec![repo.clone(), id.clone()],
      TreqCommandRequest::GitPush {
        repo,
        workspace: Some(id),
        idempotency_key: key,
      },
      "Pushed.",
      cx,
    );
  }

  fn confirm_rebase(&mut self, cx: &mut Context<Self>) {
    let target = self.target_branch();
    let this = cx.weak_entity();
    sheet::confirm(
      format!("Rebase onto {target}?"),
      format!(
        "Moves this workspace's commits onto the latest {target}. Conflicts, if any, show in Changes."
      ),
      "Rebase",
      false,
      move |_, cx| {
        this.update(cx, |screen, cx| screen.rebase(cx)).ok();
      },
      cx,
    );
  }

  fn rebase(&mut self, cx: &mut Context<Self>) {
    let Target { repo, id, .. } = self.target.clone();
    let target = self.target_branch();
    let key = self.keys.key_for("rebase", &[&repo, &id, &target]);
    self.mutate(
      "rebase",
      vec![repo.clone(), id.clone(), target.clone()],
      TreqCommandRequest::RebaseWorkspace {
        repo,
        workspace: id,
        target_branch: target,
        idempotency_key: key,
      },
      "Rebased.",
      cx,
    );
  }

  fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let message = self.commit_message.read(cx).value().trim().to_string();
    if message.is_empty() {
      self.message = Some((false, "Enter a commit message.".into()));
      return cx.notify();
    }
    let Target { repo, id, .. } = self.target.clone();
    let key = self.keys.key_for("commit", &[&repo, &id, &message]);
    self
      .commit_message
      .update(cx, |state, cx| state.set_value("", window, cx));
    self.mutate(
      "commit",
      vec![repo.clone(), id.clone(), message.clone()],
      TreqCommandRequest::CreateCommit {
        repo,
        workspace: Some(id),
        message,
        base_change_id: None,
        idempotency_key: key,
      },
      "Committed.",
      cx,
    );
  }

  /// Runs a workspace mutation, then shows its outcome and refetches.
  fn mutate(
    &mut self,
    name: &'static str,
    inputs: Vec<String>,
    request: TreqCommandRequest,
    success: &'static str,
    cx: &mut Context<Self>,
  ) {
    if self.busy.is_some() {
      return;
    }
    self.busy = Some(name);
    self.message = None;
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      this.busy = None;
      let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
      this.keys.settle(name, &refs, &outcome);
      let (ok, text) = mutation_message(&outcome, success);
      this.changed |= ok;
      this.message = Some((
        ok,
        if ok {
          text
        } else {
          errors::describe(&text).message
        },
      ));
      this.reload(cx);
    });
    cx.notify();
  }

  fn change_target(&mut self, cx: &mut Context<Self>) {
    let screen = cx.weak_entity();
    let repo = self.target.repo.clone();
    let current = self.target_branch();
    let own = self
      .workspace()
      .map(|w| w.branch_name.clone())
      .unwrap_or_default();
    sheet::open(Some("Change target".into()), cx, move |_, cx| {
      cx.new(|cx| TargetSheet::new(repo, own, current, screen, cx))
    });
  }

  fn set_target(&mut self, branch: String, cx: &mut Context<Self>) {
    let Target { repo, id, .. } = self.target.clone();
    self.mutate(
      "target",
      vec![repo.clone(), id.clone(), branch.clone()],
      TreqCommandRequest::UpdateWorkspace {
        repo,
        workspace: id,
        target_branch: Some(branch),
        title: None,
        description: None,
      },
      "Target changed.",
      cx,
    );
  }

  fn confirm_archive(&mut self, cx: &mut Context<Self>) {
    let Some(status) = self.status.ready() else {
      return;
    };
    if !status.partial.has_changes {
      return self.archive(cx);
    }
    let this = cx.weak_entity();
    sheet::confirm(
      "Archive with uncommitted changes?",
      format!(
        "{} has uncommitted changes. Archiving removes its checkout, so commit them first if you need them.",
        workspaces::title_of(&status.partial.current)
      ),
      "Archive workspace",
      true,
      move |_, cx| {
        this.update(cx, |screen, cx| screen.archive(cx)).ok();
      },
      cx,
    );
  }

  fn archive(&mut self, cx: &mut Context<Self>) {
    let request = TreqCommandRequest::ArchiveWorkspace {
      repo: self.target.repo.clone(),
      workspace: self.target.id.clone(),
    };
    self.leave_after(request, "archive", cx);
  }

  fn confirm_delete(&mut self, cx: &mut Context<Self>) {
    let Some(workspace) = self.workspace() else {
      return;
    };
    let title = workspaces::title_of(workspace);
    let branch = workspace.branch_name.clone();
    let this = cx.weak_entity();
    sheet::confirm(
      format!("Delete {title}?"),
      format!(
        "Deletes the workspace's checkout and its record. The branch {branch} and its commits stay in the repository."
      ),
      "Delete workspace",
      true,
      move |_, cx| {
        this.update(cx, |screen, cx| screen.delete(cx)).ok();
      },
      cx,
    );
  }

  fn delete(&mut self, cx: &mut Context<Self>) {
    let request = TreqCommandRequest::DeleteWorkspace {
      repo: self.target.repo.clone(),
      workspace: self.target.id.clone(),
    };
    self.leave_after(request, "delete", cx);
  }

  /// Runs a mutation that ends the workspace, then returns to the list.
  fn leave_after(
    &mut self,
    request: TreqCommandRequest,
    name: &'static str,
    cx: &mut Context<Self>,
  ) {
    self.busy = Some(name);
    self.message = None;
    load::run(cx, remote::mutate(request), |this, outcome, cx| {
      this.busy = None;
      match outcome {
        Ok(Mutation::Applied(_)) => {
          this.changed = true;
          nav::pop(cx);
        }
        other => {
          this.message = Some((
            false,
            errors::describe(&mutation_message(&other, "").1).message,
          ))
        }
      }
    });
    cx.notify();
  }

  fn header(&self, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let Some(status) = self.status.ready() else {
      return match &self.status {
        Load::Failed(error) => widgets::error_text(errors::describe(error).message, cx),
        _ => widgets::muted("Loading…", cx),
      }
      .px_4()
      .py_2();
    };
    let partial = &status.partial;
    let workspace = &partial.current;
    v_flex()
      .px_4()
      .py_2()
      .gap_2()
      .child(
        h_flex()
          .gap_1()
          .min_w_0()
          .text_sm()
          .child(
            widgets::mono(workspace.branch_name.clone(), cx)
              .min_w_0()
              .truncate(),
          )
          .child(div().text_color(theme.muted_foreground).child("→"))
          .child(
            widgets::mono(self.target_branch(), cx)
              .text_color(theme.muted_foreground)
              .truncate(),
          ),
      )
      .child(
        h_flex()
          .flex_wrap()
          .gap_2()
          .when(partial.has_conflicts, |row| {
            row.child(Tag::danger().small().child(format!(
              "{} conflicted",
              status.conflicted_files.len().max(1)
            )))
          })
          .child(if partial.has_changes {
            Tag::warning().small().child("Uncommitted changes")
          } else {
            Tag::success().small().child("Clean")
          })
          .when(partial.commits_ahead > 0, |row| {
            row.child(
              Tag::secondary()
                .small()
                .child(format!("↑{} ahead", partial.commits_ahead)),
            )
          }),
      )
  }

  fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let palette = treq_ui::theme::current(cx);
    let running = self.sessions.read(cx).running();
    let (files, conflicts) = self.diff.ready().map_or((0, false), |diff| {
      (
        diff.uncommitted_files.len() + diff.committed_files.len(),
        !diff.conflicted_files.is_empty(),
      )
    });
    let ahead = self.status.ready().map_or(0, |s| s.partial.commits_ahead);
    let view = self.view;
    let this = cx.weak_entity();
    div().px_4().pb_2().child(
      Tabs::new("workspace-tabs", &palette)
        .tab(counted(Tab::new("tab-terminals", "Terminals"), running))
        .tab({
          let tab = counted(Tab::new("tab-changes", "Changes"), files);
          if conflicts {
            tab.dot(Tone::Danger)
          } else {
            tab
          }
        })
        .tab(counted(Tab::new("tab-commits", "Commits"), ahead))
        .selected(VIEWS.iter().position(|v| *v == view).unwrap_or(0))
        .on_select(move |ix, _, cx| {
          this
            .update(cx, |screen, cx| screen.select(VIEWS[ix], cx))
            .ok();
        }),
    )
  }

  fn changes(&self, cx: &mut Context<Self>) -> AnyElement {
    let diff = match &self.diff {
      Load::Idle | Load::Loading => {
        return widgets::card(cx)
          .child(widgets::centered_message("Loading changes…", cx))
          .into_any_element()
      }
      Load::Failed(error) => {
        return widgets::card(cx)
          .p_4()
          .child(widgets::error_text(errors::describe(error).message, cx))
          .into_any_element()
      }
      Load::Ready(diff) => diff,
    };
    let target = self.target.clone();
    let committing = self.busy == Some("commit");
    let file_row = |id: (&'static str, usize), change: &JjFileChange, cx: &App| {
      widgets::row(
        id,
        change.path.clone(),
        Some(change_detail(change).into()),
        cx,
      )
    };
    v_flex()
      .gap_4()
      .when(!diff.conflicted_files.is_empty(), |list| {
        let t = target.clone();
        list.child(
          widgets::section("Conflicts", cx).child(widgets::card(cx).children(
            diff.conflicted_files.iter().enumerate().map(|(i, path)| {
              let t = t.clone();
              widgets::row(("conflict", i), path.clone(), Some("Conflicted".into()), cx)
                .text_color(cx.theme().danger)
                .on_click(move |_, _, cx| conflicts::open(t.clone(), cx))
            }),
          )),
        )
      })
      .child(
        widgets::section("Uncommitted", cx).child(
          widgets::card(cx)
            .when(diff.uncommitted_files.is_empty(), |card| {
              card.child(widgets::centered_message("No uncommitted changes.", cx))
            })
            .children(
              diff
                .uncommitted_files
                .iter()
                .enumerate()
                .map(|(i, change)| {
                  let (t, path) = (target.clone(), change.path.clone());
                  file_row(("uncommitted", i), change, cx)
                    .on_click(move |_, _, cx| diff::open(t.clone(), path.clone(), cx))
                }),
            )
            .when(!diff.uncommitted_files.is_empty(), |card| {
              card.child(
                h_flex()
                  .p_3()
                  .gap_2()
                  .child(div().flex_1().child(Input::new(&self.commit_message)))
                  .child(
                    Button::new("commit")
                      .primary()
                      .label("Commit")
                      .loading(committing)
                      .disabled(self.busy.is_some())
                      .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                  ),
              )
            }),
        ),
      )
      .child(
        widgets::section(format!("On this branch · vs {}", self.target_branch()), cx).child(
          widgets::card(cx)
            .when(diff.committed_files.is_empty(), |card| {
              card.child(widgets::centered_message("No committed changes yet.", cx))
            })
            .children(diff.committed_files.iter().enumerate().map(|(i, change)| {
              let (t, path) = (target.clone(), change.path.clone());
              let hunks = diff
                .hunks_by_file
                .iter()
                .find(|f| f.path == change.path)
                .map(|f| f.hunks.clone())
                .unwrap_or_default();
              file_row(("committed", i), change, cx).on_click(move |_, _, cx| {
                diff::open_hunks(t.clone(), path.clone(), hunks.clone(), cx)
              })
            })),
        ),
      )
      .into_any_element()
  }
}

impl Render for WorkspaceScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let body = match self.view {
      View::Terminals => div()
        .id("terminals")
        .size_full()
        .overflow_y_scroll()
        .child(widgets::page().child(self.sessions.clone()))
        .into_any_element(),
      View::Changes => div()
        .id("changes")
        .size_full()
        .overflow_y_scroll()
        .child(widgets::page().child(self.changes(cx)))
        .into_any_element(),
      View::Commits => match &self.commits {
        Some(commits) => commits.clone().into_any_element(),
        None => div().into_any_element(),
      },
    };
    let tabs = self.tabs(cx);
    let border = cx.theme().border;
    v_flex()
      .size_full()
      .child(self.header(cx))
      .child(tabs)
      .children(self.message.clone().map(|(ok, text)| {
        let line = if ok {
          widgets::muted(text, cx)
        } else {
          widgets::error_text(text, cx)
        };
        line.px_4().pb_2()
      }))
      .children(self.busy.map(|name| {
        widgets::muted(
          match name {
            "push" => "Pushing…",
            "rebase" => "Rebasing…",
            "commit" => "Committing…",
            "target" => "Changing target…",
            "archive" => "Archiving…",
            "delete" => "Deleting…",
            _ => "Working…",
          },
          cx,
        )
        .px_4()
        .pb_2()
      }))
      .child(
        div()
          .flex_1()
          .min_h_0()
          .border_t_1()
          .border_color(border)
          .child(body),
      )
  }
}

/// Picking the branch a workspace merges into.
struct TargetSheet {
  /// The workspace's own branch, which can't be its target.
  own: String,
  current: String,
  branches: Load<Vec<JjBranch>>,
  screen: WeakEntity<WorkspaceScreen>,
}

impl TargetSheet {
  fn new(
    repo: String,
    own: String,
    current: String,
    screen: WeakEntity<WorkspaceScreen>,
    cx: &mut Context<Self>,
  ) -> Self {
    load::run(
      cx,
      remote::read::<Vec<JjBranch>>(TreqCommandRequest::ListBranches { repo }),
      |this, result, cx| {
        this.branches = Load::from_result(result);
        cx.notify();
      },
    );
    Self {
      own,
      current,
      branches: Load::Loading,
      screen,
    }
  }
}

impl Render for TargetSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    match &self.branches {
      Load::Idle | Load::Loading => widgets::centered_message("Loading branches…", cx),
      Load::Failed(error) => widgets::error_text(errors::describe(error).message, cx).px_4(),
      Load::Ready(branches) => v_flex().children(
        branches
          .iter()
          .filter(|b| b.name != self.own)
          .enumerate()
          .map(|(i, branch)| {
            let name = branch.name.clone();
            let current = name == self.current;
            let screen = self.screen.clone();
            widgets::choice(("target", i), name.clone(), None, current, cx).on_click(
              move |_, _, cx| {
                sheet::close(cx);
                if !current {
                  let name = name.clone();
                  screen
                    .update(cx, |screen, cx| screen.set_target(name, cx))
                    .ok();
                }
              },
            )
          }),
      ),
    }
  }
}

/// Jump to another workspace in this repository.
fn switcher(repo: String, current: String, cx: &mut App) {
  sheet::open(Some("Workspaces".into()), cx, move |_, cx| {
    cx.new(|cx| {
      load::run(
        cx,
        workspaces::load_summaries(repo.clone(), true),
        |this: &mut Switcher, result, cx| {
          this.list = Load::from_result(result);
          cx.notify();
        },
      );
      // The list as last read shows until the fresh one is back.
      let list =
        remote::last(&workspaces::summaries_request(&repo)).map_or(Load::Loading, Load::Ready);
      Switcher {
        repo,
        current,
        list,
      }
    })
  });
}

struct Switcher {
  repo: String,
  current: String,
  list: Load<Vec<WorkspaceSummary>>,
}

impl Render for Switcher {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let palette = treq_ui::theme::current(cx);
    let list = match &self.list {
      Load::Idle | Load::Loading => return widgets::centered_message("Loading…", cx),
      Load::Failed(error) => {
        return widgets::error_text(errors::describe(error).message, cx).px_4()
      }
      Load::Ready(list) => list,
    };
    let repo = self.repo.clone();
    v_flex()
      .children(workspaces::stack_order(list).into_iter().map(|entry| {
        let summary = &list[entry.index];
        let workspace = &summary.workspace;
        let id = workspace.id.to_string();
        let title = workspaces::title_of(workspace);
        let current = id == self.current;
        let repo = repo.clone();
        widgets::row(
          ("switch-workspace", entry.index),
          title.clone(),
          Some(workspace.branch_name.clone().into()),
          cx,
        )
        .pl(px(16. + 16. * entry.depth as f32))
        .when(current, |row| row.bg(palette.selected()))
        .on_click(move |_, _, cx| {
          sheet::close(cx);
          if !current {
            replace(repo.clone(), id.clone(), title.clone(), cx);
          }
        })
      }))
      .child(
        widgets::row("switch-all-workspaces", "All workspaces", None, cx).on_click(|_, _, cx| {
          sheet::close(cx);
          nav::pop(cx);
        }),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn change(status: &str, lines: usize, previous: Option<&str>) -> JjFileChange {
    JjFileChange {
      path: "src/a.rs".into(),
      status: status.into(),
      previous_path: previous.map(str::to_string),
      changed_line_count: lines,
      diff_deferred: false,
    }
  }

  #[core::prelude::v1::test]
  fn uncounted_files_show_no_line_count() {
    assert_eq!(change_detail(&change("M", 0, None)), "Modified");
    assert_eq!(change_detail(&change("A", 1, None)), "Added · 1 line");
    assert_eq!(
      change_detail(&change("R", 12, Some("src/b.rs"))),
      "Renamed · from src/b.rs · 12 lines"
    );
  }
}
