//! The workspace view's header and tab row (React's `ShowWorkspace`), with
//! the webview framed below them.
//!
//! The header reads its data from the backend ([`workspace_data`]) and
//! pushes, syncs and rebases the home branch itself. React says which
//! workspace shows, and draws the tab's content in the frame, leaving its own
//! header and tab row out of it (its "framed" layout). Popovers (target
//! branch, checks, details) and dialogs are still React's: pressing one asks
//! React to open it, and React takes the whole webview back while it is
//! open. Menus are GPUI's, opened by React below the button that asked for
//! them.
//!
//! [`workspace_data`]: super::workspace_data

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use serde::Deserialize;
use tauri::{Webview, Wry};
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, px, rgb, Animation, AnimationExt as _, App, Bounds, Context, Div, Entity,
  FontWeight, Hsla, Pixels, SharedString, Stateful, Task, Window,
};

use super::{
  code::{CodeView, Parent},
  commits::CommitsView,
  commits_data::CommitsHost,
  workspace_data::{
    workspace_view, Loaded, Outcome, Pending, PrCache, Route, WorkspaceHost, DEFAULT_BRANCH,
  },
  ActionSink, ScreenAction,
};
use crate::gpui_shell::{
  native::{NativeEvent, ToastTone},
  popup::{self, PopupRequest, PopupSink, Side, TooltipContent},
  theme::{self, Palette},
  ui::{self, Button, Size, TextSize, Variant},
};

/// React's base-ui tooltips open after 200ms (`TooltipProvider delay={200}`).
const TOOLTIP_DELAY: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
  pub header: Header,
  /// None: React draws the tab row in the frame.
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
  pub title: Option<String>,
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
  pub treq: Option<String>,
  pub github: Option<GithubChecks>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GithubChecks {
  pub label: String,
  /// `PrCiStatus.state`: "success", "failure" or "pending".
  pub state: String,
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

/// Tailwind colors the header uses outside the theme tokens.
mod tw {
  use super::*;

  pub fn color(hex: u32) -> Hsla {
    rgb(hex).into()
  }

  pub const GREEN_400: u32 = 0x4ade80;
  pub const GREEN_500: u32 = 0x22c55e;
  pub const GREEN_600: u32 = 0x16a34a;
  pub const GREEN_700: u32 = 0x15803d;
  pub const RED_400: u32 = 0xf87171;
  pub const RED_500: u32 = 0xef4444;
  pub const RED_600: u32 = 0xdc2626;
  pub const RED_700: u32 = 0xb91c1c;
  pub const PURPLE_400: u32 = 0xc084fc;
  pub const PURPLE_500: u32 = 0xa855f7;
  pub const PURPLE_600: u32 = 0x9333ea;
  pub const PURPLE_700: u32 = 0x7e22ce;
  pub const YELLOW_400: u32 = 0xfacc15;
  pub const YELLOW_500: u32 = 0xeab308;
  pub const YELLOW_600: u32 = 0xca8a04;
  pub const YELLOW_700: u32 = 0xa16207;
  pub const YELLOW_950: u32 = 0x422006;
  pub const BLUE_600: u32 = 0x2563eb;
  pub const BLUE_700: u32 = 0x1d4ed8;
  /// GitHub's button: `bg-[#24292f] hover:bg-[#1b1f23]`.
  pub const GITHUB: u32 = 0x24292f;
  pub const GITHUB_HOVER: u32 = 0x1b1f23;

  /// `text-x-700 dark:text-x-400`, `border-x-600 dark:border-x-500`.
  pub fn state(dark: bool, [c400, c500, c600, c700]: [u32; 4]) -> (Hsla, Hsla, Hsla) {
    let text = color(if dark { c400 } else { c700 });
    let border = color(if dark { c500 } else { c600 });
    (text, border, color(c600))
  }

  pub const GREEN: [u32; 4] = [GREEN_400, GREEN_500, GREEN_600, GREEN_700];
  pub const RED: [u32; 4] = [RED_400, RED_500, RED_600, RED_700];
  pub const PURPLE: [u32; 4] = [PURPLE_400, PURPLE_500, PURPLE_600, PURPLE_700];
  pub const YELLOW: [u32; 4] = [YELLOW_400, YELLOW_500, YELLOW_600, YELLOW_700];
}

/// How often the branch's pull request is re-read from the PR poller's
/// cache, which the poller itself refreshes.
const PR_POLL: Duration = Duration::from_secs(5);

/// The header and tab row, drawn from what they read from the backend.
pub struct WorkspaceView {
  host: Arc<dyn WorkspaceHost>,
  code_host: Arc<dyn CommitsHost>,
  /// The Code tab, while GPUI draws it.
  pub(super) code: Option<Entity<CodeView>>,
  /// The Commits tab, while GPUI draws it.
  pub(super) commits: Option<Entity<CommitsView>>,
  /// The tab's height last sent to React (`workspace.contentHeight`).
  content_height: Rc<std::cell::Cell<f32>>,
  /// React, for what the header does not do itself.
  react: ActionSink,
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
  webview: Option<Webview<Wry>>,
  /// The element whose tooltip is pending or shown (`true`).
  tooltip: Option<(SharedString, bool)>,
  /// The tooltip whose popup is open.
  tooltip_sent: Rc<RefCell<Option<SharedString>>>,
  /// Where dropdown buttons were last drawn, in window coordinates.
  bounds: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
  _poll: Task<()>,
}

impl WorkspaceView {
  pub fn new(
    route: Route,
    host: Arc<dyn CommitsHost>,
    react: ActionSink,
    dark: bool,
    popups: PopupSink,
    webview: Option<Webview<Wry>>,
    cx: &mut Context<Self>,
  ) -> Self {
    let view = cx.weak_entity();
    let sink: ActionSink = Rc::new(move |action, cx| {
      let view = view.clone();
      cx.defer(move |cx| {
        let _ = view.update(cx, |view, cx| view.handle(action, cx));
      });
    });
    let poll = cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(PR_POLL).await;
      if this.update(cx, |view, cx| view.read_pr(cx)).is_err() {
        break;
      }
    });
    let mut this = Self {
      host: host.clone(),
      code_host: host,
      code: None,
      commits: None,
      content_height: Rc::default(),
      react,
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
      webview,
      tooltip: None,
      tooltip_sent: Rc::default(),
      bounds: Rc::default(),
      _poll: poll,
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
      self.loaded = None;
      self.pr = PrCache::default();
      self.pending = Pending::None;
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
      let _ = this.update(cx, |view, cx| {
        view.loading = false;
        if view.route.key() == key {
          view.loaded = Some(loaded);
          view.read_pr(cx);
          view.rebuild(cx);
        }
        if std::mem::take(&mut view.reload) {
          view.load(cx);
        }
      });
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
      let _ = this.update(cx, |view, cx| {
        if view.route.key() == key && view.pr != pr {
          view.pr = pr;
          view.rebuild(cx);
        }
      });
    })
    .detach();
  }

  fn rebuild(&mut self, cx: &mut Context<Self>) {
    let snapshot = self.loaded.as_ref().map(|loaded| {
      let (header, tab_row) = workspace_view(&self.route, loaded, &self.pr, self.pending);
      Workspace { header, tab_row }
    });
    if self.snapshot != snapshot {
      self.snapshot = snapshot;
      cx.notify();
    }
  }

  pub(super) fn handle(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
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
      let _ = this.update(cx, |view, cx| {
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
      });
    })
    .detach();
  }

  fn press(&self, id: &'static str) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) {
    let sink = self.sink.clone();
    move |_, _, cx| sink(ScreenAction::Press { id: id.to_string() }, cx)
  }

  /// A press that React answers with a menu, opened below the button.
  fn press_dropdown(
    &self,
    key: &'static str,
    id: &'static str,
  ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) {
    let bounds = self.bounds.clone();
    let press = self.press(id);
    move |event, window, cx| {
      if let Some(anchor) = bounds.borrow().get(key).copied() {
        popup::set_dropdown_anchor(anchor);
      }
      press(event, window, cx);
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
    self.tooltip = Some((key.clone(), false));
    cx.spawn(async move |this, cx| {
      cx.background_executor().timer(TOOLTIP_DELAY).await;
      let _ = this.update(cx, |this, cx| {
        if let Some((k, shown)) = this.tooltip.as_mut() {
          if *k == key && !*shown {
            *shown = true;
            cx.notify();
          }
        }
      });
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
    let dark = self.dark;
    let mono_title = |text: &str| {
      ui::text(div(), TextSize::Sm)
        .font_family(theme::MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .min_w_0()
        .max_w(px(220.))
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
            .py(px(2.))
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
          .variant(Variant::Outline)
          .size(Size::Sm)
          .disabled(target.disabled)
          .max_w(px(220.))
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
          .child(button);
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
              .max_w(px(220.))
              .truncate()
              .child(home_target.branch.clone()),
          );
        if let Some(rebase) = &home_target.rebase {
          if let Some(files) = &rebase.conflicts {
            let warning = tw::color(if dark { tw::YELLOW_400 } else { tw::YELLOW_600 });
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

    // Not shrunk: like CSS's `min-width: auto`, the row wraps it instead.
    let mut right = div().flex().flex_none().items_center().gap_2().ml_auto();
    if let Some(push) = &h.push {
      let blue = tw::color(tw::BLUE_600);
      let hover = tw::color(tw::BLUE_700);
      let button = Button::new("workspace-push", p)
        .size(Size::Sm)
        .disabled(push.disabled)
        .bg(blue)
        .hover_bg(hover)
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
          .h(px(24.))
          .w(px(56.))
          .rounded(px(4.))
          .bg(p.muted.opacity(0.6))
          .with_animation("workspace-status-pulse", pulse(), |el, delta| {
            el.opacity(pulse_opacity(delta))
          }),
      );
    }
    if let Some(sync) = &h.sync {
      right = right.child(self.sync(sync, p, cx));
    }
    if let Some(checks) = &h.checks {
      right = right.child(self.checks(checks, p));
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
      let green = tw::color(tw::GREEN_600);
      let hover = tw::color(tw::GREEN_700);
      let button = Button::new("workspace-merge", p)
        .variant(if merge.passing {
          Variant::Default
        } else {
          Variant::Outline
        })
        .size(Size::Sm)
        .gap_1()
        .disabled(merge.disabled)
        .when(merge.passing, |b| {
          b.bg(green).text_color(gpui::white()).hover_bg(hover)
        })
        .map(|b| {
          let fg = if merge.passing {
            gpui::white()
          } else {
            b.foreground()
          };
          b.child(ui::icon(theme::ICON_COMPARE, 16., fg))
            .child("Merge...")
        })
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
    if h.details {
      right = right.child(
        Button::new("workspace-details", p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .gap_1()
          .px_2()
          .icon(theme::ICON_INFO, 16.)
          .label("Details")
          .on_click(self.press("workspace.details")),
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
              .on_click(self.press_dropdown("workspace-more", "workspace.more")),
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
      .when_some(h.title.clone(), |this, title| {
        this.child(
          div().flex().items_center().px_1().mt_2().child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::MEDIUM)
              .text_color(p.foreground)
              .child(title),
          ),
        )
      })
  }

  fn view_pr(&self, pr: &ViewPr, p: &Palette, cx: &mut Context<Self>) -> Div {
    let colors = match pr.state {
      PrState::Open => tw::GREEN,
      PrState::Closed => tw::RED,
      PrState::Merged => tw::PURPLE,
    };
    let (text, border, hover) = tw::state(self.dark, colors);
    let icon = if pr.draft {
      theme::ICON_PR_DRAFT
    } else {
      match pr.state {
        PrState::Open => theme::ICON_PR,
        PrState::Closed => theme::ICON_PR_CLOSED,
        PrState::Merged => theme::ICON_MERGE,
      }
    };
    let styled = |b: Button| {
      b.variant(Variant::Outline)
        .size(Size::Sm)
        .border_color(border)
        .text_color(text)
        .hover_bg(hover.opacity(0.1))
    };
    let view = styled(Button::new("workspace-view-pr", p))
      .gap_1()
      .rounded_r_none()
      .child(ui::icon(icon, 16., text))
      .child("View PR")
      .on_click(self.press("workspace.viewPr"));
    let web = styled(Button::new("workspace-view-pr-web", p))
      .rounded_l_none()
      .border_l_0()
      .px_2()
      .child(ui::icon(theme::ICON_EXTERNAL_LINK, 16., text))
      .on_click(self.press("workspace.viewPrWeb"));
    div()
      .flex()
      .flex_none()
      .items_center()
      .child(self.with_tooltip(
        "workspace-view-pr",
        Some(TooltipContent::Text(
          format!("View pull request #{} in Treq", pr.number).into(),
        )),
        view,
        cx,
      ))
      .child(self.with_tooltip(
        "workspace-view-pr-web",
        Some(TooltipContent::Text(
          format!("Open pull request #{} on GitHub", pr.number).into(),
        )),
        web,
        cx,
      ))
  }

  fn create_pr(&self, create: &CreatePr, p: &Palette, cx: &mut Context<Self>) -> Div {
    let bg = tw::color(tw::GITHUB);
    let hover = tw::color(tw::GITHUB_HOVER);
    let white = gpui::white();
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
      .child(create.label.clone())
      .on_click(self.press("workspace.createPr"));
    let menu = styled(Button::new("workspace-create-pr-menu", p))
      .rounded_l_none()
      .border_l_1()
      .border_color(white.opacity(if self.dark { 0.3 } else { 0.2 }))
      .px(px(6.))
      .child(ui::icon(theme::ICON_CHEVRON_DOWN, 16., white))
      .on_click(self.press_dropdown("workspace-create-pr-menu", "workspace.createPrMenu"));
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
    let dot = tw::color(tw::YELLOW_600);
    let button = Button::new("workspace-sync", p)
      .variant(if sync.ahead > 0 {
        Variant::Outline
      } else {
        Variant::Ghost
      })
      .size(Size::Sm)
      .h(px(24.))
      .gap_1()
      .px_2()
      .text_size(px(11.))
      .line_height(px(16.))
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
          .top(px(-2.))
          .right(px(-2.))
          .size(px(8.))
          .rounded_full()
          .bg(dot)
          .with_animation("workspace-sync-pulse", pulse(), |el, delta| {
            el.opacity(0.5 * pulse_opacity(delta))
          }),
      )
  }

  fn checks(&self, checks: &Checks, p: &Palette) -> Button {
    let icon = match checks.status.as_deref() {
      Some("passed") => ui::icon(theme::ICON_CIRCLE_CHECK, 14., tw::color(tw::GREEN_500)),
      Some("failed") => ui::icon(theme::ICON_CIRCLE_X, 14., tw::color(tw::RED_500)),
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
      .gap(px(6.))
      .px_2()
      .map(|b| {
        if running {
          b.child(ui::spin("workspace-checks-spin", icon))
        } else {
          b.child(icon)
        }
      })
      .child("Checks")
      .when_some(checks.treq.clone(), |b, treq| {
        b.child(div().text_color(p.muted_foreground).child(treq))
      })
      .when_some(checks.github.clone(), |b, gh| {
        let colors = match gh.state.as_str() {
          "success" => tw::GREEN,
          "failure" => tw::RED,
          _ => tw::YELLOW,
        };
        let (text, _, _) = tw::state(self.dark, colors);
        b.child(
          div()
            .border_l_1()
            .border_color(p.border)
            .pl(px(6.))
            .text_color(text)
            .child(gh.label),
        )
      })
      .on_click(self.press("workspace.checks"))
  }

  fn tab_row(&self, row: &TabRow, p: &Palette) -> Div {
    let dark = self.dark;
    let pill = |count: u32, tone: Tone, id: &'static str| {
      let (bg, fg) = match tone {
        Tone::Muted => (p.muted, p.muted_foreground),
        Tone::Conflict => (p.destructive.opacity(0.2), p.destructive),
        Tone::Uncommitted => (tw::color(tw::YELLOW_500), tw::color(tw::YELLOW_950)),
      };
      ui::text(div(), TextSize::Xs)
        .debug_selector(move || id.into())
        .rounded_full()
        .px_2()
        .py(px(2.))
        .font_weight(FontWeight::MEDIUM)
        .bg(bg)
        .text_color(fg)
        .child(count.to_string())
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
            .child(ui::icon(theme::ICON_TRIANGLE_ALERT, 14., p.destructive)),
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
      .gap_1()
      .p_1()
      .rounded(px(8.))
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
          .flex()
          .items_center()
          .gap(px(6.))
          .px_3()
          .py_1()
          .rounded(px(8.))
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
      status = status.child(loc_marker(loc, p, dark));
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
      .child(div().flex().flex_none().items_center().gap_2().child(list))
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
fn loc_marker(loc: &Loc, p: &Palette, dark: bool) -> Div {
  const HALF: f32 = 14.;
  let scale = loc.insertions.max(loc.deletions) as f32;
  let width = |n: u32| {
    if n == 0 || scale == 0. {
      0.
    } else {
      ((n as f32 / scale) * HALF).round().max(1.)
    }
  };
  let green = tw::color(if dark { tw::GREEN_400 } else { tw::GREEN_600 });
  let red = tw::color(if dark { tw::RED_400 } else { tw::RED_600 });
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
    .gap(px(6.))
    .child(count(format!("+{}", loc.insertions), green))
    .child(
      div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(6.))
        .child(
          div()
            .flex()
            .justify_end()
            .h_full()
            .w(px(HALF))
            .child(div().h_full().w(px(width(loc.insertions))).bg(green)),
        )
        .child(div().w_px().h_full().bg(p.border))
        .child(
          div()
            .flex()
            .h_full()
            .w(px(HALF))
            .child(div().h_full().w(px(width(loc.deletions))).bg(red)),
        ),
    )
    .child(count(format!("-{}", loc.deletions), red))
}

/// The frame's height before React measured its terminal pane: the
/// collapsed pane's bar.
const FRAME_FALLBACK: f32 = 40.;

impl WorkspaceView {
  /// The tab GPUI draws (Code or Commits), kept in step with the header.
  fn tab_body(&mut self, cx: &mut Context<Self>) -> Option<Body> {
    let loaded = self.loaded.as_ref().filter(|_| self.route.code);
    let tab = self.route.tab;
    let Some(loaded) = loaded.filter(|_| matches!(tab, Tab::Overview | Tab::Commits)) else {
      self.code = None;
      self.commits = None;
      self.content_height.set(0.);
      return None;
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
    };
    let dark = self.dark;
    let (host, react, popups) = (
      self.code_host.clone(),
      self.react.clone(),
      self.popups.clone(),
    );
    if tab == Tab::Commits {
      self.code = None;
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
    if let Some(code) = &self.code {
      code.update(cx, |code, cx| code.set_parent(parent, dark, cx));
      return Some(Body::Code(code.clone()));
    }
    let code = cx.new(|cx| CodeView::new(parent, host, react, popups, dark, cx));
    self.code = Some(code.clone());
    Some(Body::Code(code))
  }
}

/// The tab drawn above the terminal pane.
enum Body {
  Code(Entity<CodeView>),
  Commits(Entity<CommitsView>),
}

impl Body {
  /// A dialog over the whole window would leave the webview on top.
  fn covers_window(&self, cx: &App) -> bool {
    match self {
      Body::Code(code) => code.read(cx).covers_window(),
      Body::Commits(commits) => commits.read(cx).covers_window(),
    }
  }

  fn into_any_element(self) -> gpui::AnyElement {
    match self {
      Body::Code(code) => code.into_any_element(),
      Body::Commits(commits) => commits.into_any_element(),
    }
  }
}

impl Render for WorkspaceView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.sync_tooltip_popup(cx);
    let p = theme::palette(self.dark);
    let snapshot = self.snapshot.clone();
    let header = match &snapshot {
      Some(snapshot) => self.header(&snapshot.header, &p, cx),
      // Until the first load: the empty bar, as tall as a row of buttons.
      None => div()
        .flex_none()
        .h(px(49.))
        .border_b_1()
        .border_color(p.border)
        .debug_selector(|| "workspace-header".into()),
    };
    let tab_row = snapshot
      .and_then(|snapshot| snapshot.tab_row)
      .map(|row| self.tab_row(&row, &p));
    let code = self.tab_body(cx);
    let frame = |webview: Option<&Webview<Wry>>| match webview {
      Some(webview) => tauri_plugin_gpui::webview_frame(webview)
        .size_full()
        .into_any_element(),
      None => div().size_full().into_any_element(),
    };
    let body = match code {
      // The tab, with React's terminal pane framed below it.
      Some(code) => {
        let covered = code.covers_window(cx);
        let sent = self.content_height.clone();
        let sink = self.sink.clone();
        div()
          .relative()
          .flex_1()
          .min_h_0()
          .flex()
          .flex_col()
          .child(
            gpui::canvas(
              move |bounds, _, cx| {
                let height = f32::from(bounds.size.height).round();
                if sent.replace(height) != height {
                  sink(
                    ScreenAction::Field {
                      field: "workspace.contentHeight".into(),
                      value: height.into(),
                    },
                    cx,
                  );
                }
              },
              |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
          )
          .child(div().flex_1().min_h_0().child(code.into_any_element()))
          .child(
            div()
              .flex_none()
              .h(px(self.route.frame_height.unwrap_or(FRAME_FALLBACK)))
              .debug_selector(|| "workspace-frame".into())
              .when(!covered, |this| this.child(frame(self.webview.as_ref()))),
          )
      }
      None => div()
        .flex_1()
        .min_h_0()
        .debug_selector(|| "workspace-frame".into())
        .child(frame(self.webview.as_ref())),
    };
    div()
      .size_full()
      .flex()
      .flex_col()
      .bg(p.background)
      .text_color(p.foreground)
      .child(header)
      .children(tab_row)
      .child(body)
  }
}
