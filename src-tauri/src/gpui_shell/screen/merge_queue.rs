//! The GitHub panel on its Merge Queue tab (`GitHubPanel` with
//! `MergeQueueTab`): queued branches grouped into stacks above the target
//! branch they land on, with merged history below it.
//!
//! The queue lives in Supabase, which Rust reaches ([`merge_queue_data`]):
//! the tab reads the opt-in and the entries every 30 s, removes branches,
//! and takes the plan from the account ([`account_data`]). React sends the
//! repository's GitHub name and the history page it shows. The tab also
//! reads each queued workspace's lines changed, the default branch and the
//! target branch's tip.
//!
//! [`account_data`]: super::account_data
//!
//! [`MergeQueueView::tab`] draws only the tab, for a GitHub panel drawn in
//! GPUI to host; [`MergeQueueView`]'s own render adds the panel's header
//! and tabs around it.
//!
//! [`merge_queue_data`]: super::merge_queue_data

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use serde::Deserialize;
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, AnyElement, App, ClipboardItem, Context, Div, FontWeight, SharedString,
  Task, Window,
};

use super::{
  account_data,
  code_data::diff_stats,
  merge_queue_data::{
    bar_widths, history_page, max_change, partition, status_label, tip_title, with_local_stats,
    MergeQueueHost, QueueEntry, QueueStack, HISTORY_PAGE, HISTORY_SLACK,
  },
  ActionSink, ScreenAction,
};
use crate::gpui_shell::{
  native,
  popup::{PopupSink, Side, TooltipContent},
  theme::{self, Palette},
  tooltips::{HasTooltips, Tooltips},
  ui::{self, Button, Size, TextSize, Tone, Variant},
};

/// What React sends: the panel's remote and the history it shows.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  /// The GitHub remote's `owner/repo`, once read.
  pub full_name: Option<String>,
  pub remote_loading: bool,
  pub show_merged_history: bool,
  pub history_limit: usize,
}

/// `useQueueEntries` refreshes this often.
const QUEUE_POLL: Duration = Duration::from_secs(30);

/// The queue, as read from Supabase.
#[derive(Debug, Clone, Default)]
struct Queue {
  /// The `owner/repo` it was read for.
  full_name: Option<String>,
  /// `get_merge_queue_enabled`, `None` until read.
  enabled: Option<bool>,
  loading: bool,
  entries: Vec<QueueEntry>,
  /// A Remove is running.
  dequeuing: bool,
}

/// The queue's stacks, split once per change of the entries or the stats
/// instead of cloned and rebuilt on every render.
struct Stacks {
  active: Vec<QueueStack>,
  history: Vec<QueueStack>,
}

/// What the tab read from the backend.
#[derive(Debug, Clone, Default)]
struct Local {
  default_branch: Option<String>,
  /// The home repository's newest commit: short id and full id.
  tip: Option<(String, String)>,
  /// Lines changed by each queued branch's workspace.
  stats: HashMap<String, (u32, u32)>,
}

/// `STACK_DOCS_URL`.
fn stack_docs() -> String {
  format!(
    "{}/docs/concepts/workspaces#stacks-and-rebasing",
    native::web_url()
  )
}

pub struct MergeQueueView {
  host: Arc<dyn MergeQueueHost>,
  react: ActionSink,
  route: Route,
  dark: bool,
  tips: Tooltips,
  local: Local,
  queue: Queue,
  /// `queue` and `local` as stacks; `None` until the next render needs them.
  stacks: Option<Rc<Stacks>>,
  /// The account's plan carries Pro.
  pro: bool,
  poll: Option<Task<()>>,
  /// The branches the stats were read for.
  read_for: Option<(String, Vec<String>)>,
  pub(super) copied: bool,
  copied_reset: Option<Task<()>>,
}

impl HasTooltips for MergeQueueView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

impl MergeQueueView {
  pub fn new(
    route: Route,
    host: Arc<dyn MergeQueueHost>,
    react: ActionSink,
    dark: bool,
    popups: PopupSink,
    cx: &mut Context<Self>,
  ) -> Self {
    let mut this = Self {
      host,
      react,
      route,
      dark,
      tips: Tooltips::new(popups, dark),
      local: Local::default(),
      queue: Queue::default(),
      stacks: None,
      pro: account_data::account(cx).is_pro(),
      poll: None,
      read_for: None,
      copied: false,
      copied_reset: None,
    };
    cx.observe_global::<account_data::Account>(|view, cx| {
      let pro = account_data::account(cx).is_pro();
      if view.pro != pro {
        view.pro = pro;
        view.read_queue(cx);
        cx.notify();
      }
    })
    .detach();
    this.read_queue(cx);
    this.load(cx);
    this
  }

  /// Reads the opt-in, then the entries, now and every 30 s, for a Pro
  /// account once the remote is known.
  fn read_queue(&mut self, cx: &mut Context<Self>) {
    let full_name = self.route.full_name.clone().filter(|_| self.pro);
    if self.queue.full_name != full_name {
      self.queue = Queue {
        full_name: full_name.clone(),
        loading: full_name.is_some(),
        ..Queue::default()
      };
      self.stacks = None;
    }
    let Some(full_name) = full_name else {
      self.poll = None;
      return;
    };
    let host = self.host.clone();
    self.poll = Some(cx.spawn(async move |this, cx| loop {
      let name = full_name.clone();
      let host = host.clone();
      let Ok(history) = this.update(cx, |view, _| view.history_fetch()) else {
        return;
      };
      let read = cx
        .background_spawn(async move { host.merge_queue(&name, history) })
        .await;
      let kept = this.update(cx, |view, cx| {
        if view.queue.full_name.as_deref() != Some(full_name.as_str()) {
          return;
        }
        let was_loading = std::mem::replace(&mut view.queue.loading, false);
        let mut changed = was_loading;
        match read {
          Ok((enabled, entries)) => {
            changed |= view.queue.enabled != Some(enabled);
            view.queue.enabled = Some(enabled);
            if view.queue.entries != entries {
              view.queue.entries = entries;
              view.stacks = None;
              changed = true;
            }
          }
          Err(error) => log::warn!("reading the merge queue failed: {error}"),
        }
        // An unchanged queue neither rereads the workspaces nor repaints.
        if changed {
          view.load(cx);
          cx.notify();
        }
      });
      if kept.is_err() {
        return;
      }
      cx.background_executor().timer(QUEUE_POLL).await;
    }));
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      self.tips.dark = dark;
    }
    if self.route != route {
      if self.route.repo_path != route.repo_path {
        self.local = Local::default();
        self.stacks = None;
      }
      let reread = self.route.full_name != route.full_name
        || self.history_fetch_for(&self.route) != self.history_fetch_for(&route);
      self.route = route;
      if reread {
        self.read_queue(cx);
      }
      self.load(cx);
    }
    cx.notify();
  }

  /// Merged entries to read: the shown history, plus [`HISTORY_SLACK`].
  fn history_fetch_for(&self, route: &Route) -> usize {
    let shown = if route.show_merged_history {
      route.history_limit
    } else {
      0
    };
    shown + HISTORY_SLACK
  }

  fn history_fetch(&self) -> usize {
    self.history_fetch_for(&self.route)
  }

  fn branches(&self) -> Vec<String> {
    self
      .queue
      .entries
      .iter()
      .map(|e| e.branch_name.clone())
      .collect()
  }

  /// Reads the default branch, the tip and the queued workspaces' stats,
  /// once per repository and set of queued branches.
  fn load(&mut self, cx: &mut Context<Self>) {
    let key = (self.route.repo_path.clone(), self.branches());
    if self.read_for.as_ref() == Some(&key) {
      return;
    }
    self.read_for = Some(key.clone());
    let host = self.host.clone();
    let entries = self.queue.entries.clone();
    let repo = self.route.repo_path.clone();
    cx.spawn(async move |this, cx| {
      let local = cx
        .background_spawn(async move {
          let default_branch = host.default_branch(&repo).ok();
          let tip = host
            .commits(&repo, None, false, Some(1))
            .ok()
            .and_then(|log| log.commits.into_iter().next())
            .map(|c| (c.short_id, c.commit_id));
          let mut stats = HashMap::new();
          if !entries.is_empty() {
            let workspaces = host.workspaces(&repo).unwrap_or_default();
            for entry in &entries {
              if entry.insertions.is_some() || entry.deletions.is_some() {
                continue;
              }
              let Some(ws) = workspaces
                .iter()
                .find(|w| w.branch_name == entry.branch_name)
              else {
                continue;
              };
              if let Ok(log) = host.commits(&repo, Some(ws.id), false, None) {
                stats.insert(entry.branch_name.clone(), diff_stats(&log.commits));
              }
            }
          }
          Local {
            default_branch,
            tip,
            stats,
          }
        })
        .await;
      this
        .update(cx, |view, cx| {
          if view.read_for.as_ref() == Some(&key) {
            view.local = local;
            view.stacks = None;
            cx.notify();
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  fn send(&self, action: ScreenAction, cx: &mut App) {
    let react = self.react.clone();
    cx.defer(move |cx| react(action, cx));
  }

  fn press(&self, id: &str, cx: &mut App) {
    self.send(ScreenAction::Press { id: id.into() }, cx);
  }

  fn field(&self, field: &str, value: serde_json::Value, cx: &mut App) {
    self.send(
      ScreenAction::Field {
        field: field.into(),
        value,
      },
      cx,
    );
  }

  /// `useDequeueBranches`: removes the branches, then rereads the queue.
  fn dequeue(&mut self, branches: Vec<String>, cx: &mut Context<Self>) {
    let Some(full_name) = self.queue.full_name.clone() else {
      return;
    };
    if self.queue.dequeuing {
      return;
    }
    self.queue.dequeuing = true;
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move { host.dequeue(&full_name, &branches) })
        .await;
      this
        .update(cx, |view, cx| {
          view.queue.dequeuing = false;
          if let Err(error) = result {
            log::warn!("removing from the merge queue failed: {error}");
          }
          view.read_queue(cx);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
    cx.notify();
  }

  fn copy_tip(&mut self, cx: &mut Context<Self>) {
    let Some((short, full)) = self.local.tip.clone() else {
      return;
    };
    let value = if full.is_empty() { short } else { full };
    cx.write_to_clipboard(ClipboardItem::new_string(value));
    self.copied = true;
    self.copied_reset = Some(cx.spawn(async move |this, cx| {
      cx.background_executor()
        .timer(Duration::from_millis(1500))
        .await;
      this
        .update(cx, |view, cx| {
          view.copied = false;
          cx.notify();
        })
        .ignore_closed();
    }));
    cx.notify();
  }
}

impl Render for MergeQueueView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    let p = theme::palette(self.dark);
    let panel = self.panel(&p, cx);
    div()
      .debug_selector(|| "github-panel".into())
      .size_full()
      .flex()
      .bg(p.background)
      .text_color(p.foreground)
      .child(panel)
  }
}

impl MergeQueueView {
  /// `GitHubPanel`'s list column on the Merge Queue tab.
  fn panel(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let route = &self.route;
    let heading = if route.remote_loading {
      ui::text(div(), TextSize::Sm)
        .text_color(p.muted_foreground)
        .child("Loading…")
    } else if let Some(name) = &route.full_name {
      ui::text(div(), TextSize::Sm)
        .debug_selector(|| "github-repo-name".into())
        .font_family(theme::MONO_FONT)
        .child(name.clone())
    } else {
      ui::text(div(), TextSize::Sm)
        .text_color(p.destructive)
        .child("No GitHub remote")
    };
    let header = div()
      .flex()
      .flex_none()
      .items_center()
      .px_4()
      .pt_4()
      .pb_2()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(ui::icon(theme::ICON_GITHUB, 20., p.muted_foreground))
          .child(heading.font_weight(FontWeight::SEMIBOLD)),
      );
    let tab = |id: &'static str, label: &'static str, active: bool| {
      let bg = p.background;
      ui::text(div(), TextSize::Sm)
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .items_center()
        .gap_1p5()
        .px_3()
        .py_1()
        .rounded(radius::MD)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(active, |this| {
          this.bg(p.background).text_color(p.foreground).shadow_sm()
        })
        .when(!active, |this| {
          let fg = p.foreground;
          this
            .text_color(p.muted_foreground)
            .hover(move |s| s.bg(bg.opacity(0.5)).text_color(fg))
        })
        .child(label)
    };
    let pro = (!self.pro).then(|| ui::Badge::new("PRO", p));
    let tabs = div().flex_none().px_4().pb_2().child(
      div()
        .flex()
        .gap_1()
        .p_1()
        .rounded(radius::MD)
        .bg(p.muted)
        .child(
          tab("github-tab-issues", "Issues", false)
            .on_click(cx.listener(|view, _, _, cx| view.press("github.tab:issues", cx))),
        )
        .child(
          tab("github-tab-prs", "Pull Requests", false)
            .on_click(cx.listener(|view, _, _, cx| view.press("github.tab:prs", cx))),
        )
        .child(tab("github-tab-merge-queue", "Merge Queue", true).children(pro)),
    );
    let toolbar = (self.pro && self.queue.enabled == Some(true) && route.full_name.is_some())
      .then(|| self.toolbar(p, cx));
    let body = self.tab(p, cx);
    div()
      .flex_1()
      .flex()
      .flex_col()
      .min_w_0()
      .overflow_hidden()
      .child(header)
      .child(div().flex().child(tabs).child(div().flex_1()))
      .children(toolbar)
      .child(
        div()
          .id("merge-queue-body")
          .flex_1()
          .min_h_0()
          .flex()
          .flex_col()
          .children(body),
      )
  }

  /// The Merged toggle and Refresh (`merge-queue-toolbar`).
  fn toolbar(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let on = self.route.show_merged_history;
    let fg = p.foreground;
    let merged = ui::text(div(), TextSize::Sm)
      .id("merge-queue-merged")
      .debug_selector(|| "merge-queue-merged".into())
      .flex()
      .items_center()
      .gap_1p5()
      .h_7()
      .px_2()
      .rounded(radius::MD)
      .cursor_pointer()
      .text_color(if on { p.foreground } else { p.muted_foreground })
      .hover(move |s| s.text_color(fg))
      .child(ui::Checkbox::new("merge-queue-merged-check", p).checked(on))
      .child("Merged")
      .on_click(cx.listener(move |view, _, _, cx| {
        view.field("mergeQueue.showMerged", json!(!on), cx);
      }));
    let refresh = Button::new("merge-queue-refresh", p)
      .aria_label("Refresh")
      .variant(Variant::Ghost)
      .size(Size::Icon)
      .w(zpx(28.))
      .h(zpx(28.))
      .icon(theme::ICON_REFRESH, 14.)
      .on_click(cx.listener(|view, _, _, cx| view.read_queue(cx)));
    div()
      .debug_selector(|| "merge-queue-toolbar".into())
      .flex()
      .flex_none()
      .items_center()
      .gap_2()
      .px_4()
      .pb_2()
      .child(merged)
      .child(div().flex_1())
      .child(self.tips.wrap(
        "merge-queue-refresh-tip",
        Side::Bottom,
        Some(TooltipContent::Text("Refresh".into())),
        refresh,
        cx,
      ))
  }

  /// `MergeQueueTab`: the upsell, the opt-in, or the queue.
  pub fn tab(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
    if !self.pro {
      return Some(self.upsell(p).into_any_element());
    }
    self.route.full_name.as_ref()?;
    // Until the opt-in is read, as `useMergeQueueEnabled` loads.
    if self.queue.enabled.is_none() {
      return Some(
        div()
          .flex()
          .items_center()
          .justify_center()
          .py_12()
          .child(ui::spinner("merge-queue-loading", 20., p.muted_foreground))
          .into_any_element(),
      );
    }
    if self.queue.enabled != Some(true) {
      return Some(self.disabled(p, cx).into_any_element());
    }
    Some(self.list(p, cx).into_any_element())
  }

  /// `MergeQueueUpsell`.
  fn upsell(&self, p: &Palette) -> impl IntoElement {
    let green = Tone::Success.text(p);
    let host = self.host.clone();
    div()
      .debug_selector(|| "merge-queue-upsell".into())
      .size_full()
      .flex()
      .items_center()
      .justify_center()
      .p_6()
      .child(
        div()
          .max_w(zpx(448.))
          .w_full()
          .rounded(radius::LG)
          .border_1()
          .border_color(p.border)
          .bg(gpui::linear_gradient(
            135.,
            gpui::linear_color_stop(p.muted.opacity(0.4), 0.),
            gpui::linear_color_stop(Tone::Success.color(p).opacity(0.05), 1.),
          ))
          .p_8()
          .flex()
          .flex_col()
          .items_center()
          .gap_4()
          .child(
            div()
              .size_12()
              .flex()
              .items_center()
              .justify_center()
              .rounded_full()
              .bg(Tone::Success.tint(p))
              .child(ui::icon(theme::ICON_ROCKET, 24., green)),
          )
          .child(
            div()
              .flex()
              .flex_col()
              .items_center()
              .gap_2()
              .child(ui::Badge::new("PRO", p))
              .child(
                ui::text(div(), TextSize::Lg)
                  .font_weight(FontWeight::SEMIBOLD)
                  .child("Unlock Merge Queue"),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_center()
                  .text_color(p.muted_foreground)
                  .child(
                    "Queue stacked PRs, run CI in parallel lanes, and merge with confidence. \
                     Upgrade to Pro to manage your repository's merge queue from Treq.",
                  ),
              ),
          )
          .child(
            Button::new("merge-queue-upgrade", p)
              .size(Size::Lg)
              .w_full()
              .gap_2()
              .bg(Tone::Success.color(p))
              .hover_bg(Tone::Success.color(p).opacity(0.85))
              .text_color(p.primary_foreground)
              .child(ui::icon(theme::ICON_ROCKET, 16., p.primary_foreground))
              .label("Upgrade to Pro")
              .on_click(move |_, _, _| host.open_url(&format!("{}/dashboard", native::web_url()))),
          ),
      )
  }

  /// `MergeQueueDisabled`.
  fn disabled(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .debug_selector(|| "merge-queue-disabled".into())
      .size_full()
      .flex()
      .flex_col()
      .items_center()
      .justify_center()
      .p_8()
      .gap_3()
      .child(ui::icon(theme::ICON_MERGE, 32., p.muted_foreground))
      .child(
        ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child("The merge queue is off for this repository."),
      )
      .child(
        Button::new("merge-queue-enable", p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .label("Enable it in Settings › Integrations")
          .on_click(cx.listener(|view, _, _, cx| view.press("mergeQueue.openSettings", cx))),
      )
  }

  /// `MergeQueueList`: the stacks, the target branch, then merged history.
  fn list(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
    if self.queue.loading {
      return div()
        .flex()
        .items_center()
        .justify_center()
        .py_12()
        .child(ui::spinner("merge-queue-loading", 20., p.muted_foreground));
    }
    let stacks = match &self.stacks {
      Some(stacks) => stacks.clone(),
      None => {
        let entries = with_local_stats(&self.queue.entries, &self.local.stats);
        let (active, history) = partition(&entries);
        let stacks = Rc::new(Stacks { active, history });
        self.stacks = Some(stacks.clone());
        stacks
      }
    };
    let (active, history) = (&stacks.active, &stacks.history);
    let limit = if self.route.show_merged_history {
      self.route.history_limit
    } else {
      0
    };
    let (visible, more) = history_page(history, limit);
    let target = active
      .first()
      .map(|s| s.target_branch.clone())
      .or_else(|| history.first().map(|s| s.target_branch.clone()))
      .or_else(|| self.local.default_branch.clone())
      .unwrap_or_else(|| "main".into());
    let tip_entry = history.first().and_then(|s| s.entries.first());
    let mut column = div().relative().flex().flex_col().gap_3();
    if active.is_empty() {
      column = column.child(
        div().py_8().child(
          div()
            .debug_selector(|| "merge-queue-empty".into())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py_12()
            .gap_3()
            .text_color(p.muted_foreground)
            .child(ui::icon(theme::ICON_MERGE, 32., p.muted_foreground))
            .child(ui::text(div(), TextSize::Sm).child("Merge queue is empty.")),
        ),
      );
    } else {
      for stack in active.iter().rev() {
        column = column.child(self.stack_block(stack, true, p, cx));
      }
    }
    column = column.child(self.target_marker(&target, tip_title(tip_entry), p, cx));
    if self.route.show_merged_history && !visible.is_empty() {
      let mut past = div()
        .debug_selector(|| "merge-queue-history".into())
        .flex()
        .flex_col()
        .gap_3()
        .pt_1();
      for stack in &visible {
        past = past.child(self.stack_block(stack, false, p, cx));
      }
      if more {
        let next = self.route.history_limit + HISTORY_PAGE;
        past = past.child(
          div().flex().justify_center().pt_1().pb_1().child(
            Button::new("merge-queue-load-more", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Load more")
              .on_click(cx.listener(move |view, _, _, cx| {
                view.field("mergeQueue.historyLimit", json!(next), cx)
              })),
          ),
        );
      }
      column = column.child(past);
    }
    // The rail runs through every card and the gaps between them, under
    // the nodes: the cards are drawn without a background of their own,
    // which is the panel's.
    let rail = div()
      .debug_selector(|| "merge-queue-rail".into())
      .absolute()
      .left(zpx(35. - 12.))
      .top_0()
      .bottom_0()
      .w(zpx(2.))
      .bg(p.border);
    div()
      .debug_selector(|| "merge-queue-list".into())
      .size_full()
      .flex()
      .flex_col()
      .child(
        div()
          .id("merge-queue-scroll")
          .flex_1()
          .overflow_y_scroll()
          .p_3()
          .child(div().relative().child(rail).child(column)),
      )
  }

  /// `QueueStackBlock`: the stack's branches, next to merge at the bottom.
  fn stack_block(
    &mut self,
    stack: &QueueStack,
    removable: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let is_stack = stack.entries.len() > 1;
    let key = stack.entries[0].branch_name.clone();
    let max = max_change(&stack.entries);
    let dequeuing = self.queue.dequeuing;
    let selector = if is_stack {
      format!("merge-queue-stack-{key}")
    } else {
      format!("merge-queue-single-{key}")
    };
    let mut card = div()
      .debug_selector(move || selector)
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .p_4();
    if is_stack {
      let n = stack.entries.len();
      let branches: Vec<String> = stack
        .entries
        .iter()
        .map(|e| e.branch_name.clone())
        .collect();
      let help = div()
        .id(SharedString::from(format!("stack-help-{key}")))
        .cursor_pointer()
        .child(ui::icon(
          theme::ICON_CIRCLE_HELP,
          14.,
          p.muted_foreground.opacity(0.7),
        ))
        .on_click({
          let host = self.host.clone();
          move |_, _, _| host.open_url(&stack_docs())
        });
      card = card.child(
        ui::text(div(), TextSize::Sm)
          .flex()
          .items_center()
          .gap_1p5()
          .font_weight(FontWeight::SEMIBOLD)
          .text_color(p.muted_foreground)
          .mb_3()
          .pl_6()
          .child(ui::icon(theme::ICON_LAYERS, 16., p.muted_foreground))
          .child(format!("Stack of {n}"))
          .child(
            self.tips.wrap(
              format!("stack-help-tip-{key}"),
              Side::Top,
              Some(TooltipContent::Text(
                format!(
                  "Stacked PRs merge bottom-up into {}: the lowest PR lands first, then each PR \
                 above it.",
                  stack.target_branch
                )
                .into(),
              )),
              help,
              cx,
            ),
          )
          .child(div().flex_1())
          .when(removable, |this| {
            this.child(
              Button::new(SharedString::from(format!("merge-queue-remove-{key}")), p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .h_7()
                .font_weight(FontWeight::NORMAL)
                .disabled(dequeuing)
                .label("Remove")
                .on_click(cx.listener(move |view, _, _, cx| view.dequeue(branches.clone(), cx))),
            )
          }),
      );
    }
    let mut list = div().flex().flex_col();
    for entry in stack.entries.iter().rev() {
      list = list.child(self.entry_row(entry, is_stack, removable, max, p, cx));
    }
    card.child(list)
  }

  fn entry_row(
    &self,
    entry: &QueueEntry,
    in_stack: bool,
    removable: bool,
    max: u32,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let tone = match entry.status.as_str() {
      "testing" => Tone::Warning,
      "merging" | "merged" => Tone::Success,
      "failed" => Tone::Danger,
      _ => Tone::Neutral,
    };
    let node = tone.color(p);
    let (chip_bg, chip_fg) = match tone {
      Tone::Neutral => (p.muted, p.muted_foreground),
      tone => (tone.tint(p), tone.text(p)),
    };
    let (insertions, deletions) = (entry.insertions.unwrap_or(0), entry.deletions.unwrap_or(0));
    let branch = entry.branch_name.clone();
    let selector = format!("merge-queue-entry-{}", entry.position);
    div()
      .debug_selector(move || selector)
      .flex()
      .w_full()
      .items_start()
      .gap_3()
      .py_2()
      .child(
        div().flex_none().mt_0p5().size(zpx(14.)).child(
          div()
            .size(zpx(14.))
            .rounded_full()
            .border_2()
            .border_color(p.background)
            .bg(node),
        ),
      )
      .child(
        div()
          .flex_1()
          .min_w_0()
          .child(
            div()
              .flex()
              .items_center()
              .gap_2()
              .min_w_0()
              .child(
                ui::text(div(), TextSize::Sm)
                  .flex_none()
                  .text_color(p.muted_foreground)
                  .child(format!("#{}", entry.position)),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .truncate()
                  .font_weight(FontWeight::MEDIUM)
                  .child(match entry.pr_number {
                    Some(n) => format!("PR #{n}"),
                    None => "No PR".into(),
                  }),
              )
              .when(removable && !in_stack, |this| {
                this.child(
                  div().ml_auto().child(
                    Button::new(
                      SharedString::from(format!("merge-queue-remove-{}", entry.branch_name)),
                      p,
                    )
                    .variant(Variant::Outline)
                    .size(Size::Sm)
                    .h_7()
                    .font_weight(FontWeight::NORMAL)
                    .disabled(self.queue.dequeuing)
                    .label("Remove")
                    .on_click(
                      cx.listener(move |view, _, _, cx| view.dequeue(vec![branch.clone()], cx)),
                    ),
                  ),
                )
              }),
          )
          .child(
            div()
              .flex()
              .flex_wrap()
              .items_center()
              .gap_2()
              .mt_0p5()
              .min_w_0()
              .child(
                ui::text(div(), TextSize::Xs)
                  .flex_none()
                  .px_2()
                  .py_0p5()
                  .rounded_full()
                  .bg(chip_bg)
                  .text_color(chip_fg)
                  .child(status_label(&entry.status).to_string()),
              )
              .child(
                ui::text(div(), TextSize::Xs)
                  .min_w_0()
                  .truncate()
                  .font_family(theme::MONO_FONT)
                  .text_color(p.muted_foreground)
                  .child(format!("{} → {}", entry.branch_name, entry.target_branch)),
              )
              .children(self.diff_stats(insertions, deletions, max, p)),
          ),
      )
  }

  /// `DiffStatsInline`: `+N`, the bar, `-N`.
  fn diff_stats(&self, insertions: u32, deletions: u32, max: u32, p: &Palette) -> Option<Div> {
    if insertions == 0 && deletions == 0 {
      return None;
    }
    let (add, del) = bar_widths(insertions, deletions, max);
    let (green, red) = (Tone::GitOpen.color(p), Tone::GitClosed.color(p));
    let half = super::merge_queue_data::BAR_HALF;
    Some(
      div()
        .debug_selector(|| "diff-stats-inline".into())
        .ml_auto()
        .flex()
        .items_center()
        .gap_1p5()
        .child(
          ui::text(div(), TextSize::Xs)
            .font_family(theme::MONO_FONT)
            .text_color(green)
            .child(format!("+{insertions}")),
        )
        .child(
          div()
            .flex()
            .flex_none()
            .h(zpx(6.))
            .items_center()
            .child(
              div()
                .w(zpx(half))
                .h_full()
                .flex()
                .justify_end()
                .child(div().h_full().w(zpx(add)).bg(green)),
            )
            .child(div().w(zpx(1.)).h_full().bg(p.border))
            .child(
              div()
                .w(zpx(half))
                .h_full()
                .flex()
                .child(div().h_full().w(zpx(del)).bg(red)),
            ),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .font_family(theme::MONO_FONT)
            .text_color(red)
            .child(format!("-{deletions}")),
        ),
    )
  }

  /// `TargetBranchMarker`: where the stacks land, and its tip.
  fn target_marker(
    &mut self,
    target: &str,
    title: Option<String>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let tip = self.local.tip.clone().map(|(short, _)| short);
    let copied = self.copied;
    let copy = tip.as_ref().map(|short| {
      let button = Button::new("merge-queue-copy-sha", p)
        .aria_label(if copied {
          "Commit SHA copied"
        } else {
          "Copy commit SHA"
        })
        .variant(Variant::Ghost)
        .size(Size::IconXs)
        .text_color(p.muted_foreground)
        .icon(
          if copied {
            theme::ICON_CHECK
          } else {
            theme::ICON_COPY
          },
          12.,
        )
        .on_click(cx.listener(|view, _, _, cx| view.copy_tip(cx)));
      div()
        .ml_auto()
        .flex()
        .flex_none()
        .items_center()
        .gap_0p5()
        .child(
          ui::text(div(), TextSize::Xs)
            .debug_selector(|| "merge-queue-tip-sha".into())
            .font_family(theme::MONO_FONT)
            .text_color(p.muted_foreground)
            .child(short.clone()),
        )
        .child(self.tips.wrap(
          "merge-queue-copy-sha-tip",
          Side::Top,
          Some(TooltipContent::Text(
            if copied { "Copied" } else { "Copy commit SHA" }.into(),
          )),
          button,
          cx,
        ))
    });
    div()
      .debug_selector(|| "merge-queue-target".into())
      .flex()
      .items_center()
      .gap_3()
      .py_2()
      .pl_4()
      .text_color(p.muted_foreground)
      .child(
        div()
          .flex_none()
          .size(zpx(14.))
          .flex()
          .items_center()
          .justify_center()
          .child(ui::icon(theme::ICON_ARROW_DOWN, 14., p.muted_foreground)),
      )
      .child(
        div()
          .min_w_0()
          .flex_1()
          .flex()
          .items_center()
          .gap_2()
          .child(
            div()
              .min_w_0()
              .flex()
              .items_center()
              .gap_2()
              .child(
                ui::text(div(), TextSize::Xs)
                  .debug_selector(|| "merge-queue-target-branch".into())
                  .flex_none()
                  .px_1p5()
                  .py_0p5()
                  .rounded(radius::SM)
                  .font_family(theme::MONO_FONT)
                  .font_weight(FontWeight::MEDIUM)
                  .bg(p.muted)
                  .text_color(p.foreground)
                  .child(target.to_string()),
              )
              .children(title.map(|t| {
                ui::text(div(), TextSize::Sm)
                  .min_w_0()
                  .truncate()
                  .text_color(p.foreground)
                  .child(t)
              })),
          )
          .children(copy),
      )
  }
}
