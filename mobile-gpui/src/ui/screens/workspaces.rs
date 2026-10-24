//! A repository's workspaces, as the desktop sidebar shows them: stacked
//! workspaces under their base, status dots, and a menu per row.

use std::collections::{HashMap, HashSet};

use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};
use treq_lib::{
  core::{remote::TreqCommandRequest, workspaces::WorkspaceSummary},
  local_db::Workspace,
};
use treq_ui::{Checkbox, Dot, Tone};

use super::{mutation_message, on_epoch, repos, workspace};
use crate::{
  backend, errors,
  remote::{self, Mutation},
  store,
  ui::{self, load, nav, sheet, widgets, Load, Location, Machine, MachineStatus},
};

/// What a workspace sheet calls after its change applied.
pub type OnDone = std::rc::Rc<dyn Fn(&mut App)>;

/// Indent per stacking level.
const INDENT: f32 = 16.;

pub fn open(repo: String, cx: &mut App) {
  let title = repo_name(&repo).to_string();
  let location = Location {
    repo: Some(repo.clone()),
    workspace: None,
  };
  let switcher_repo = repo.clone();
  nav::push_with(
    title,
    nav::Header {
      on_title: Some(std::rc::Rc::new(move |_, cx| {
        switcher(switcher_repo.clone(), cx)
      })),
      more: None,
    },
    location,
    cx,
    move |window, cx| cx.new(|cx| WorkspacesScreen::new(repo, window, cx)),
  );
}

fn repo_name(path: &str) -> &str {
  path
    .trim_end_matches('/')
    .rsplit('/')
    .next()
    .unwrap_or(path)
}

pub fn title_of(workspace: &Workspace) -> String {
  if workspace.title.is_empty() {
    workspace.workspace_name.clone()
  } else {
    workspace.title.clone()
  }
}

/// A row in display order: stacked workspaces follow their base, one level
/// deeper.
#[derive(Debug, PartialEq)]
pub struct Entry {
  pub index: usize,
  pub depth: usize,
}

/// Orders workspaces so each one stacked on another (its target branch is
/// that one's branch) comes right after it, indented. Others keep their order.
pub fn stack_order(list: &[WorkspaceSummary]) -> Vec<Entry> {
  let by_branch: HashMap<&str, usize> = list
    .iter()
    .enumerate()
    .map(|(i, s)| (s.workspace.branch_name.as_str(), i))
    .collect();
  let parent = |i: usize| {
    list[i]
      .workspace
      .target_branch
      .as_deref()
      .and_then(|t| by_branch.get(t).copied())
      .filter(|p| *p != i)
  };
  let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
  let mut roots = Vec::new();
  for i in 0..list.len() {
    match parent(i) {
      Some(p) => children.entry(p).or_default().push(i),
      None => roots.push(i),
    }
  }
  let mut out = Vec::new();
  let mut seen = HashSet::new();
  fn walk(
    i: usize,
    depth: usize,
    children: &HashMap<usize, Vec<usize>>,
    seen: &mut HashSet<usize>,
    out: &mut Vec<Entry>,
  ) {
    if !seen.insert(i) {
      return;
    }
    out.push(Entry { index: i, depth });
    for &child in children.get(&i).into_iter().flatten() {
      walk(child, depth + 1, children, seen, out);
    }
  }
  for root in roots {
    walk(root, 0, &children, &mut seen, &mut out);
  }
  // A cycle of target branches has no root; show those flat.
  for i in 0..list.len() {
    walk(i, 0, &children, &mut seen, &mut out);
  }
  out
}

/// Whether a workspace matches the search text (title or branch).
fn matches(summary: &WorkspaceSummary, query: &str) -> bool {
  let query = query.trim().to_lowercase();
  query.is_empty()
    || title_of(&summary.workspace).to_lowercase().contains(&query)
    || summary
      .workspace
      .branch_name
      .to_lowercase()
      .contains(&query)
}

pub fn summaries_request(repo: &str) -> TreqCommandRequest {
  TreqCommandRequest::WorkspaceSummaries {
    repo: repo.to_string(),
  }
}

/// The summaries, or a plain list from a host whose treq predates them.
/// `join` as in [`remote::read_cached`].
pub async fn load_summaries(repo: String, join: bool) -> Result<Vec<WorkspaceSummary>, String> {
  match remote::read_cached::<Vec<WorkspaceSummary>>(summaries_request(&repo), join).await {
    Err(error) if error.contains("unknown workspace action") => {
      let list =
        remote::read::<Vec<Workspace>>(TreqCommandRequest::ListWorkspaces { repo }).await?;
      Ok(
        list
          .into_iter()
          .map(|workspace| WorkspaceSummary {
            workspace,
            has_conflicts: false,
            has_changes: false,
            commits_ahead: 0,
            terminals: 0,
            last_activity_at: None,
          })
          .collect(),
      )
    }
    result => result,
  }
}

pub struct WorkspacesScreen {
  repo: String,
  list: Load<Vec<WorkspaceSummary>>,
  query: Entity<InputState>,
  /// The workspace ids picked for a bulk action, while selecting.
  selection: Option<HashSet<i64>>,
  /// A bulk or row action in progress.
  busy: bool,
  message: Option<(bool, String)>,
  scroll: ScrollHandle,
}

impl WorkspacesScreen {
  fn new(repo: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search workspaces"));
    cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        cx.notify();
      }
    })
    .detach();
    // Shown at once when this list was read before; refetched either way.
    let list = remote::last(&summaries_request(&repo)).map_or(Load::Idle, Load::Ready);
    let mut this = Self {
      repo,
      list,
      query,
      selection: None,
      busy: false,
      message: None,
      scroll: ScrollHandle::new(),
    };
    this.fetch(true, cx);
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.list.ready().is_none() {
      self.list = Load::Loading;
    }
    load::run(
      cx,
      load_summaries(self.repo.clone(), join),
      |this, result, _| this.list = Load::from_result(result),
    );
  }

  fn summary(&self, id: i64) -> Option<&WorkspaceSummary> {
    self.list.ready()?.iter().find(|s| s.workspace.id == id)
  }

  fn row_menu(&self, id: i64, cx: &mut Context<Self>) {
    let Some(summary) = self.summary(id) else {
      return;
    };
    let workspace = summary.workspace.clone();
    let this = cx.weak_entity();
    let repo = self.repo.clone();
    let (open_ws, stack_ws, rename_ws, copy_ws, select_this, archive_this, delete_this) = (
      workspace.clone(),
      workspace.clone(),
      workspace.clone(),
      workspace.branch_name.clone(),
      this.clone(),
      this.clone(),
      this.clone(),
    );
    let entries = vec![
      sheet::MenuEntry::new("menu-open", "Open", move |_, cx| {
        workspace::open(repo.clone(), open_ws.id.to_string(), title_of(&open_ws), cx)
      }),
      sheet::MenuEntry::new("menu-stack", "New workspace stacked on this", {
        let this = this.clone();
        move |_, cx| {
          let branch = stack_ws.branch_name.clone();
          this
            .update(cx, |screen, cx| screen.new_workspace(Some(branch), cx))
            .ok();
        }
      }),
      sheet::MenuEntry::new("menu-rename", "Rename…", {
        let this = this.clone();
        move |_, cx| {
          let workspace = rename_ws.clone();
          this
            .update(cx, |screen, cx| screen.rename(workspace, cx))
            .ok();
        }
      }),
      sheet::MenuEntry::new("menu-copy", "Copy branch name", move |_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(copy_ws.clone()))
      }),
      sheet::MenuEntry::new("menu-select", "Select", move |_, cx| {
        select_this
          .update(cx, |screen, cx| {
            screen.selection = Some(HashSet::from([id]));
            cx.notify();
          })
          .ok();
      }),
      sheet::MenuEntry::new("menu-archive", "Archive", move |_, cx| {
        archive_this
          .update(cx, |screen, cx| screen.confirm_archive(vec![id], cx))
          .ok();
      }),
      sheet::MenuEntry::new("menu-delete", "Delete…", move |_, cx| {
        delete_this
          .update(cx, |screen, cx| screen.confirm_delete(id, cx))
          .ok();
      })
      .danger(),
    ];
    sheet::menu(title_of(&workspace), entries, cx);
  }

  /// Archive asks first only when a workspace has uncommitted changes,
  /// since those are what archiving would lose.
  fn confirm_archive(&mut self, ids: Vec<i64>, cx: &mut Context<Self>) {
    let dirty: Vec<String> = ids
      .iter()
      .filter_map(|id| self.summary(*id))
      .filter(|s| s.has_changes)
      .map(|s| title_of(&s.workspace))
      .collect();
    if dirty.is_empty() {
      return self.archive(ids, cx);
    }
    let this = cx.weak_entity();
    let message = if dirty.len() == 1 {
      format!(
        "{} has uncommitted changes. Archiving removes its checkout, so commit them first if you need them.",
        dirty[0]
      )
    } else {
      format!(
        "{} workspaces have uncommitted changes. Archiving removes their checkouts, so commit those first if you need them.",
        dirty.len()
      )
    };
    let label = if ids.len() == 1 {
      "Archive workspace".to_string()
    } else {
      format!("Archive {} workspaces", ids.len())
    };
    sheet::confirm(
      "Archive with uncommitted changes?",
      message,
      label,
      true,
      move |_, cx| {
        this
          .update(cx, |screen, cx| screen.archive(ids.clone(), cx))
          .ok();
      },
      cx,
    );
  }

  fn archive(&mut self, ids: Vec<i64>, cx: &mut Context<Self>) {
    self.busy = true;
    self.message = None;
    let repo = self.repo.clone();
    let count = ids.len();
    load::run(
      cx,
      async move {
        let mut failed = Vec::new();
        for id in ids {
          let outcome = remote::mutate(TreqCommandRequest::ArchiveWorkspace {
            repo: repo.clone(),
            workspace: id.to_string(),
          })
          .await;
          if !matches!(outcome, Ok(Mutation::Applied(_))) {
            failed.push(mutation_message(&outcome, "").1);
          }
        }
        failed
      },
      move |this, failed, cx| {
        this.busy = false;
        this.selection = None;
        this.message = Some(match failed.first() {
          None if count == 1 => (true, "Workspace archived.".into()),
          None => (true, format!("{count} workspaces archived.")),
          Some(error) => (
            false,
            format!(
              "{} of {count} not archived: {}",
              failed.len(),
              errors::describe(error).message
            ),
          ),
        });
        this.reload(cx);
      },
    );
    cx.notify();
  }

  fn confirm_delete(&mut self, id: i64, cx: &mut Context<Self>) {
    let Some(summary) = self.summary(id) else {
      return;
    };
    let title = title_of(&summary.workspace);
    let branch = summary.workspace.branch_name.clone();
    let this = cx.weak_entity();
    sheet::confirm(
      format!("Delete {title}?"),
      format!(
        "Deletes the workspace's checkout and its record. The branch {branch} and its commits stay in the repository."
      ),
      "Delete workspace",
      true,
      move |_, cx| {
        this.update(cx, |screen, cx| screen.delete(id, cx)).ok();
      },
      cx,
    );
  }

  fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
    self.busy = true;
    let request = TreqCommandRequest::DeleteWorkspace {
      repo: self.repo.clone(),
      workspace: id.to_string(),
    };
    load::run(cx, remote::mutate(request), |this, outcome, cx| {
      this.busy = false;
      this.message = Some(mutation_message(&outcome, "Workspace deleted."));
      this.reload(cx);
    });
    cx.notify();
  }

  fn rename(&mut self, workspace: Workspace, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    rename(
      self.repo.clone(),
      workspace,
      std::rc::Rc::new(move |cx| {
        this.update(cx, |screen, cx| screen.reload(cx)).ok();
      }),
      cx,
    );
  }

  pub fn new_workspace(&mut self, stacked_on: Option<String>, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    new_workspace(
      self.repo.clone(),
      stacked_on,
      std::rc::Rc::new(move |cx| {
        this
          .update(cx, |screen, cx| {
            screen.message = Some((true, "Workspaces ready.".into()));
            screen.reload(cx);
          })
          .ok();
      }),
      cx,
    );
  }

  fn toggle(&mut self, id: i64, cx: &mut Context<Self>) {
    if let Some(selection) = &mut self.selection {
      if !selection.remove(&id) {
        selection.insert(id);
      }
      cx.notify();
    }
  }

  fn row(&self, entry: &Entry, list: &[WorkspaceSummary], cx: &mut Context<Self>) -> AnyElement {
    let summary = &list[entry.index];
    let workspace = &summary.workspace;
    let id = workspace.id;
    let palette = treq_ui::theme::current(cx);
    let theme = cx.theme();
    let selecting = self.selection.as_ref();
    let selected = selecting.is_some_and(|s| s.contains(&id));

    let mut status: Vec<AnyElement> = Vec::new();
    if summary.terminals > 0 {
      status.push(
        h_flex()
          .gap_0p5()
          .items_center()
          .text_xs()
          .text_color(theme.muted_foreground)
          .child(IconName::SquareTerminal)
          .child(summary.terminals.to_string())
          .into_any_element(),
      );
    }
    if summary.commits_ahead > 0 {
      status.push(
        div()
          .text_xs()
          .text_color(theme.muted_foreground)
          .child(format!("↑{}", summary.commits_ahead))
          .into_any_element(),
      );
    }
    if summary.has_conflicts {
      status.push(Dot::new(Tone::Danger, &palette).into_any_element());
    } else if summary.has_changes {
      status.push(Dot::new(Tone::Warning, &palette).into_any_element());
    }

    let menu = Button::new(("ws-menu", id as u64))
      .ghost()
      .small()
      .icon(IconName::Ellipsis)
      .on_click(cx.listener(move |this, _, _, cx| {
        cx.stop_propagation();
        this.row_menu(id, cx);
      }));
    let trailing = h_flex()
      .gap_2()
      .items_center()
      .children(status)
      .when(selecting.is_none(), |t| t.child(menu))
      .into_any_element();
    let leading = selecting.map(|_| {
      Checkbox::new(SharedString::from(format!("ws-check-{id}")), &palette)
        .checked(selected)
        .into_any_element()
    });
    let (repo, name) = (self.repo.clone(), title_of(workspace));
    widgets::row_full(
      SharedString::from(format!("ws-{id}")),
      leading,
      title_of(workspace),
      Some(workspace.branch_name.clone().into()),
      Some(trailing),
      cx,
    )
    .pl(px(16. + INDENT * entry.depth as f32))
    .when(selected, |row| row.bg(palette.selected()))
    .on_click(cx.listener(move |this, _, _, cx| {
      if this.selection.is_some() {
        this.toggle(id, cx);
      } else {
        workspace::open(repo.clone(), id.to_string(), name.clone(), cx);
      }
    }))
    .into_any_element()
  }

  fn bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let theme = cx.theme();
    let bar = h_flex()
      .p_3()
      .gap_2()
      .border_t_1()
      .border_color(theme.border)
      .bg(theme.background);
    match &self.selection {
      Some(selection) => {
        let ids: Vec<i64> = selection.iter().copied().collect();
        let count = ids.len();
        bar
          .child(
            Button::new("select-cancel")
              .outline()
              .large()
              .label("Cancel")
              .on_click(cx.listener(|this, _, _, cx| {
                this.selection = None;
                cx.notify();
              })),
          )
          .child(
            Button::new("archive-selected")
              .primary()
              .large()
              .flex_1()
              .label(match count {
                1 => "Archive 1 workspace".to_string(),
                n => format!("Archive {n} workspaces"),
              })
              .disabled(count == 0 || self.busy)
              .loading(self.busy)
              .on_click(cx.listener(move |this, _, _, cx| this.confirm_archive(ids.clone(), cx))),
          )
      }
      None => bar.child(
        Button::new("new-workspace")
          .primary()
          .large()
          .w_full()
          .h(px(widgets::CTA_H - 8.))
          .icon(IconName::Plus)
          .label("New workspaces")
          .on_click(cx.listener(|this, _, _, cx| this.new_workspace(None, cx))),
      ),
    }
  }
}

impl Render for WorkspacesScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let query = self.query.read(cx).value().to_string();
    let body = match &self.list {
      Load::Idle | Load::Loading => {
        widgets::card(cx).child(widgets::centered_message("Loading workspaces…", cx))
      }
      Load::Failed(error) => {
        let problem = errors::describe(error);
        widgets::card(cx)
          .p_4()
          .gap_3()
          .child(widgets::error_text(problem.message, cx))
          .child(
            h_flex().child(
              Button::new("retry")
                .outline()
                .label("Retry")
                .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            ),
          )
      }
      Load::Ready(all) if all.is_empty() => widgets::card(cx).p_6().gap_3().items_center().children([
        div()
          .font_weight(FontWeight::SEMIBOLD)
          .child("No workspaces yet")
          .into_any_element(),
        widgets::muted(
          "A workspace is a branch with its own checkout, so an agent can work without touching your main branch.",
          cx,
        )
        .text_center()
        .into_any_element(),
      ]),
      Load::Ready(list) => {
        let entries: Vec<Entry> = stack_order(list)
          .into_iter()
          .filter(|e| matches(&list[e.index], &query))
          .collect();
        if entries.is_empty() {
          widgets::card(cx).child(widgets::centered_message("No workspaces match.", cx))
        } else {
          let rows: Vec<AnyElement> = entries.iter().map(|e| self.row(e, list, cx)).collect();
          widgets::card(cx).children(rows)
        }
      }
    };
    let count = self.list.ready().map_or(0, Vec::len);
    let palette = treq_ui::theme::current(cx);
    v_flex()
      .size_full()
      .child(
        div()
          .id("workspaces")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .track_scroll(&self.scroll)
          .child(
            widgets::page()
              .gap_3()
              .when(count > 0, |page| {
                page.child(Input::new(&self.query).large().cleanable(true))
              })
              .child(
                h_flex()
                  .gap_2()
                  .items_center()
                  .child(
                    div()
                      .text_xs()
                      .font_weight(FontWeight::SEMIBOLD)
                      .text_color(cx.theme().muted_foreground)
                      .child("WORKSPACES"),
                  )
                  .when(count > 0, |h| h.child(treq_ui::Count::new(count, &palette)))
                  .child(div().flex_1())
                  .child(
                    Button::new("refresh")
                      .ghost()
                      .small()
                      .icon(IconName::RefreshCw)
                      .loading(self.list.is_loading())
                      .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                  ),
              )
              .child(body)
              .children(self.message.clone().map(|(ok, text)| {
                if ok {
                  widgets::muted(text, cx)
                } else {
                  widgets::error_text(text, cx)
                }
              })),
          ),
      )
      .child(self.bottom_bar(cx))
  }
}

/// Opens the sheet that renames `workspace`.
pub fn rename(repo: String, workspace: Workspace, done: OnDone, cx: &mut App) {
  sheet::open(Some("Rename workspace".into()), cx, move |window, cx| {
    cx.new(|cx| RenameSheet::new(repo, workspace, done, window, cx))
  });
}

/// Opens the sheet that creates workspaces, stacked on `stacked_on`'s
/// branch when given.
pub fn new_workspace(repo: String, stacked_on: Option<String>, done: OnDone, cx: &mut App) {
  super::new_workspaces::open(repo, stacked_on, done, cx)
}

/// Renaming a workspace's title.
struct RenameSheet {
  repo: String,
  workspace: Workspace,
  input: Entity<InputState>,
  done: OnDone,
  saving: bool,
  error: Option<String>,
}

impl RenameSheet {
  fn new(
    repo: String,
    workspace: Workspace,
    done: OnDone,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let title = title_of(&workspace);
    let input = cx.new(|cx| {
      let mut state = InputState::new(window, cx).placeholder("Title");
      state.set_value(title, window, cx);
      state
    });
    input.update(cx, |state, cx| state.focus(window, cx));
    Self {
      repo,
      workspace,
      input,
      done,
      saving: false,
      error: None,
    }
  }

  fn save(&mut self, cx: &mut Context<Self>) {
    let title = self.input.read(cx).value().trim().to_string();
    if title.is_empty() {
      self.error = Some("Enter a title.".into());
      return cx.notify();
    }
    self.saving = true;
    let request = TreqCommandRequest::UpdateWorkspace {
      repo: self.repo.clone(),
      workspace: self.workspace.id.to_string(),
      target_branch: None,
      title: Some(title),
      description: None,
    };
    let done = self.done.clone();
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      this.saving = false;
      match outcome {
        Ok(Mutation::Applied(_)) => {
          sheet::close(cx);
          done(cx);
        }
        other => this.error = Some(errors::describe(&mutation_message(&other, "").1).message),
      }
    });
    cx.notify();
  }
}

impl Render for RenameSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    v_flex()
      .px_4()
      .pt_1()
      .gap_3()
      .child(Input::new(&self.input).large())
      .children(self.error.clone().map(|e| widgets::error_text(e, cx)))
      .child(
        Button::new("rename-save")
          .primary()
          .large()
          .w_full()
          .label("Save")
          .loading(self.saving)
          .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
      )
  }
}

/// Jump to another repository on this machine, or another machine.
fn switcher(current: String, cx: &mut App) {
  sheet::open(None, cx, move |_, cx| cx.new(|_| Switcher { current }));
}

struct Switcher {
  current: String,
}

impl Render for Switcher {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let model = ui::model(cx).read(cx);
    let active = model.connection.as_ref().map(|c| c.machine().to_string());
    let repos = backend::get()
      .endpoint()
      .map(|e| store::saved_repositories(&e.id, store::endpoint_generation(&e)))
      .unwrap_or_default();
    let palette = treq_ui::theme::current(cx);
    let mut machines: Vec<(Machine, String)> = store::user_managed_endpoints()
      .into_iter()
      .map(|h| {
        let name = h.display_name.clone();
        (Machine::Host(h), name)
      })
      .collect();
    if model.user.is_some() {
      machines.push((Machine::Managed, "Cloud machine".into()));
    }
    machines.retain(|(m, _)| Some(m.id()) != active.as_deref());
    let statuses: Vec<MachineStatus> = machines
      .iter()
      .map(|(m, _)| model.machine(m.id()).status)
      .collect();
    let machine_name = ui::machine_name(cx).unwrap_or_default();

    v_flex()
      .child(widgets::muted(machine_name, cx).px_4().pt_2().pb_1())
      .children(repos.into_iter().enumerate().map(|(i, repo)| {
        let path = repo.canonical_remote_path.clone();
        let current = path == self.current;
        widgets::row(
          ("switch-repo", i),
          repo_name(&path).to_string(),
          Some(path.clone().into()),
          cx,
        )
        .when(current, |row| row.bg(palette.selected()))
        .on_click(move |_, _, cx| {
          sheet::close(cx);
          if !current {
            nav::home(cx);
            repos::open(cx);
            open(path.clone(), cx);
          }
        })
      }))
      .child(
        widgets::row("switch-open-repo", "Open repository…", None, cx).on_click(|_, _, cx| {
          sheet::close(cx);
          nav::home(cx);
          repos::open(cx);
        }),
      )
      .when(!machines.is_empty(), |list| {
        list
          .child(widgets::muted("Other machines", cx).px_4().pt_4().pb_1())
          .children(machines.into_iter().zip(statuses).enumerate().map(
            |(i, ((machine, name), status))| {
              let tone = match status {
                MachineStatus::Ready => Some(Tone::Success),
                MachineStatus::Failed(_) => Some(Tone::Danger),
                MachineStatus::CutOff(_) => Some(Tone::Warning),
                _ => None,
              };
              let leading = div()
                .w(px(14.))
                .flex()
                .justify_center()
                .children(tone.map(|t| Dot::new(t, &palette)))
                .into_any_element();
              widgets::row_full(("switch-machine", i), Some(leading), name, None, None, cx)
                .on_click(move |_, _, cx| {
                  sheet::close(cx);
                  ui::open_machine(machine.clone(), cx);
                })
            },
          ))
      })
      .child(
        widgets::row("switch-all-machines", "All machines", None, cx).on_click(|_, _, cx| {
          sheet::close(cx);
          ui::show_machines(cx);
        }),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn summary(id: i64, branch: &str, target: Option<&str>) -> WorkspaceSummary {
    WorkspaceSummary {
      workspace: Workspace {
        id,
        repo_path: "/r".into(),
        workspace_name: branch.replace('/', "-"),
        workspace_path: branch.replace('/', "-"),
        branch_name: branch.into(),
        created_at: String::new(),
        refreshed_at: None,
        metadata: None,
        target_branch: target.map(str::to_string),
        title: String::new(),
        description: None,
        moved_files: None,
        not_on_remote: false,
        sparse_patterns: None,
        archived: false,
      },
      has_conflicts: false,
      has_changes: false,
      commits_ahead: 0,
      terminals: 0,
      last_activity_at: None,
    }
  }

  #[core::prelude::v1::test]
  fn stacked_workspaces_follow_their_base_indented() {
    let list = vec![
      summary(1, "feat/a", Some("main")),
      summary(2, "feat/b-page", Some("feat/b-api")),
      summary(3, "feat/b-api", Some("main")),
      summary(4, "feat/b-tests", Some("feat/b-page")),
    ];
    let order: Vec<(usize, usize)> = stack_order(&list)
      .into_iter()
      .map(|e| (e.index, e.depth))
      .collect();
    assert_eq!(order, vec![(0, 0), (2, 0), (1, 1), (3, 2)]);
  }

  #[core::prelude::v1::test]
  fn a_target_cycle_still_lists_every_workspace() {
    let list = vec![
      summary(1, "a", Some("b")),
      summary(2, "b", Some("a")),
      summary(3, "c", Some("c")),
    ];
    let mut indexes: Vec<usize> = stack_order(&list).into_iter().map(|e| e.index).collect();
    indexes.sort();
    assert_eq!(indexes, vec![0, 1, 2]);
  }

  #[core::prelude::v1::test]
  fn search_matches_title_or_branch() {
    let mut s = summary(1, "fix/slack-retry", None);
    s.workspace.title = "Retry flaky Slack webhooks".into();
    assert!(matches(&s, "flaky"));
    assert!(matches(&s, "SLACK-RETRY"));
    assert!(matches(&s, "  "));
    assert!(!matches(&s, "discord"));
  }
}
