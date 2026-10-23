//! The workspace view (React's `ShowWorkspace`): its header, tab row, the
//! tab's content and the terminal pane.
//!
//! The header reads its data from the backend ([`workspace_data`]) and
//! pushes, syncs and rebases the home branch itself. The window model says
//! which workspace shows. The popovers under the target branch, checks and
//! details buttons open in the modal window (see
//! [`popover`](crate::gpui_shell::popover)); menus open below the button
//! that asked for them.
//!
//! [`workspace_data`]: super::workspace_data

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, Animation, AnimationExt as _, App, Bounds, Context, Div, Entity,
  FontWeight, Hsla, MouseDownEvent, Pixels, SharedString, Stateful, Task, WeakEntity, Window,
};

use super::launch_data::LaunchRequest;
use super::{
  changes::{self, ChangesView},
  code::{CodeView, Parent},
  commits::CommitsView,
  commits_data::CommitsHost,
  file_browser::{FileBrowserView, Opening},
  file_browser_data::FileBrowserHost,
  logs::LogsView,
  logs_data::LogsHost,
  terminal::{Place, TerminalPane},
  terminal_data::TerminalHost,
  workspace_data::{
    workspace_view, Loaded, Outcome, Pending, PrCache, Route, WorkspaceHost, DEFAULT_BRANCH,
  },
  ActionSink, ScreenAction,
};
use crate::gpui_shell::{
  native::{NativeEvent, ToastTone},
  popover::{self, Popover, PopoverHost},
  popup::{self, PopupRequest, PopupSink, Side, TooltipContent},
  theme::{self, Palette},
  ui::{self, Button, Size, TextSize, Variant},
};

mod actions;

/// React's base-ui tooltips open after 200ms (`TooltipProvider delay={200}`).
const TOOLTIP_DELAY: Duration = Duration::from_millis(200);

/// Where the terminal pane was last placed: the repository, workspace and
/// sessions revision.
type TerminalPlace = (String, Option<i64>, u64);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
  pub header: Header,
  pub tab_row: Option<TabRow>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
  pub branch: String,
  pub home: bool,
  pub linear: Option<Linear>,
  pub target: Option<Target>,
  pub home_target: Option<HomeTarget>,
  pub stack: Option<Tooltip>,
  pub push: Option<Disabled>,
  pub view_pr: Option<ViewPr>,
  pub create_pr: Option<CreatePr>,
  pub status_loading: bool,
  pub sync: Option<Sync>,
  pub checks: Option<Checks>,
  pub merge_queue: Option<MergeQueue>,
  pub merge: Option<Merge>,
  pub details: bool,
  pub more: Option<Disabled>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Linear {
  pub key: String,
  pub url: String,
  pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Target {
  pub branch: Option<String>,
  pub loading: bool,
  pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HomeTarget {
  pub branch: String,
  pub rebase: Option<Rebase>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Rebase {
  pub tooltip: String,
  pub rebasing: bool,
  pub conflicts: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Tooltip {
  pub tooltip: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Disabled {
  pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PrState {
  Open,
  Closed,
  Merged,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ViewPr {
  pub number: u64,
  pub state: PrState,
  pub draft: bool,
}

/// React's `View PR (#12, open)`: the number and state the icon and color show.
fn view_pr_name(pr: &ViewPr) -> String {
  let state = if pr.draft {
    "draft"
  } else {
    match pr.state {
      PrState::Open => "open",
      PrState::Closed => "closed",
      PrState::Merged => "merged",
    }
  };
  format!("View PR (#{}, {state})", pr.number)
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePr {
  pub label: String,
  pub creating: bool,
  pub disabled: bool,
  pub tooltip: String,
  pub menu_tooltip: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Sync {
  pub ahead: u32,
  pub behind: u32,
  pub disabled: bool,
  pub spinning: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Checks {
  /// "passed", "failed" or "running"; None for nothing run yet.
  pub status: Option<String>,
  /// Treq's and GitHub's checks together, as "passed/total".
  pub count: String,
  /// The accessible name, naming each source ("Checks: Treq 3/3, GitHub 4/4").
  pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MergeQueue {
  pub label: String,
  pub secondary: bool,
  pub disabled: bool,
  pub tooltip: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Merge {
  pub passing: bool,
  pub disabled: bool,
  pub tooltip: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
  Overview,
  Commits,
  Changes,
  Logs,
}

impl Tab {
  fn value(self) -> &'static str {
    match self {
      Tab::Overview => "overview",
      Tab::Commits => "commits",
      Tab::Changes => "changes",
      Tab::Logs => "logs",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
  Muted,
  Conflict,
  Uncommitted,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CommitsLabel {
  Count { count: u32, tone: Tone },
  HomeConflict,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Pill {
  pub count: u32,
  pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Loc {
  pub insertions: u32,
  pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TabRow {
  pub tab: Tab,
  pub commits: Option<CommitsLabel>,
  pub changes: Option<Pill>,
  pub logs: bool,
  pub busy: Option<String>,
  pub loc: Option<Loc>,
}

/// How often the branch's pull request is re-read from the PR poller's
/// cache, which the poller itself refreshes.
const PR_POLL: Duration = Duration::from_secs(5);

/// The header and tab row, drawn from what they read from the backend.
pub struct WorkspaceView {
  host: Arc<dyn WorkspaceHost>,
  /// What the header's own actions ask of the backend.
  actions: Arc<dyn super::workspace_ops::WorkspaceActionsHost>,
  /// Those actions in flight.
  busy: actions::Busy,
  /// The open confirmation (Delete workspace).
  confirm: Option<crate::gpui_shell::confirm::Pending<Self>>,
  popover_host: Arc<dyn PopoverHost>,
  code_host: Arc<dyn TerminalHost>,
  /// The header popover that shows in the modal window, if one does.
  popover: Option<(Entity<Popover>, gpui::Subscription)>,
  /// The window this view draws in, which a press outside the popover goes
  /// on to.
  window: Option<gpui::AnyWindowHandle>,
  /// The terminal pane, while GPUI draws it, and the workspace it was
  /// last placed in.
  terminal: Option<(Entity<TerminalPane>, TerminalPlace)>,
  /// The Code tab, kept while other tabs show so it draws at once on return.
  pub(super) code: Option<Entity<CodeView>>,
  /// Whether the Code tab is the one drawn.
  code_shown: bool,
  /// The Code tab's file browser, while GPUI draws it.
  pub(super) file_browser: Option<Entity<FileBrowserView>>,
  /// The Commits tab, while GPUI draws it.
  pub(super) commits: Option<Entity<CommitsView>>,
  /// The Changes tab, while GPUI draws it.
  pub(super) changes: Option<Entity<ChangesView>>,
  /// The Logs tab and the `logs_key` it started on.
  logs: Option<(Entity<LogsView>, u64)>,
  /// React, for what the header does not do itself.
  react: ActionSink,
  /// Where the tabs and the terminal pane send their actions: React, but
  /// for the agent sessions this view starts itself.
  children: ActionSink,
  /// This view's own handler, which defers to after the current update.
  sink: ActionSink,
  route: Route,
  /// `None` until the first load for `route`'s workspace.
  loaded: Option<Loaded>,
  pr: PrCache,
  pending: Pending,
  loading: bool,
  /// Another load was asked for while one ran.
  reload: bool,
  /// What `loaded` draws as, `None` before it is.
  snapshot: Option<Workspace>,
  dark: bool,
  popups: PopupSink,
  /// The element whose tooltip is pending or shown (`true`).
  tooltip: Option<(SharedString, bool)>,
  /// The tooltip whose popup is open.
  tooltip_sent: Rc<RefCell<Option<SharedString>>>,
  /// Where dropdown buttons were last drawn, in window coordinates.
  bounds: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
  _poll: Task<()>,
  _place: Task<()>,
  /// Draws checks as running while one runs anywhere in the app.
  _running: gpui::Subscription,
}

impl WorkspaceView {
  pub fn new(
    route: Route,
    host: Arc<dyn TerminalHost>,
    react: ActionSink,
    dark: bool,
    popups: PopupSink,
    cx: &mut Context<Self>,
  ) -> Self {
    let view = cx.weak_entity();
    let sink: ActionSink = Rc::new(move |action, cx| {
      let view = view.clone();
      cx.defer(move |cx| {
        view
          .update(cx, |view, cx| view.handle(action, cx))
          .ignore_closed();
      });
    });
    let children: ActionSink = {
      let (view, react) = (cx.weak_entity(), react.clone());
      Rc::new(move |action, cx| {
        if !launches(&action) {
          return react(action, cx);
        }
        let view = view.clone();
        cx.defer(move |cx| {
          view
            .update(cx, |view, cx| view.launch(action, cx))
            .ignore_closed();
        });
      })
    };
    let poll = cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(PR_POLL).await;
      if this.update(cx, |view, cx| view.read_pr(cx)).is_err() {
        break;
      }
    });
    let running =
      cx.observe_global::<crate::gpui_shell::checks_run::Running>(|view, cx| view.rebuild(cx));
    let mut this = Self {
      host: host.clone(),
      actions: host.clone(),
      busy: actions::Busy::default(),
      confirm: None,
      popover_host: host.clone(),
      popover: None,
      window: None,
      code_host: host,
      terminal: None,
      code: None,
      code_shown: false,
      file_browser: None,
      commits: None,
      changes: None,
      logs: None,
      react,
      children,
      sink,
      route,
      loaded: None,
      pr: PrCache::default(),
      pending: Pending::None,
      loading: false,
      reload: false,
      snapshot: None,
      dark,
      popups,
      tooltip: None,
      tooltip_sent: Rc::default(),
      bounds: Rc::default(),
      _poll: poll,
      _place: Task::ready(()),
      _running: running,
    };
    this.load(cx);
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.route == route && self.dark == dark {
      return;
    }
    let moved = self.route.key() != route.key();
    let stale = moved || self.route.revision != route.revision;
    self.route = route;
    self.dark = dark;
    if moved {
      self.close_popover(cx);
      self.loaded = None;
      self.pr = PrCache::default();
      self.pending = Pending::None;
      self.busy = actions::Busy::default();
    }
    if stale {
      self.load(cx);
    }
    self.rebuild(cx);
  }

  /// Reads everything again off the main thread; a load asked for while one
  /// runs happens after it, once.
  fn load(&mut self, cx: &mut Context<Self>) {
    if self.loading {
      self.reload = true;
      return;
    }
    self.loading = true;
    let (host, route) = (self.host.clone(), self.route.clone());
    cx.spawn(async move |this, cx| {
      let key = route.key();
      let loaded = cx.background_spawn(async move { host.load(&route) }).await;
      this
        .update(cx, |view, cx| {
          view.loading = false;
          if view.route.key() == key {
            view.loaded = Some(loaded);
            view.read_pr(cx);
            view.rebuild(cx);
          }
          if std::mem::take(&mut view.reload) {
            view.load(cx);
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  /// The branch whose pull request the header shows, if any.
  fn pr_branch(&self) -> Option<String> {
    let loaded = self.loaded.as_ref()?;
    let workspace = loaded.workspace.as_ref()?;
    (loaded.remote.is_some() && workspace.branch_name != DEFAULT_BRANCH)
      .then(|| workspace.branch_name.clone())
  }

  fn read_pr(&mut self, cx: &mut Context<Self>) {
    let Some(branch) = self.pr_branch() else {
      return;
    };
    let (host, repo) = (self.host.clone(), self.route.repo_path.clone());
    let key = self.route.key();
    cx.spawn(async move |this, cx| {
      let pr = cx
        .background_spawn(async move { host.pr_cache(&repo, &branch) })
        .await;
      this
        .update(cx, |view, cx| {
          if view.route.key() == key && view.pr != pr {
            view.pr = pr;
            view.rebuild(cx);
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  fn rebuild(&mut self, cx: &mut Context<Self>) {
    let route = self.drawn_route(cx);
    // While another workspace loads, the last one stays drawn and swaps in
    // one frame once it lands, rather than flashing an empty header.
    let Some(loaded) = self.loaded.as_ref() else {
      self.sync_popover(cx);
      return;
    };
    let (header, tab_row) = workspace_view(&route, loaded, &self.pr, self.pending);
    let snapshot = Some(Workspace { header, tab_row });
    if self.snapshot != snapshot {
      self.snapshot = snapshot;
      cx.notify();
    }
    self.sync_popover(cx);
  }

  /// What the Checks popover draws, from what the header read.
  fn checks_input(&self, cx: &App) -> Option<popover::ChecksInput> {
    let loaded = self.loaded.as_ref()?;
    let route = self.drawn_route(cx);
    Some(popover::ChecksInput {
      data: loaded.checks.clone()?,
      ci: self.pr.ci.clone().filter(|ci| ci.total > 0),
      running: route.running_checks,
      running_workflows: route.running_workflows,
      setup_rerunning: route.setup_rerunning,
      logs: loaded.logs_enabled,
    })
  }

  /// The popover that header button `key` opens, for the shown workspace.
  fn popover_kind(&self, key: &str, cx: &App) -> Option<popover::Kind> {
    let loaded = self.loaded.as_ref()?;
    let repo_path = self.route.repo_path.clone();
    match key {
      "workspace-details" => {
        let workspace = loaded.workspace.as_ref()?;
        Some(popover::Kind::Details(popover::DetailsSpec {
          repo_path,
          workspace_id: workspace.id,
          branch: workspace.branch_name.clone(),
          target: workspace
            .target_branch
            .clone()
            .unwrap_or_else(|| self.route.default_target().to_string()),
          created_at: workspace.created_at.clone(),
        }))
      }
      "workspace-target" => {
        let target = self.snapshot.as_ref()?.header.target.as_ref()?;
        Some(popover::Kind::Target(popover::TargetSpec {
          repo_path,
          current: target.branch.clone(),
        }))
      }
      "workspace-checks" => Some(popover::Kind::Checks(self.checks_input(cx)?)),
      _ => None,
    }
  }

  /// Opens the popover under header button `key` in the modal window.
  fn open_popover(&mut self, key: &'static str, cx: &mut Context<Self>) {
    let Some(anchor) = self.bounds.borrow().get(key).copied() else {
      return;
    };
    let Some(kind) = self.popover_kind(key, cx) else {
      return;
    };
    self.close_popover(cx);
    // The button's tooltip would cover the popover and swallow the next
    // click; `hover_tooltip` keeps it hidden while the popover shows.
    if self.tooltip.take().is_some() {
      self.sync_tooltip_popup(cx);
      cx.notify();
    }
    let (host, react, dark) = (self.popover_host.clone(), self.sink.clone(), self.dark);
    let shell = self.window;
    let popover = cx.new(|cx| Popover::new(kind, anchor, shell, dark, host, react, cx));
    let subscription = cx.subscribe(&popover, |this, _, _: &popover::Closed, cx| {
      this.close_popover(cx)
    });
    self.popover = Some((popover.clone(), subscription));
    (self.popups)(PopupRequest::ShowPopover { popover, dark }, cx);
  }

  fn close_popover(&mut self, cx: &mut Context<Self>) {
    if self.popover.take().is_some() {
      (self.popups)(PopupRequest::HidePopover, cx);
    }
  }

  /// Passes the open popover what changed under it.
  fn sync_popover(&mut self, cx: &mut Context<Self>) {
    let Some(popover) = self.popover.as_ref().map(|(popover, _)| popover.clone()) else {
      return;
    };
    let input = self.checks_input(cx);
    let dark = self.dark;
    popover.update(cx, |popover, cx| {
      popover.set_dark(dark, cx);
      if let Some(input) = input {
        popover.set_checks(input, cx);
      }
    });
  }

  pub(super) fn handle(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    if self.act(&action, cx) {
      return;
    }
    let (repo, workspace_id) = (self.route.repo_path.clone(), self.route.workspace_id);
    match action {
      ScreenAction::Press { id } => match id.as_str() {
        "workspace.push" => self.run(Pending::Push, cx, move |host| {
          host.push(&repo, workspace_id)
        }),
        "workspace.sync" => self.run(Pending::Sync, cx, move |host| {
          host.sync(&repo, workspace_id)
        }),
        "workspace.homeRebase" if !self.route.home_branch.is_empty() => {
          let branch = self.route.home_branch.clone();
          self.run(Pending::HomeRebase, cx, move |host| {
            host.rebase_home(&repo, &branch)
          })
        }
        "workspace.details" => self.open_popover("workspace-details", cx),
        "workspace.target" => self.open_popover("workspace-target", cx),
        "workspace.checks" => self.open_popover("workspace-checks", cx),
        "workspace.viewPrWeb" => {
          if let Some(Some(pr)) = &self.pr.pr {
            self.host.open_url(&pr.url);
          }
        }
        _ => (self.react)(ScreenAction::Press { id }, cx),
      },
      ScreenAction::OpenUrl { url } => self.host.open_url(&url),
      action => (self.react)(action, cx),
    }
  }

  /// Runs a header action off the main thread, then toasts its outcome and
  /// reads everything again, as React's handlers do.
  fn run(
    &mut self,
    pending: Pending,
    cx: &mut Context<Self>,
    work: impl FnOnce(&dyn WorkspaceHost) -> Outcome + Send + 'static,
  ) {
    if self.pending != Pending::None {
      return;
    }
    self.pending = pending;
    self.rebuild(cx);
    let host = self.host.clone();
    let repo_path = self.route.repo_path.clone();
    cx.spawn(async move |this, cx| {
      let outcome = cx.background_spawn(async move { work(&*host) }).await;
      this
        .update(cx, |view, cx| {
          view.pending = Pending::None;
          view.host.notify(NativeEvent::Toast {
            title: outcome.title,
            description: outcome.description,
            tone: match (outcome.ok, outcome.warning) {
              (false, _) => ToastTone::Error,
              (true, true) => ToastTone::Warning,
              (true, false) => ToastTone::Success,
            },
            action: None,
          });
          view
            .host
            .notify(NativeEvent::WorkspaceChanged { repo_path });
          view.load(cx);
          view.rebuild(cx);
        })
        .ignore_closed();
    })
    .detach();
  }

  fn press(&self, id: &'static str) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) {
    let sink = self.sink.clone();
    move |_, _, cx| sink(ScreenAction::Press { id: id.to_string() }, cx)
  }

  /// The open PR's GitHub URL.
  fn pr_url(&self) -> Option<String> {
    self.pr.pr.as_ref()?.as_ref().map(|pr| pr.url.clone())
  }

  /// A right-click on a button that opens `url`: its "Copy link" menu.
  fn copy_link(
    &self,
    url: Option<String>,
  ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    let (popups, host, dark) = (self.popups.clone(), self.host.clone(), self.dark);
    move |e, _, cx| {
      let Some(url) = url.clone() else {
        return;
      };
      let host = host.clone();
      let menu = popup::copy_link_menu(url, e.position, dark, move |e| host.notify(e));
      popups(menu, cx);
    }
  }

  /// Records the bounds of its parent as `key`'s, for a dropdown to open
  /// below.
  fn record_bounds(&self, key: &'static str) -> impl IntoElement {
    let bounds = self.bounds.clone();
    gpui::canvas(
      move |b, _, _| {
        bounds.borrow_mut().insert(key.into(), b);
      },
      |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
  }

  fn hover_tooltip(&mut self, key: SharedString, hovered: bool, cx: &mut Context<Self>) {
    if !hovered {
      if self.tooltip.as_ref().is_some_and(|(k, _)| *k == key) {
        self.tooltip = None;
        cx.notify();
      }
      return;
    }
    if self.popover.is_some() {
      return;
    }
    self.tooltip = Some((key.clone(), false));
    cx.spawn(async move |this, cx| {
      cx.background_executor().timer(TOOLTIP_DELAY).await;
      this
        .update(cx, |this, cx| {
          if let Some((k, shown)) = this.tooltip.as_mut() {
            if *k == key && !*shown {
              *shown = true;
              cx.notify();
            }
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Wraps `el` so hovering it shows `content` below it, as `Tooltip` does.
  fn with_tooltip(
    &self,
    key: &'static str,
    content: Option<TooltipContent>,
    el: impl IntoElement,
    cx: &mut Context<Self>,
  ) -> Stateful<Div> {
    let shown = matches!(&self.tooltip, Some((k, true)) if k.as_ref() == key);
    let wrapper = div()
      .id(SharedString::from(format!("{key}-tooltip")))
      .relative()
      .flex()
      .flex_none()
      .child(el);
    let Some(content) = content else {
      return wrapper;
    };
    wrapper
      .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
        this.hover_tooltip(key.into(), *hovered, cx);
      }))
      // A click closes it, as base-ui's does.
      .on_mouse_down(
        gpui::MouseButton::Left,
        cx.listener(|this, _, _, cx| {
          if this.tooltip.take().is_some() {
            cx.notify();
          }
        }),
      )
      .when(shown, |this| {
        let sent = self.tooltip_sent.clone();
        let popups = self.popups.clone();
        let dark = self.dark;
        let key = SharedString::from(key);
        this.child(
          gpui::canvas(
            move |anchor, _, cx| {
              if sent.borrow().as_ref() != Some(&key) {
                *sent.borrow_mut() = Some(key);
                popups(
                  PopupRequest::ShowTooltip {
                    anchor,
                    side: Side::Bottom,
                    content,
                    dark,
                  },
                  cx,
                );
              }
            },
            |_, _, _, _| {},
          )
          .absolute()
          .top_0()
          .left_0()
          .size_full(),
        )
      })
  }

  /// Closes the tooltip popup once no tooltip shows.
  fn sync_tooltip_popup(&self, cx: &mut App) {
    let shown = matches!(self.tooltip, Some((_, true)));
    if !shown && self.tooltip_sent.borrow_mut().take().is_some() {
      (self.popups)(PopupRequest::HideTooltip, cx);
    }
  }

  fn header(&self, h: &Header, p: &Palette, cx: &mut Context<Self>) -> Div {
    let mono_title = |text: &str| {
      ui::text(div(), TextSize::Sm)
        .font_family(theme::MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .min_w_0()
        .max_w(zpx(220.))
        .truncate()
        .child(text.to_string())
    };

    let mut left = div()
      .flex()
      .items_center()
      .gap_2()
      .min_w_0()
      .child(ui::icon(theme::ICON_BRANCH_BOLD, 16., p.muted_foreground));

    if !h.home {
      left = left.child(mono_title(&h.branch).debug_selector(|| "workspace-branch".into()));
      if let Some(linear) = &h.linear {
        let url = linear.url.clone();
        let sink = self.sink.clone();
        left = left.child(
          ui::text(div(), TextSize::Xs)
            .id("workspace-linear")
            .debug_selector(|| "workspace-linear".into())
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .px_2()
            .py_0p5()
            .rounded_full()
            .bg(p.muted)
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.muted_foreground)
            .cursor_pointer()
            .hover(|s| s.bg(p.muted.opacity(0.8)))
            .child(ui::icon(theme::ICON_ZAP, 12., p.muted_foreground))
            .child(linear.key.clone())
            .on_click(move |_, _, cx| sink(ScreenAction::OpenUrl { url: url.clone() }, cx)),
        );
      }
      if let Some(target) = &h.target {
        let label = target.branch.clone().unwrap_or_else(|| "Select...".into());
        let button = Button::new("workspace-target", p)
          .aria_label("Workspace target")
          .variant(Variant::Outline)
          .size(Size::Sm)
          .disabled(target.disabled)
          .max_w(zpx(220.))
          .map(|b| {
            let fg = b.foreground();
            if target.loading {
              b.child(ui::spinner("workspace-target-spin", 16., fg))
            } else {
              b.icon(theme::ICON_BRANCH_BOLD, 16.)
            }
          })
          .child(
            div()
              .font_family(theme::MONO_FONT)
              .min_w_0()
              .truncate()
              .child(label),
          )
          .child(ui::icon(
            theme::ICON_CHEVRON_DOWN,
            16.,
            p.foreground.opacity(0.5),
          ))
          .on_click(self.press("workspace.target"));
        left = left
          .child(ui::icon(theme::ICON_ARROW_RIGHT, 16., p.muted_foreground))
          .child(
            div()
              .relative()
              .flex_none()
              .child(button)
              .child(self.record_bounds("workspace-target")),
          );
      }
      if h.details {
        let button = Button::new("workspace-details", p)
          .aria_label("Workspace details")
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .px_1()
          .icon(theme::ICON_INFO, 16.)
          .on_click(self.press("workspace.details"));
        left = left.child(
          div()
            .relative()
            .flex_none()
            .child(self.with_tooltip(
              "workspace-details",
              Some(TooltipContent::Text("Workspace details".into())),
              button,
              cx,
            ))
            .child(self.record_bounds("workspace-details")),
        );
      }
    } else {
      left = left.child(
        mono_title(&h.branch)
          .id("workspace-branch")
          .debug_selector(|| "workspace-branch".into())
          .cursor_pointer()
          .hover(|s| s.underline())
          .on_click(self.press("workspace.branchSwitcher")),
      );
      if let Some(home_target) = &h.home_target {
        left = left
          .child(ui::icon(theme::ICON_ARROW_RIGHT, 16., p.muted_foreground))
          .child(
            ui::text(div(), TextSize::Sm)
              .font_family(theme::MONO_FONT)
              .text_color(p.muted_foreground)
              .min_w_0()
              .max_w(zpx(220.))
              .truncate()
              .child(home_target.branch.clone()),
          );
        if let Some(rebase) = &home_target.rebase {
          if let Some(files) = &rebase.conflicts {
            let warning = ui::Tone::Warning.text(p);
            let content = if files.is_empty() {
              TooltipContent::Text("Potential conflicts detected".into())
            } else {
              TooltipContent::List {
                title: "Potential conflicts detected".into(),
                items: files.iter().cloned().map(Into::into).collect(),
              }
            };
            left = left.child(self.with_tooltip(
              "workspace-rebase-conflicts",
              Some(content),
              ui::icon(theme::ICON_TRIANGLE_ALERT, 16., warning),
              cx,
            ));
          }
          let button = Button::new("workspace-home-rebase", p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .gap_1()
            .px_2()
            .disabled(rebase.rebasing)
            .map(|b| {
              let fg = b.foreground();
              if rebase.rebasing {
                b.child(ui::spinner("workspace-rebase-spin", 16., fg))
              } else {
                b.icon(theme::ICON_COMPARE, 16.)
              }
            })
            .label("Rebase")
            .on_click(self.press("workspace.homeRebase"));
          left = left.child(self.with_tooltip(
            "workspace-home-rebase",
            Some(TooltipContent::Text(rebase.tooltip.clone().into())),
            button,
            cx,
          ));
        }
      }
      if let Some(stack) = &h.stack {
        let button = Button::new("workspace-stack", p)
          .size(Size::Sm)
          .gap_1()
          .px_2()
          .icon(theme::ICON_LAYERS, 16.)
          .label("Stack")
          .on_click(self.press("workspace.stack"));
        left = left.child(self.with_tooltip(
          "workspace-stack",
          Some(TooltipContent::Text(stack.tooltip.clone().into())),
          button,
          cx,
        ));
      }
    }

    // The row wraps it whole first; alone on its row and still too wide (at
    // a large zoom), its buttons wrap too, right-aligned.
    let mut right = div()
      .flex()
      .flex_wrap()
      .justify_end()
      .items_center()
      .gap_2()
      .min_w_0()
      .ml_auto();
    if let Some(push) = &h.push {
      let button = Button::new("workspace-push", p)
        .size(Size::Sm)
        .disabled(push.disabled)
        .icon(theme::ICON_UPLOAD, 16.)
        .label("Push")
        .on_click(self.press("workspace.push"));
      right = right.child(self.with_tooltip(
        "workspace-push",
        Some(TooltipContent::Text("Push to remote".into())),
        button,
        cx,
      ));
    }
    if let Some(checks) = &h.checks {
      right = right.child(
        div()
          .relative()
          .flex_none()
          .child(self.checks(checks, p))
          .child(self.record_bounds("workspace-checks")),
      );
    }
    if let Some(pr) = &h.view_pr {
      right = right.child(self.view_pr(pr, p, cx));
    }
    if let Some(create) = &h.create_pr {
      right = right.child(self.create_pr(create, p, cx));
    }
    if h.status_loading {
      right = right.child(
        div()
          .id("workspace-status-skeleton")
          .debug_selector(|| "workspace-status-skeleton".into())
          .h(zpx(24.))
          .w(zpx(56.))
          .rounded(radius::SM)
          .bg(p.muted.opacity(0.6))
          .with_animation("workspace-status-pulse", pulse(), |el, delta| {
            el.opacity(pulse_opacity(delta))
          }),
      );
    }
    if let Some(sync) = &h.sync {
      right = right.child(self.sync(sync, p, cx));
    }
    if let Some(queue) = &h.merge_queue {
      let button = Button::new("workspace-merge-queue", p)
        .variant(if queue.secondary {
          Variant::Secondary
        } else {
          Variant::Outline
        })
        .size(Size::Sm)
        .gap_1()
        .disabled(queue.disabled)
        .icon(theme::ICON_MERGE, 16.)
        .label(queue.label.clone())
        .on_click(self.press("workspace.mergeQueue"));
      right = right.child(self.with_tooltip(
        "workspace-merge-queue",
        Some(TooltipContent::Text(queue.tooltip.clone().into())),
        button,
        cx,
      ));
    }
    if let Some(merge) = &h.merge {
      let green = ui::Tone::Success.color(p);
      let hover = green.opacity(0.85);
      let button = Button::new("workspace-merge", p)
        .aria_label("Merge...")
        .variant(if merge.passing {
          Variant::Default
        } else {
          Variant::Outline
        })
        .size(Size::Sm)
        .gap_1()
        .disabled(merge.disabled)
        .when(merge.passing, |b| {
          b.bg(green).text_color(p.primary_foreground).hover_bg(hover)
        })
        .icon(theme::ICON_COMPARE, 16.)
        .label("Merge...")
        .on_click(self.press("workspace.merge"));
      right = right.child(
        self.with_tooltip(
          "workspace-merge",
          merge
            .tooltip
            .clone()
            .map(|t| TooltipContent::Text(t.into())),
          button,
          cx,
        ),
      );
    }
    if let Some(more) = &h.more {
      right = right.child(
        div()
          .relative()
          .flex_none()
          .child(
            Button::new("workspace-more", p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .px_1()
              .disabled(more.disabled)
              .icon(theme::ICON_MORE, 16.)
              .aria_label("More workspace actions")
              .on_click(self.press("workspace.more")),
          )
          .child(self.record_bounds("workspace-more")),
      );
    }

    div()
      .flex_none()
      .flex()
      .flex_col()
      .gap_1()
      .p_2()
      .border_b_1()
      .border_color(p.border)
      .debug_selector(|| "workspace-header".into())
      .child(
        div()
          .flex()
          .flex_wrap()
          .items_center()
          // React's `justify-between` too, which CSS ignores beside the
          // right group's `ml-auto`; taffy would add the free space twice.
          .gap_x_2()
          .gap_y_1()
          .child(left)
          .child(right),
      )
  }

  fn view_pr(&self, pr: &ViewPr, p: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
    // The PR's state as GitHub labels it, as a pill that is also the button:
    // the label opens the PR in Treq, the trailing half on GitHub.
    let draft = pr.draft && pr.state == PrState::Open;
    let (label, icon, tone) = match pr.state {
      _ if draft => ("Draft", theme::ICON_PR_DRAFT, ui::Tone::Neutral),
      PrState::Open => ("Open", theme::ICON_PR, ui::Tone::GitOpen),
      PrState::Closed => ("Closed", theme::ICON_PR_CLOSED, ui::Tone::GitClosed),
      PrState::Merged => ("Merged", theme::ICON_MERGE, ui::Tone::GitMerged),
    };
    let copy = self.copy_link(self.pr_url());
    let pill = ui::SplitButton::new("workspace-view-pr", label, p)
      .pill(tone)
      .icon(icon)
      .aria_label(view_pr_name(pr))
      .menu_label("Open PR on web")
      .menu_icon(theme::ICON_EXTERNAL_LINK)
      .on_click(self.press("workspace.viewPr"))
      .on_menu(self.press("workspace.viewPrWeb"))
      .on_right_click(move |e, w, cx| copy(e, w, cx));
    self.with_tooltip(
      "workspace-view-pr",
      Some(TooltipContent::Text(
        format!(
          "Pull request #{}: view in Treq, or open on GitHub",
          pr.number
        )
        .into(),
      )),
      pill,
      cx,
    )
  }

  fn create_pr(&self, create: &CreatePr, p: &Palette, cx: &mut Context<Self>) -> Div {
    let bg = p.github();
    let hover = bg.opacity(0.85);
    let white = p.primary_foreground;
    let styled = |b: Button| {
      b.size(Size::Sm)
        .disabled(create.disabled)
        .bg(bg)
        .text_color(white)
        .hover_bg(hover)
    };
    let main = styled(Button::new("workspace-create-pr", p))
      .gap_1()
      .rounded_r_none()
      .map(|b| {
        if create.creating {
          b.child(ui::spinner("workspace-create-pr-spin", 16., white))
        } else {
          b.child(ui::icon(theme::ICON_GITHUB, 16., white))
        }
      })
      .label(create.label.clone())
      .on_click(self.press("workspace.createPr"));
    let menu = styled(Button::new("workspace-create-pr-menu", p))
      .rounded_l_none()
      .border_l_1()
      .border_color(
        p.primary_foreground
          .opacity(if self.dark { 0.3 } else { 0.2 }),
      )
      .px_1p5()
      .child(ui::icon(theme::ICON_CHEVRON_DOWN, 16., white))
      .aria_label("More Create PR options")
      .on_click(self.press("workspace.createPrMenu"));
    div()
      .flex()
      .flex_none()
      .items_center()
      .child(self.with_tooltip(
        "workspace-create-pr",
        Some(TooltipContent::Text(create.tooltip.clone().into())),
        main,
        cx,
      ))
      .child(
        self
          .with_tooltip(
            "workspace-create-pr-menu",
            Some(TooltipContent::Text(create.menu_tooltip.clone().into())),
            menu,
            cx,
          )
          .child(self.record_bounds("workspace-create-pr-menu")),
      )
  }

  fn sync(&self, sync: &Sync, p: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
    let dot = ui::Tone::Warning.color(p);
    let button = Button::new("workspace-sync", p)
      .aria_label("Sync with remote")
      .variant(if sync.ahead > 0 {
        Variant::Outline
      } else {
        Variant::Ghost
      })
      .size(Size::Sm)
      .h(zpx(24.))
      .gap_1()
      .px_2()
      .text_color(p.muted_foreground)
      .disabled(sync.disabled)
      .when(sync.behind > 0, |b| b.child(format!("↓{}", sync.behind)))
      .when(sync.ahead > 0, |b| b.child(format!("↑{}", sync.ahead)))
      .map(|b| {
        let icon = ui::icon(theme::ICON_REFRESH, 16., p.muted_foreground);
        if sync.spinning {
          b.child(ui::spin("workspace-sync-spin", icon))
        } else {
          b.child(icon)
        }
      })
      .on_click(self.press("workspace.sync"));
    self
      .with_tooltip(
        "workspace-sync",
        Some(TooltipContent::Text(
          "Sync with remote (fetch and push)".into(),
        )),
        button,
        cx,
      )
      .child(
        div()
          .absolute()
          .top(zpx(-2.))
          .right(zpx(-2.))
          .size(zpx(8.))
          .rounded_full()
          .bg(dot)
          .with_animation("workspace-sync-pulse", pulse(), |el, delta| {
            el.opacity(0.5 * pulse_opacity(delta))
          }),
      )
  }

  fn checks(&self, checks: &Checks, p: &Palette) -> Button {
    let icon = match checks.status.as_deref() {
      Some("passed") => ui::icon(theme::ICON_CIRCLE_CHECK, 14., ui::Tone::Success.color(p)),
      Some("failed") => ui::icon(theme::ICON_CIRCLE_X, 14., ui::Tone::Danger.color(p)),
      Some(_) => ui::icon(theme::ICON_SPINNER, 14., p.muted_foreground),
      None => ui::icon(theme::ICON_CIRCLE_DOT, 14., p.muted_foreground),
    };
    let running = checks
      .status
      .as_deref()
      .is_some_and(|s| s != "passed" && s != "failed");
    Button::new("workspace-checks", p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .gap_1p5()
      .px_2()
      .map(|b| {
        if running {
          b.child(ui::spin("workspace-checks-spin", icon))
        } else {
          b.child(icon)
        }
      })
      .child(
        div()
          .text_color(p.muted_foreground)
          .child(checks.count.clone()),
      )
      .aria_label(checks.name.clone())
      .on_click(self.press("workspace.checks"))
      .on_right_click(self.copy_link(self.pr_url().map(|url| format!("{url}/checks"))))
  }

  fn tab_row(&self, row: &TabRow, p: &Palette) -> Div {
    // Counts are neutral; a conflict shows once, as a danger dot.
    let pill = |count: u32, tone: Tone, id: &'static str| {
      div()
        .debug_selector(move || id.into())
        .flex()
        .items_center()
        .gap_1p5()
        .child(ui::Count::new(count as usize, p))
        .when(tone == Tone::Conflict, |el| {
          el.child(ui::Dot::new(ui::Tone::Danger, p))
        })
    };
    let mut tabs: Vec<(Tab, &'static str, &'static str, Option<Div>)> = vec![
      (Tab::Overview, theme::ICON_CODE, "Code", None),
      (
        Tab::Commits,
        theme::ICON_COMMIT,
        "Commits",
        row.commits.as_ref().map(|label| match label {
          CommitsLabel::Count { count, tone } => pill(*count, *tone, "commits-tab-count"),
          CommitsLabel::HomeConflict => div()
            .debug_selector(|| "commits-tab-conflict-icon".into())
            .child(ui::Dot::new(ui::Tone::Danger, p)),
        }),
      ),
      (
        Tab::Changes,
        theme::ICON_FILE_DIFF,
        "Changes",
        row
          .changes
          .as_ref()
          .map(|c| pill(c.count, c.tone, "review-change-count")),
      ),
    ];
    if row.logs {
      tabs.push((Tab::Logs, theme::ICON_DATABASE, "Logs", None));
    }
    let list = div()
      .flex()
      // At a large zoom the tabs wrap inside their pill rather than run past
      // the window's edge.
      .flex_wrap()
      .gap_1()
      .p_1()
      .rounded(radius::MD)
      .bg(p.muted)
      .children(tabs.into_iter().map(|(tab, icon, label, extra)| {
        let active = tab == row.tab;
        let id = SharedString::from(format!("workspace-tab-{}", tab.value()));
        let selector = id.to_string();
        let sink = self.sink.clone();
        let color = if active {
          p.foreground
        } else {
          p.muted_foreground
        };
        ui::text(div(), TextSize::Sm)
          .id(id)
          .debug_selector(move || selector)
          .role(gpui::Role::Tab)
          .aria_label(label)
          .aria_selected(active)
          .test_support()
          .flex()
          .items_center()
          .gap_1p5()
          .px_3()
          .py_1()
          .rounded(radius::MD)
          .font_weight(FontWeight::MEDIUM)
          .cursor_pointer()
          .text_color(color)
          .map(|this| {
            if active {
              this.bg(p.background).shadow_sm()
            } else {
              this.hover(|s| s.bg(p.background.opacity(0.5)).text_color(p.foreground))
            }
          })
          .child(ui::icon(icon, 16., color))
          .child(label)
          .children(extra)
          .on_click(move |_, _, cx| {
            sink(
              ScreenAction::Field {
                field: "workspace.tab".into(),
                value: tab.value().into(),
              },
              cx,
            )
          })
      }));

    let mut status = div().flex().flex_none().items_center().gap_3();
    if let Some(busy) = &row.busy {
      status = status.child(
        ui::text(div(), TextSize::Sm)
          .flex()
          .items_center()
          .gap_2()
          .text_color(p.muted_foreground)
          .child(ui::spinner("workspace-busy-spin", 16., p.muted_foreground))
          .child(busy.clone()),
      );
    }
    if let Some(loc) = &row.loc {
      status = status.child(loc_marker(loc, p));
    }

    div()
      .flex_none()
      .flex()
      .items_center()
      .gap_3()
      .px_4()
      .py_2()
      .bg(p.background)
      .border_b_1()
      .border_color(p.border)
      .debug_selector(|| "workspace-tab-row".into())
      .child(div().flex().min_w_0().items_center().gap_2().child(list))
      .child(div().flex_1().min_w_0())
      .child(status)
  }
}

/// Tailwind's `animate-pulse`: 2s, opacity dipping to half midway.
fn pulse() -> Animation {
  Animation::new(Duration::from_secs(2)).repeat()
}

fn pulse_opacity(delta: f32) -> f32 {
  1. - 0.5 * (1. - (2. * delta - 1.).abs())
}

/// `LocDiffMarker`: green `+N`, a centered insertion/deletion bar, red `-N`.
fn loc_marker(loc: &Loc, p: &Palette) -> Div {
  const HALF: f32 = 14.;
  let scale = loc.insertions.max(loc.deletions) as f32;
  let width = |n: u32| {
    if n == 0 || scale == 0. {
      0.
    } else {
      ((n as f32 / scale) * HALF).round().max(1.)
    }
  };
  let (added, removed) = (ui::Tone::GitOpen, ui::Tone::GitClosed);
  let (green, red) = (added.color(p), removed.color(p));
  let count = |text: String, color: Hsla| {
    ui::text(div(), TextSize::Xs)
      .font_family(theme::MONO_FONT)
      .text_color(color)
      .child(text)
  };
  div()
    .debug_selector(|| "loc-diff-marker".into())
    .flex()
    .items_center()
    .gap_1p5()
    .child(count(format!("+{}", loc.insertions), added.text(p)))
    .child(
      div()
        .flex()
        .flex_none()
        .items_center()
        .h(zpx(6.))
        .child(
          div()
            .flex()
            .justify_end()
            .h_full()
            .w(zpx(HALF))
            .child(div().h_full().w(zpx(width(loc.insertions))).bg(green)),
        )
        .child(div().w_px().h_full().bg(p.border))
        .child(
          div()
            .flex()
            .h_full()
            .w(zpx(HALF))
            .child(div().h_full().w(zpx(width(loc.deletions))).bg(red)),
        ),
    )
    .child(count(format!("-{}", loc.deletions), removed.text(p)))
}

/// The tabs' actions that start an agent session, which the view handles
/// itself ([`WorkspaceView::launch`]).
fn launches(action: &ScreenAction) -> bool {
  matches!(action, ScreenAction::Field { field, .. } if matches!(
    field.as_str(),
    "changes.startReview"
      | "changes.review"
      | "fileBrowser.review"
      | "changes.agentComment"
      | "logs.sendToAgent"
  ))
}

impl WorkspaceView {
  /// `ShowWorkspace`'s agent starts: a review, a finished review or a
  /// review comment sent to an agent, and log lines. A review or comment
  /// goes to the workspace's idle agent when asked and there is one.
  fn launch(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    let ScreenAction::Field { field, value } = action else {
      return;
    };
    let repo = self.route.repo_path.clone();
    let workspace_id = self.route.workspace_id;
    let request = {
      let repo = repo.clone();
      move |name: &str, prompt: String, mode: &str, agent: Option<String>| LaunchRequest {
        repo_path: repo,
        workspace_id,
        prompt: Some(prompt),
        mode: Some(mode.into()),
        agent,
        session_name: Some(name.into()),
      }
    };
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_string();
    match field.as_str() {
      // A reviewer that inherits an idle agent's context would review its
      // own work, so a review always starts a new terminal.
      "changes.startReview" => {
        let agent = value["agent"].as_str().map(str::to_string);
        let prompt = text("prompt");
        self.start(cx, None, "Failed to create agent", move |host| {
          let agent = host.resolve_agent(&repo, agent.as_deref());
          request("AI Review", prompt, "acceptEdits", agent)
        });
      }
      "changes.review" | "fileBrowser.review" => {
        let (markdown, mode) = (text("markdown"), text("mode"));
        if value["useIdleAgent"].as_bool() == Some(true) {
          match self.send_to_idle(&markdown, cx) {
            None => return (self.react)(ScreenAction::Field { field, value }, cx),
            Some(true) => return self.toast(ToastTone::Success, "Review sent to agent", ""),
            Some(false) => {}
          }
        }
        self.start(cx, None, "Failed to create agent", move |host| {
          let agent = host.resolve_agent(&repo, None);
          request("Code Review", markdown, &mode, agent)
        });
      }
      "changes.agentComment" => {
        let Ok(comment) = serde_json::from_value(value.clone()) else {
          return;
        };
        let dir = self
          .loaded
          .as_ref()
          .and_then(|loaded| loaded.workspace.as_ref())
          .map(super::merge::full_path)
          .unwrap_or_else(|| self.route.repo_path.clone());
        let prompt = crate::core::agent_launch::review_comment_prompt(&comment, &dir);
        match self.send_to_idle(&prompt, cx) {
          None => return (self.react)(ScreenAction::Field { field, value }, cx),
          Some(true) => return self.toast(ToastTone::Success, "Comment sent to agent", ""),
          Some(false) => {}
        }
        let done = ("Comment sent to agent", "");
        self.start(cx, Some(done), "Failed to create agent", move |_| {
          request("Review Comment", prompt, "acceptEdits", None)
        });
      }
      "logs.sendToAgent" => {
        let prompt = value.as_str().unwrap_or_default().to_string();
        self.start(cx, None, "Failed to send logs to agent", move |_| {
          request("Logs", prompt, "plan", None)
        });
      }
      _ => (self.react)(ScreenAction::Field { field, value }, cx),
    }
  }

  /// Sends `text` to the shown workspace's idle agent: `None` while React
  /// draws the terminal pane, which knows its own agents.
  fn send_to_idle(&mut self, text: &str, cx: &mut Context<Self>) -> Option<bool> {
    let (pane, _) = self.terminal.as_ref()?;
    let branch = self.snapshot_branch();
    let repo = self.route.repo_path.clone();
    Some(branch.is_some_and(|branch| {
      pane.update(cx, |pane, cx| {
        pane.send_to_idle_agent(&repo, &branch, text, cx)
      })
    }))
  }

  /// The shown workspace's branch; `None` for the home repository.
  fn snapshot_branch(&self) -> Option<String> {
    let id = self.route.workspace_id?;
    super::code_data::CodeHost::workspaces(&*self.code_host, &self.route.repo_path)
      .ok()?
      .into_iter()
      .find(|w| w.id == id)
      .map(|w| w.branch_name)
  }

  /// Starts the session `request` builds, off the main thread; toasts
  /// `done` once it starts, or `failed` with the error.
  fn start(
    &self,
    cx: &mut Context<Self>,
    done: Option<(&'static str, &'static str)>,
    failed: &'static str,
    request: impl FnOnce(&dyn TerminalHost) -> LaunchRequest + Send + 'static,
  ) {
    let host = self.code_host.clone();
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move {
          let request = request(host.as_ref());
          host.launch_session(&request, false)
        })
        .await;
      this
        .update(cx, |view, _| match (result, done) {
          (Ok(_), Some((title, description))) => view.toast(ToastTone::Success, title, description),
          (Ok(_), None) => {}
          (Err(err), _) => view.toast(ToastTone::Error, failed, err),
        })
        .ignore_closed();
    })
    .detach();
  }

  fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    self.host.notify(NativeEvent::Toast {
      title: title.into(),
      description: description.into(),
      tone,
      action: None,
    });
  }
}

impl WorkspaceView {
  /// Gives up the terminal pane, so its terminals outlive this view while
  /// another screen (Settings, say) shows.
  pub fn take_terminal(&mut self, cx: &mut Context<Self>) -> Option<Entity<TerminalPane>> {
    let (pane, _) = self.terminal.take()?;
    pane.update(cx, |pane, _| pane.park());
    Some(pane)
  }

  /// Draws `pane`, which an earlier workspace view gave up, placing it in
  /// the shown workspace when it next draws.
  pub fn adopt_terminal(&mut self, pane: Entity<TerminalPane>, cx: &mut Context<Self>) {
    let react = self.children.clone();
    pane.update(cx, |pane, _| pane.set_react(react));
    self.terminal = Some((pane, TerminalPlace::default()));
  }

  /// The terminal pane, placed in the shown workspace, while the route
  /// asks GPUI to draw it.
  fn sync_terminal(&mut self, cx: &mut Context<Self>) -> Option<Entity<TerminalPane>> {
    let Some(route) = self.route.terminal.clone() else {
      self.terminal = None;
      return None;
    };
    let key = (
      self.route.repo_path.clone(),
      self.route.workspace_id,
      self.route.revision,
    );
    let pane = match &self.terminal {
      Some((pane, placed)) if *placed == key => pane.clone(),
      _ => {
        let pane = match &self.terminal {
          Some((pane, _)) => pane.clone(),
          None => {
            let (host, react, popups) = (
              self.code_host.clone(),
              self.children.clone(),
              self.popups.clone(),
            );
            // The repository's root until its workspaces are read.
            let place = Place {
              repo_path: self.route.repo_path.clone(),
              working_directory: self.route.repo_path.clone(),
              current_branch: self.route.home_branch.clone(),
              ..Place::default()
            };
            cx.new(|cx| TerminalPane::new(host, react, popups, place, cx))
          }
        };
        self.terminal = Some((pane.clone(), key));
        self.place_terminal(pane.downgrade(), cx);
        pane
      }
    };
    pane.update(cx, |pane, cx| pane.set_route(route, cx));
    Some(pane)
  }

  /// Places the pane in the shown workspace once its repository's
  /// workspaces are read, off the main thread: a remote repository's go
  /// over SSH.
  fn place_terminal(&mut self, pane: WeakEntity<TerminalPane>, cx: &mut Context<Self>) {
    let (host, repo, workspace_id, home) = (
      self.code_host.clone(),
      self.route.repo_path.clone(),
      self.route.workspace_id,
      self.route.home_branch.clone(),
    );
    self._place = cx.spawn(async move |_, cx| {
      let place = cx
        .background_spawn(async move { terminal_place(&*host, repo, workspace_id, home) })
        .await;
      pane
        .update(cx, |pane, cx| pane.set_place(place, cx))
        .ignore_closed();
    });
  }

  /// The tab GPUI draws (Code or Commits), kept in step with the header.
  fn tab_body(&mut self, cx: &mut Context<Self>) -> Option<Body> {
    let tab = self.route.tab;
    if !matches!(tab, Tab::Overview | Tab::Commits | Tab::Changes | Tab::Logs) {
      self.code = None;
      self.file_browser = None;
      self.commits = None;
      self.changes = None;
      self.logs = None;
      return None;
    }
    // Another workspace is loading: keep the tab drawn as it was.
    let Some(loaded) = self.loaded.as_ref() else {
      return self.kept_body();
    };
    let parent = Parent {
      repo_path: self.route.repo_path.clone(),
      workspace: loaded.workspace.clone(),
      conflicted_files: loaded
        .status
        .as_ref()
        .map(|s| s.conflicted_files.clone())
        .unwrap_or_default(),
      default_branch: self.route.default_target().to_string(),
      revision: self.route.revision,
      remote: self.route.remote_chrome.is_some(),
    };
    let dark = self.dark;
    let (host, react, popups) = (
      self.code_host.clone(),
      self.children.clone(),
      self.popups.clone(),
    );
    if tab == Tab::Logs {
      self.code_shown = false;
      self.file_browser = None;
      self.commits = None;
      self.changes = None;
      let key = self.route.logs_key;
      if let Some((logs, started)) = &self.logs {
        if *started == key {
          let repo = self.route.repo_path.clone();
          logs.update(cx, |view, cx| view.set_repo(repo, dark, cx));
          return Some(Body::Logs(logs.clone()));
        }
      }
      let host: Arc<dyn LogsHost> = host;
      let (repo, source) = (self.route.repo_path.clone(), self.route.logs_source.clone());
      let logs = cx.new(|cx| LogsView::new(repo, source.as_deref(), host, react, popups, dark, cx));
      self.logs = Some((logs.clone(), key));
      return Some(Body::Logs(logs));
    }
    self.logs = None;
    if tab == Tab::Changes {
      self.code_shown = false;
      self.file_browser = None;
      self.commits = None;
      let options = changes::Options {
        show_committed: self.route.show_committed,
        move_targets: self.route.move_targets.clone(),
        revision: self.route.changes_revision,
      };
      if let Some(changes) = &self.changes {
        changes.update(cx, |view, cx| view.set_parent(parent, options, dark, cx));
        return Some(Body::Changes(changes.clone()));
      }
      let changes = cx.new(|cx| ChangesView::new(parent, options, host, react, popups, dark, cx));
      self.changes = Some(changes.clone());
      return Some(Body::Changes(changes));
    }
    self.changes = None;
    let host: Arc<dyn CommitsHost> = host;
    if tab == Tab::Commits {
      self.code_shown = false;
      self.file_browser = None;
      let scroll_to = self.route.scroll_to_commit.clone();
      if let Some(commits) = &self.commits {
        commits.update(cx, |view, cx| view.set_parent(parent, scroll_to, dark, cx));
        return Some(Body::Commits(commits.clone()));
      }
      let commits = cx.new(|cx| CommitsView::new(parent, scroll_to, host, react, popups, dark, cx));
      self.commits = Some(commits.clone());
      return Some(Body::Commits(commits));
    }
    self.commits = None;
    if let Some(route) = self.route.file_browser.clone() {
      self.code_shown = false;
      let opening = Opening {
        route,
        revision: self.route.changes_revision,
      };
      if let Some(browser) = &self.file_browser {
        browser.update(cx, |view, cx| view.set_parent(parent, opening, dark, cx));
        return Some(Body::FileBrowser(browser.clone()));
      }
      let host: Arc<dyn FileBrowserHost> = self.code_host.clone();
      let browser =
        cx.new(|cx| FileBrowserView::new(parent, opening, host, react, popups, dark, cx));
      self.file_browser = Some(browser.clone());
      return Some(Body::FileBrowser(browser));
    }
    self.file_browser = None;
    // Back on the Code tab: draw what it last read and read it again behind.
    let returning = !std::mem::replace(&mut self.code_shown, true);
    if let Some(code) = &self.code {
      code.update(cx, |code, cx| {
        code.set_parent(parent, dark, cx);
        if returning {
          code.revalidate(cx);
        }
      });
      return Some(Body::Code(code.clone()));
    }
    let code = cx.new(|cx| CodeView::new(parent, host, react, popups, dark, cx));
    self.code = Some(code.clone());
    Some(Body::Code(code))
  }

  /// The tab's view as last drawn, while the route's workspace loads.
  fn kept_body(&self) -> Option<Body> {
    match self.route.tab {
      Tab::Logs => self.logs.as_ref().map(|(logs, _)| Body::Logs(logs.clone())),
      Tab::Changes => self.changes.clone().map(Body::Changes),
      Tab::Commits => self.commits.clone().map(Body::Commits),
      _ if self.route.file_browser.is_some() => self.file_browser.clone().map(Body::FileBrowser),
      _ => self.code.clone().map(Body::Code),
    }
  }
}

/// The tab drawn above the terminal pane.
enum Body {
  Code(Entity<CodeView>),
  FileBrowser(Entity<FileBrowserView>),
  Commits(Entity<CommitsView>),
  Changes(Entity<ChangesView>),
  Logs(Entity<LogsView>),
}

impl Body {
  fn into_any_element(self) -> gpui::AnyElement {
    match self {
      Body::Code(code) => code.into_any_element(),
      Body::FileBrowser(browser) => browser.into_any_element(),
      Body::Commits(commits) => commits.into_any_element(),
      Body::Changes(changes) => changes.into_any_element(),
      Body::Logs(logs) => logs.into_any_element(),
    }
  }
}

impl Render for WorkspaceView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.window = Some(window.window_handle());
    self.sync_tooltip_popup(cx);
    let p = theme::palette(self.dark);
    let confirm = crate::gpui_shell::confirm::render(Self::confirm_slot, self, window, &p, cx);
    let snapshot = self.snapshot.clone();
    let header = match &snapshot {
      Some(snapshot) => self.header(&snapshot.header, &p, cx),
      // Until the first load: the empty bar, as tall as a row of buttons.
      None => div()
        .flex_none()
        .h(zpx(49.))
        .border_b_1()
        .border_color(p.border)
        .debug_selector(|| "workspace-header".into()),
    };
    let tab_row = snapshot
      .and_then(|snapshot| snapshot.tab_row)
      .map(|row| self.tab_row(&row, &p));
    let code = self.tab_body(cx);
    let remote = self.route.remote_chrome.clone();
    let cut_off = remote.as_ref().is_some_and(|r| r.cut_off);
    let strip = remote.map(|chrome| {
      let sink = self.sink.clone();
      let press: super::remote_chrome::Press = Rc::new(move |id| {
        let sink = sink.clone();
        Box::new(move |_, _, cx| sink(ScreenAction::Press { id: id.to_string() }, cx))
      });
      super::remote_chrome::strip(&chrome, self.dark, &p, &press)
    });
    let body = match (code, self.sync_terminal(cx)) {
      // The tab with the terminal pane below: a share of the whole view,
      // header and tabs included. A maximized pane covers the tabs and
      // keeps the header.
      (code, Some(pane)) => {
        pane.update(cx, |pane, cx| pane.set_dark(self.dark, cx));
        let covered = pane.read(cx).covers();
        let container = pane.read(cx).container.clone();
        let content = match code {
          Some(code) => code.into_any_element(),
          None => div().size_full().into_any_element(),
        };
        let pane_area = div()
          .relative()
          .flex_1()
          .min_h_0()
          .flex()
          .flex_col()
          .child(
            gpui::canvas(
              move |bounds, _, _| container.set(Some(bounds)),
              |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
          )
          .map(|this| {
            if covered {
              return this.child(header);
            }
            this.child(
              div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(header)
                .children(tab_row)
                .child(div().flex_1().min_h_0().child(content)),
            )
          })
          .child(pane)
          .when(cut_off, |d| {
            d.child(super::remote_chrome::cutoff_overlay(&p))
          });
        return div()
          .size_full()
          .flex()
          .flex_col()
          .bg(p.background)
          .text_color(p.foreground)
          .children(strip)
          .child(pane_area)
          .children(confirm)
          .into_any_element();
      }
      // The tab alone, while no terminal pane shows.
      (Some(code), None) => div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(div().flex_1().min_h_0().child(code.into_any_element())),
      (None, None) => div().flex_1().min_h_0(),
    };
    div()
      .size_full()
      .flex()
      .flex_col()
      .bg(p.background)
      .text_color(p.foreground)
      .children(strip)
      .child(
        div()
          .relative()
          .flex_1()
          .min_h_0()
          .flex()
          .flex_col()
          .child(header)
          .children(tab_row)
          .child(body)
          .when(cut_off, |d| {
            d.child(super::remote_chrome::cutoff_overlay(&p))
          }),
      )
      .children(confirm)
      .into_any_element()
  }
}

/// Where the terminal pane runs for workspace `workspace_id` of `repo`.
fn terminal_place(
  host: &dyn TerminalHost,
  repo: String,
  workspace_id: Option<i64>,
  home_branch: String,
) -> Place {
  let workspaces = super::code_data::CodeHost::workspaces(host, &repo).unwrap_or_default();
  let selected = workspace_id.and_then(|id| workspaces.iter().find(|w| w.id == id));
  Place {
    working_directory: selected
      .map(super::merge::full_path)
      .unwrap_or_else(|| repo.clone()),
    current_branch: home_branch,
    branches: workspaces
      .iter()
      .map(|w| (super::merge::full_path(w), w.branch_name.clone()))
      .collect(),
    workspace_ids: workspaces
      .iter()
      .map(|w| (super::merge::full_path(w), w.id))
      .collect(),
    repo_path: repo,
  }
}
