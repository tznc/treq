//! A repository's workspaces, as the desktop sidebar shows them: stacked
//! workspaces under their base and status dots. A tap opens a workspace;
//! holding one starts selecting, and the title bar's ⋯ acts on the picks.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    spinner::Spinner,
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
  ui::{self, hold::hold, load, nav, sheet, widgets, Load, Location, Machine, MachineStatus},
};

/// What a workspace sheet calls after its change applied.
pub type OnDone = std::rc::Rc<dyn Fn(&mut App)>;

/// Indent per stacking level.
const INDENT: f32 = 16.;

/// How often the list checks the host for changes made elsewhere (the
/// desktop app, the CLI, an agent).
const MARKER_POLL: Duration = Duration::from_secs(10);

#[derive(serde::Deserialize)]
struct ChangeMarker {
  operation_id: Option<String>,
}

pub fn open(repo: String, cx: &mut App) {
  let title = repo_name(&repo).to_string();
  let location = Location {
    repo: Some(repo.clone()),
    workspace: None,
  };
  let switcher_repo = repo.clone();
  // The header's ⋯ reaches the screen once it exists.
  let screen: Rc<RefCell<Option<WeakEntity<WorkspacesScreen>>>> = Default::default();
  let menu_screen = screen.clone();
  nav::push_with(
    title,
    nav::Header {
      on_title: Some(Rc::new(move |_, cx| switcher(switcher_repo.clone(), cx))),
      more: Some(Rc::new(move |_, cx| {
        if let Some(screen) = menu_screen.borrow().as_ref() {
          screen.update(cx, |screen, cx| screen.menu(cx)).ok();
        }
      })),
    },
    location,
    cx,
    move |window, cx| {
      let entity = cx.new(|cx| WorkspacesScreen::new(repo, window, cx));
      *screen.borrow_mut() = Some(entity.downgrade());
      entity
    },
  );
}

/// A repository's folder name, from its path on the host.
pub fn repo_name(path: &str) -> &str {
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
  /// Workspaces an archive or delete is running on. Their rows show a
  /// spinner and take no taps until it ends.
  pending: HashSet<i64>,
  message: Option<(bool, String)>,
  /// Why the last refresh failed, while the list shows what it had.
  refresh_error: Option<String>,
  /// The host's last change marker, to refetch when something else changed
  /// the repository.
  marker: Option<String>,
  /// The rows on screen, in display order, as the last render left them.
  entries: Vec<Entry>,
  rows: ListState,
  _poll: Task<()>,
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
    let poll = cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(MARKER_POLL).await;
      let polled = this.update(cx, |this, cx| {
        // Nothing shows the result under another page or in the
        // background. The next tick once shown catches up, and a return
        // to the foreground refetches at once.
        if crate::backend::in_foreground() && nav::is_top(cx.entity_id(), cx) {
          this.poll_marker(cx);
        }
      });
      if polled.is_err() {
        break;
      }
    });
    let mut this = Self {
      repo,
      list,
      query,
      selection: None,
      pending: HashSet::new(),
      message: None,
      refresh_error: None,
      marker: None,
      entries: Vec::new(),
      rows: widgets::list_state(),
      _poll: poll,
    };
    this.poll_marker(cx);
    this.fetch(true, cx);
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  /// A screen showing `list`, without the host reads (measurement tests).
  #[cfg(test)]
  pub(crate) fn for_test(
    list: Vec<WorkspaceSummary>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search workspaces"));
    Self {
      repo: "/r".into(),
      list: Load::Ready(list),
      query,
      selection: None,
      pending: HashSet::new(),
      message: None,
      refresh_error: None,
      marker: None,
      entries: Vec::new(),
      rows: widgets::list_state(),
      _poll: Task::ready(()),
    }
  }

  /// The list's viewport and the bounds of its item `ix`, once drawn.
  #[cfg(test)]
  pub(crate) fn drawn(&self, ix: usize) -> (Bounds<Pixels>, Option<Bounds<Pixels>>) {
    (self.rows.viewport_bounds(), self.rows.bounds_for_item(ix))
  }

  #[cfg(test)]
  pub(crate) fn set_selecting(
    &mut self,
    ids: impl IntoIterator<Item = i64>,
    cx: &mut Context<Self>,
  ) {
    self.selection = Some(ids.into_iter().collect());
    cx.notify();
  }

  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.list.ready().is_none() {
      self.list = Load::Loading;
    }
    load::run(
      cx,
      load_summaries(self.repo.clone(), join),
      |this, result, _| {
        match result {
          Ok(list) => {
            // Drop picks the host no longer has.
            if let Some(selection) = &mut this.selection {
              selection.retain(|id| list.iter().any(|s| s.workspace.id == *id));
            }
            this.list = Load::Ready(list);
            this.refresh_error = None;
          }
          // Keep what's on screen; a refresh that fails says so below it.
          Err(error) if this.list.ready().is_some() => {
            this.refresh_error = Some(format!(
              "Couldn't refresh: {}",
              errors::describe(&error).message
            ));
          }
          Err(error) => this.list = Load::Failed(error),
        }
      },
    );
  }

  /// Refetches when the repository changed on the host since the last poll.
  fn poll_marker(&mut self, cx: &mut Context<Self>) {
    let repo = self.repo.clone();
    load::run(
      cx,
      remote::read::<ChangeMarker>(TreqCommandRequest::WorkspaceChangeMarker {
        repo,
        workspace: None,
      }),
      |this, result, cx| {
        if let Ok(marker) = result {
          let changed = this.marker.is_some() && this.marker != marker.operation_id;
          this.marker = marker.operation_id;
          if changed && this.pending.is_empty() {
            this.reload(cx);
          }
        }
      },
    );
  }

  /// The picked workspaces the list shows: on the host and matching the
  /// search, so a bulk action never reaches rows the user can't see.
  fn visible_selection(&self, cx: &App) -> Vec<i64> {
    let (Some(selection), Some(list)) = (&self.selection, self.list.ready()) else {
      return Vec::new();
    };
    let query = self.query.read(cx).value().to_string();
    list
      .iter()
      .filter(|s| selection.contains(&s.workspace.id) && matches(s, &query))
      .map(|s| s.workspace.id)
      .collect()
  }

  fn summary(&self, id: i64) -> Option<&WorkspaceSummary> {
    self.list.ready()?.iter().find(|s| s.workspace.id == id)
  }

  /// The title bar's ⋯: actions for the picked workspaces while selecting,
  /// otherwise a way to start selecting.
  fn menu(&mut self, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    let ids = self.visible_selection(cx);
    if self.selection.is_none() {
      let entries = vec![
        sheet::MenuEntry::new("menu-select", "Select workspaces", {
          let this = this.clone();
          move |_, cx| {
            this.update(cx, |screen, cx| screen.select(None, cx)).ok();
          }
        }),
        sheet::MenuEntry::new("menu-github", "GitHub", {
          let repo = self.repo.clone();
          move |_, cx| super::github::open(repo.clone(), cx)
        }),
        sheet::MenuEntry::new("menu-refresh", "Refresh", move |_, cx| {
          this.update(cx, |screen, cx| screen.reload(cx)).ok();
        }),
      ];
      return sheet::menu(repo_name(&self.repo).to_string(), entries, cx);
    }
    let mut entries = Vec::new();
    if let [id] = ids[..] {
      let Some(workspace) = self.summary(id).map(|s| s.workspace.clone()) else {
        return;
      };
      let repo = self.repo.clone();
      let (open_ws, stack_ws, rename_ws, copy_ws) = (
        workspace.clone(),
        workspace.branch_name.clone(),
        workspace.clone(),
        workspace.branch_name.clone(),
      );
      entries.extend([
        sheet::MenuEntry::new("menu-open", "Open", {
          let this = this.clone();
          move |_, cx| {
            this.update(cx, |screen, cx| screen.end_selection(cx)).ok();
            workspace::open(repo.clone(), open_ws.id.to_string(), title_of(&open_ws), cx)
          }
        }),
        sheet::MenuEntry::new("menu-stack", "New workspace stacked on this", {
          let this = this.clone();
          move |_, cx| {
            let branch = stack_ws.clone();
            this
              .update(cx, |screen, cx| {
                screen.end_selection(cx);
                screen.new_workspace(Some(branch), cx)
              })
              .ok();
          }
        }),
        sheet::MenuEntry::new("menu-rename", "Rename…", {
          let this = this.clone();
          move |_, cx| {
            let workspace = rename_ws.clone();
            this
              .update(cx, |screen, cx| {
                screen.end_selection(cx);
                screen.rename(workspace, cx)
              })
              .ok();
          }
        }),
        sheet::MenuEntry::new("menu-copy", "Copy branch name", move |_, cx| {
          cx.write_to_clipboard(ClipboardItem::new_string(copy_ws.clone()))
        }),
      ]);
    }
    if !ids.is_empty() {
      let archive_ids = ids.clone();
      entries.push(sheet::MenuEntry::new(
        "menu-archive",
        match ids.len() {
          1 => "Archive".to_string(),
          n => format!("Archive {n} workspaces"),
        },
        {
          let this = this.clone();
          move |_, cx| {
            this
              .update(cx, |screen, cx| {
                screen.confirm_archive(archive_ids.clone(), cx)
              })
              .ok();
          }
        },
      ));
    }
    if let [id] = ids[..] {
      entries.push(
        sheet::MenuEntry::new("menu-delete", "Delete…", {
          let this = this.clone();
          move |_, cx| {
            this
              .update(cx, |screen, cx| screen.confirm_delete(id, cx))
              .ok();
          }
        })
        .danger(),
      );
    }
    entries.push(sheet::MenuEntry::new("menu-select-all", "Select all", {
      let this = this.clone();
      move |_, cx| {
        this.update(cx, |screen, cx| screen.select_all(cx)).ok();
      }
    }));
    entries.push(sheet::MenuEntry::new(
      "menu-select-done",
      "Cancel selection",
      move |_, cx| {
        this.update(cx, |screen, cx| screen.end_selection(cx)).ok();
      },
    ));
    let title = match ids.len() {
      0 => "Nothing selected".to_string(),
      n => format!("{n} selected"),
    };
    sheet::menu(title, entries, cx);
  }

  /// Starts selecting, with `id` picked when given. Picking again keeps it.
  fn select(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
    if id.is_some_and(|id| self.pending.contains(&id)) {
      return;
    }
    let selection = self.selection.get_or_insert_with(HashSet::new);
    selection.extend(id);
    cx.notify();
  }

  /// Picks every workspace the list shows.
  fn select_all(&mut self, cx: &mut Context<Self>) {
    let query = self.query.read(cx).value().to_string();
    let ids: Vec<i64> = self
      .list
      .ready()
      .into_iter()
      .flatten()
      .filter(|s| matches(s, &query) && !self.pending.contains(&s.workspace.id))
      .map(|s| s.workspace.id)
      .collect();
    self.selection.get_or_insert_with(HashSet::new).extend(ids);
    cx.notify();
  }

  fn end_selection(&mut self, cx: &mut Context<Self>) {
    self.selection = None;
    cx.notify();
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
    let ids: Vec<i64> = ids
      .into_iter()
      .filter(|id| !self.pending.contains(id))
      .collect();
    if ids.is_empty() {
      return;
    }
    self.pending.extend(&ids);
    self.message = None;
    let repo = self.repo.clone();
    let count = ids.len();
    let title = (count == 1)
      .then(|| self.summary(ids[0]).map(|s| title_of(&s.workspace)))
      .flatten();
    let done_ids = ids.clone();
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
        for id in &done_ids {
          this.pending.remove(id);
        }
        this.selection = None;
        this.message = Some(match (failed.first(), title) {
          (None, Some(title)) => (true, format!("{title} archived.")),
          (None, None) => (true, format!("{count} workspaces archived.")),
          (Some(error), Some(title)) => (
            false,
            format!(
              "Couldn't archive {title}: {}",
              errors::describe(error).message
            ),
          ),
          (Some(error), None) => (
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
    if !self.pending.insert(id) {
      return;
    }
    self.message = None;
    let request = TreqCommandRequest::DeleteWorkspace {
      repo: self.repo.clone(),
      workspace: id.to_string(),
    };
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      this.pending.remove(&id);
      let (ok, text) = mutation_message(&outcome, "Workspace deleted.");
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

  /// Picks or unpicks `id`. Unpicking the last one stops selecting.
  fn toggle(&mut self, id: i64, cx: &mut Context<Self>) {
    if let Some(selection) = &mut self.selection {
      if !selection.remove(&id) {
        selection.insert(id);
      }
      if selection.is_empty() {
        self.selection = None;
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
    let pending = self.pending.contains(&id);

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

    let trailing = h_flex()
      .gap_2()
      .items_center()
      .children(status)
      .when(pending, |t| t.child(Spinner::new().small()))
      .into_any_element();
    let leading = selecting.map(|_| {
      Checkbox::new(SharedString::from(format!("ws-check-{id}")), &palette)
        .checked(selected)
        .into_any_element()
    });
    let (repo, name) = (self.repo.clone(), title_of(workspace));
    let screen = cx.weak_entity();
    let row = widgets::row_full(
      SharedString::from(format!("ws-{id}")),
      leading,
      title_of(workspace),
      Some(workspace.branch_name.clone().into()),
      Some(trailing),
      cx,
    )
    .pl(px(16. + INDENT * entry.depth as f32))
    .when(selected, |row| row.bg(palette.selected()))
    .when(pending, |row| row.opacity(0.6))
    .on_click(cx.listener(move |this, _, _, cx| {
      if this.pending.contains(&id) {
        return;
      }
      if this.selection.is_some() {
        this.toggle(id, cx);
      } else {
        workspace::open(repo.clone(), id.to_string(), name.clone(), cx);
      }
    }));
    hold(("ws-hold", id as u64), row, move |_, cx| {
      screen
        .update(cx, |screen, cx| screen.select(Some(id), cx))
        .ok();
    })
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
      Some(_) => {
        let count = self.visible_selection(cx).len();
        bar
          .items_center()
          .child(
            div()
              .flex_1()
              .pl_1()
              .font_weight(FontWeight::SEMIBOLD)
              .child(match count {
                0 => "Tap workspaces to select them".to_string(),
                n => format!("{n} selected"),
              }),
          )
          .child(
            Button::new("select-done")
              .outline()
              .large()
              .label("Done")
              .on_click(cx.listener(|this, _, _, cx| this.end_selection(cx))),
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

impl WorkspacesScreen {
  /// The card standing in for the rows: loading, failed, empty or no match.
  fn placeholder(&self, cx: &mut Context<Self>) -> Div {
    match &self.list {
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
      Load::Ready(_) => widgets::card(cx).child(widgets::centered_message("No workspaces match.", cx)),
    }
  }

  /// The search box and list title above the rows.
  fn head(&self, cx: &mut Context<Self>) -> AnyElement {
    let count = self.list.ready().map_or(0, Vec::len);
    let palette = treq_ui::theme::current(cx);
    v_flex()
      .gap_3()
      .px_4()
      .pt_4()
      .pb_3()
      .when(count > 0, |head| {
        head.child(Input::new(&self.query).large().cleanable(true))
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
      .children(
        self
          .refresh_error
          .clone()
          .map(|error| widgets::error_text(error, cx)),
      )
      .into_any_element()
  }

  /// The line under the rows and the page's bottom padding.
  fn tail(&self, cx: &mut Context<Self>) -> AnyElement {
    div()
      .px_4()
      .pb_4()
      .children(self.message.clone().map(|(ok, text)| {
        let line = if ok {
          widgets::muted(text, cx)
        } else {
          widgets::error_text(text, cx)
        };
        line.pt_3()
      }))
      .into_any_element()
  }

  /// Item `ix` of the list: the head, the rows (or their placeholder), the tail.
  fn item(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
    // The list is borrowed while it builds items, so its count is not asked.
    let last = 1 + self.entries.len().max(1);
    if ix == 0 {
      return self.head(cx);
    }
    if ix >= last {
      return self.tail(cx);
    }
    let (Load::Ready(list), Some(entry)) = (&self.list, self.entries.get(ix - 1)) else {
      return div().px_4().child(self.placeholder(cx)).into_any_element();
    };
    let (first, end) = (ix == 1, ix + 1 == last);
    widgets::card_item(self.row(entry, list, cx), first, end, cx).into_any_element()
  }
}

impl Render for WorkspacesScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let query = self.query.read(cx).value().to_string();
    self.entries = match &self.list {
      Load::Ready(list) => stack_order(list)
        .into_iter()
        .filter(|e| matches(&list[e.index], &query))
        .collect(),
      _ => Vec::new(),
    };
    // Head, then the rows (one placeholder card without any), then the tail.
    widgets::resize(&self.rows, 2 + self.entries.len().max(1));
    let screen = cx.weak_entity();
    v_flex()
      .size_full()
      .child(
        div().flex_1().min_h_0().child(
          list(self.rows.clone(), move |ix, _, cx| {
            screen
              .update(cx, |this, cx| this.item(ix, cx))
              .unwrap_or_else(|_| div().into_any_element())
          })
          .size_full(),
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
