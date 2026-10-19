//! `MergePreviewPage`: the commits and files a workspace merges into its
//! target branch, and the merge itself.
//!
//! Drawn from the backend directly: GPUI reads the workspace, its commits
//! ahead and its diff, and merges it. React still owns leaving the page,
//! which a merge does too (its caches refresh then).

use std::{
  collections::HashSet,
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, deferred, div, hsla, prelude::*, px, relative, rgb, App, Context, FontWeight, Hsla,
  SharedString, Window,
};

use super::{workspace_data::WorkspaceHost, ActionSink, ScreenAction};
use crate::{
  core::{self, MergeCommit},
  gpui_shell::{
    form::{self, Controls},
    native::{Backend, NativeEvent, ToastTone},
    theme::{self, Palette},
    ui::{self, Button, Size, TextSize, Variant},
  },
  jj::{JjCommitsAhead, JjRevisionDiff},
  local_db::Workspace,
};

/// What React sends: the workspace being merged.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  pub workspace_id: i64,
}

/// The merge strategies of the button's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
  #[default]
  Merge,
  Squash,
  Rebase,
}

impl Strategy {
  const ALL: [Strategy; 3] = [Strategy::Merge, Strategy::Squash, Strategy::Rebase];

  fn value(self) -> &'static str {
    match self {
      Strategy::Merge => "merge",
      Strategy::Squash => "squash",
      Strategy::Rebase => "rebase",
    }
  }

  fn parse(value: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|s| s.value() == value)
  }

  fn commit(self) -> MergeCommit {
    match self {
      Strategy::Merge => MergeCommit::Merge,
      Strategy::Squash => MergeCommit::SquashAndMerge,
      Strategy::Rebase => MergeCommit::RebaseAndMerge,
    }
  }
}

/// What the page reads from the backend.
#[derive(Debug, Clone, Default)]
pub struct Preview {
  pub workspace: Option<Workspace>,
  pub commits: Option<JjCommitsAhead>,
  pub diff: Option<JjRevisionDiff>,
  /// Why the commits or diff could not be read.
  pub error: Option<String>,
}

/// What the merge preview asks of the backend; faked in tests.
pub trait MergeHost: WorkspaceHost {
  fn merge_preview(&self, repo_path: &str, workspace_id: i64) -> Preview;
  fn merge(
    &self,
    repo_path: &str,
    workspace_id: i64,
    message: &str,
    strategy: Strategy,
  ) -> Result<(), String>;
}

/// `getFullWorkspacePath`.
fn full_path(workspace: &Workspace) -> String {
  if workspace.workspace_path.starts_with('/') {
    workspace.workspace_path.clone()
  } else {
    format!(
      "{}/.treq/workspaces/{}",
      workspace.repo_path, workspace.workspace_path
    )
  }
}

impl MergeHost for Backend {
  fn merge_preview(&self, repo_path: &str, workspace_id: i64) -> Preview {
    let workspace = crate::local_db::get_workspace_by_id(repo_path, workspace_id)
      .ok()
      .flatten();
    let Some(ws) = &workspace else {
      return Preview {
        error: Some(format!("Workspace {workspace_id} not found")),
        ..Preview::default()
      };
    };
    let target = target_branch(ws);
    let commits =
      crate::jj::jj_get_commits_ahead(&full_path(ws), target).map_err(|e| e.to_string());
    let diff = core::workspace_diff(repo_path, workspace_id);
    // React reads both at once and shows neither when one fails.
    match (commits, diff) {
      (Ok(commits), Ok(diff)) => Preview {
        workspace,
        commits: Some(commits),
        diff: Some(diff),
        error: None,
      },
      (Err(err), _) | (_, Err(err)) => Preview {
        workspace,
        error: Some(err),
        ..Preview::default()
      },
    }
  }

  fn merge(
    &self,
    repo_path: &str,
    workspace_id: i64,
    message: &str,
    strategy: Strategy,
  ) -> Result<(), String> {
    core::merge_workspace(repo_path, workspace_id, message, strategy.commit())
  }
}

fn target_branch(workspace: &Workspace) -> &str {
  workspace
    .target_branch
    .as_deref()
    .filter(|b| !b.is_empty())
    .unwrap_or("main")
}

/// The message the box starts with for `strategy`.
pub fn default_message(workspace: &Workspace, strategy: Strategy) -> String {
  let description = workspace
    .metadata
    .as_deref()
    .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
    .and_then(|m| m["description"].as_str().map(str::to_string))
    .unwrap_or_default();
  let target = target_branch(workspace);
  match strategy {
    Strategy::Squash => description,
    Strategy::Rebase => String::new(),
    Strategy::Merge if description.is_empty() => {
      format!("Merge {} into {target}", workspace.branch_name)
    }
    Strategy::Merge => format!(
      "Merge {} into {target}\n\n{description}",
      workspace.branch_name
    ),
  }
}

/// The merge preview page and what it read.
pub struct MergeView {
  host: Arc<dyn MergeHost>,
  /// React's handlers, for leaving the page.
  react: ActionSink,
  /// This view's own handler, which defers to after the current update.
  sink: ActionSink,
  controls: Controls,
  route: Route,
  dark: bool,
  /// `None` while loading.
  pub(super) preview: Option<Preview>,
  pub(super) strategy: Strategy,
  /// What was typed in the message box, over the default.
  pub(super) draft: Option<String>,
  pub(super) merging: bool,
  pub(super) menu_open: bool,
  /// When a press outside closed the menu: the press on its own button
  /// that closed it must not open it again.
  menu_closed_at: Option<Instant>,
  /// Files whose hunks are hidden; React expands every file at first.
  collapsed: HashSet<String>,
}

impl MergeView {
  pub fn new(
    route: Route,
    host: Arc<dyn MergeHost>,
    react: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let view = cx.weak_entity();
    let sink: ActionSink = Rc::new(move |action, cx| {
      let view = view.clone();
      cx.defer(move |cx| {
        let _ = view.update(cx, |view, cx| view.handle(action, cx));
      });
    });
    let mut this = Self {
      host,
      react,
      controls: Controls::new(sink.clone()),
      sink,
      route,
      dark,
      preview: None,
      strategy: Strategy::default(),
      draft: None,
      merging: false,
      menu_open: false,
      menu_closed_at: None,
      collapsed: HashSet::new(),
    };
    this.load(cx);
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.route != route {
      self.route = route;
      self.preview = None;
      self.strategy = Strategy::default();
      self.draft = None;
      self.collapsed.clear();
      self.load(cx);
    }
    if self.dark != dark {
      self.dark = dark;
    }
    cx.notify();
  }

  fn load(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    let route = self.route.clone();
    cx.spawn(async move |this, cx| {
      let preview = cx
        .background_spawn({
          let route = route.clone();
          async move { host.merge_preview(&route.repo_path, route.workspace_id) }
        })
        .await;
      let _ = this.update(cx, |view, cx| {
        if view.route != route {
          return;
        }
        if let Some(err) = &preview.error {
          view.toast(ToastTone::Error, "Failed to load merge preview", err);
        }
        view.preview = Some(preview);
        cx.notify();
      });
    })
    .detach();
  }

  fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    self.host.notify(NativeEvent::Toast {
      title: title.into(),
      description: description.into(),
      tone,
    });
  }

  fn workspace(&self) -> Option<&Workspace> {
    self.preview.as_ref()?.workspace.as_ref()
  }

  pub(super) fn message(&self) -> String {
    match (&self.draft, self.workspace()) {
      (Some(draft), _) => draft.clone(),
      (None, Some(ws)) => default_message(ws, self.strategy),
      (None, None) => String::new(),
    }
  }

  fn can_merge(&self) -> bool {
    let ahead = self
      .preview
      .as_ref()
      .and_then(|p| p.commits.as_ref())
      .is_some_and(|c| c.total_count > 0);
    ahead && (self.strategy == Strategy::Rebase || !self.message().trim().is_empty())
  }

  fn handle(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    match action {
      ScreenAction::Field { field, value } if field == "merge.message" => {
        self.draft = value.as_str().map(str::to_string);
      }
      ScreenAction::Press { id } => match id.as_str() {
        "merge.menu" => {
          let just_closed = self
            .menu_closed_at
            .take()
            .is_some_and(|at| at.elapsed() < Duration::from_millis(500));
          self.menu_open = !self.menu_open && !just_closed;
        }
        "merge.menu.close" if self.menu_open => {
          self.menu_open = false;
          self.menu_closed_at = Some(Instant::now());
        }
        "merge.menu.close" => {}
        "merge.confirm" => self.merge(cx),
        id => {
          if let Some(strategy) = id.strip_prefix("merge.strategy:").and_then(Strategy::parse) {
            self.strategy = strategy;
            self.menu_open = false;
          } else if let Some(path) = id.strip_prefix("merge.file:") {
            if !self.collapsed.remove(path) {
              self.collapsed.insert(path.to_string());
            }
          } else {
            (self.react)(ScreenAction::Press { id: id.to_string() }, cx);
          }
        }
      },
      other => (self.react)(other, cx),
    }
    cx.notify();
  }

  fn merge(&mut self, cx: &mut Context<Self>) {
    if self.merging {
      return;
    }
    let Some(ws) = self.workspace().cloned() else {
      return;
    };
    let message = self.message();
    if self.strategy != Strategy::Rebase && message.trim().is_empty() {
      self.toast(ToastTone::Error, "Commit message required", "");
      return;
    }
    self.merging = true;
    let (host, strategy, route) = (self.host.clone(), self.strategy, self.route.clone());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move {
          host.merge(&route.repo_path, route.workspace_id, &message, strategy)
        })
        .await;
      let _ = this.update(cx, |view, cx| {
        view.merging = false;
        match result {
          Ok(()) => {
            (view.react)(
              ScreenAction::Press {
                id: "mergePreview.merged".into(),
              },
              cx,
            );
            let (title, done) = match strategy {
              Strategy::Squash => ("Workspace squashed", "squashed"),
              Strategy::Rebase => ("Workspace rebased", "rebased"),
              Strategy::Merge => ("Workspace merged", "merged"),
            };
            view.toast(
              ToastTone::Success,
              title,
              format!(
                "Successfully {done} {} into {}",
                ws.branch_name,
                target_branch(&ws)
              ),
            );
          }
          Err(err) => view.toast(ToastTone::Error, "Merge failed", err),
        }
        cx.notify();
      });
    })
    .detach();
  }

  fn press(&self, id: impl Into<String>) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) {
    let (sink, id) = (self.sink.clone(), id.into());
    move |_, _, cx| sink(ScreenAction::Press { id: id.clone() }, cx)
  }
}

/// `hsl(142, 76%, 36%)`, the merge button's inline color.
fn merge_green() -> Hsla {
  hsla(142. / 360., 0.76, 0.36, 1.)
}

fn heading(label: impl Into<SharedString>) -> gpui::Div {
  ui::text(div(), TextSize::Sm)
    .font_weight(FontWeight::SEMIBOLD)
    .mb_3()
    .child(label.into())
}

/// `border rounded-lg p-6 text-center text-muted-foreground`.
fn empty(p: &Palette, label: String) -> gpui::Div {
  ui::text(div(), TextSize::Sm)
    .border_1()
    .border_color(p.border)
    .rounded(px(12.))
    .p_6()
    .text_center()
    .text_color(p.muted_foreground)
    .child(label)
}

/// `border rounded-lg divide-y`.
fn list(p: &Palette) -> gpui::Div {
  div()
    .border_1()
    .border_color(p.border)
    .rounded(px(12.))
    .flex()
    .flex_col()
}

impl Render for MergeView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    let Some(preview) = self.preview.clone() else {
      return div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(ui::spinner("merge-loading", 32., p.muted_foreground))
        .into_any_element();
    };
    let target = preview
      .workspace
      .as_ref()
      .map_or("main", target_branch)
      .to_string();
    let branch = preview
      .workspace
      .as_ref()
      .map(|w| w.branch_name.clone())
      .unwrap_or_default();

    let header = div()
      .border_b_1()
      .border_color(p.border)
      .p_4()
      .flex()
      .items_center()
      .gap_4()
      .flex_none()
      .child(
        Button::new("merge-back", &p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .icon(theme::ICON_ARROW_LEFT, 16.)
          .on_click(self.press("mergePreview.cancel")),
      )
      .child(
        div()
          .flex_1()
          .child(
            ui::text(div(), TextSize::Lg)
              .font_weight(FontWeight::SEMIBOLD)
              .child("Merge Preview"),
          )
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child(format!("{branch} \u{2192} {target}")),
          ),
      );

    let content = div()
      .id("merge-scroll")
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .p_4()
      .flex()
      .flex_col()
      .gap_6()
      .child(self.message_card(&preview, &target, &p, window, cx))
      .child(commits(&preview, &target, &p))
      .child(self.files(&preview, &p));

    div()
      .size_full()
      .flex()
      .flex_col()
      .bg(p.background)
      .child(header)
      .child(content)
      .into_any_element()
  }
}

impl MergeView {
  fn message_card(
    &mut self,
    preview: &Preview,
    target: &str,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let rebase = self.strategy == Strategy::Rebase;
    let message = self.message();
    let textarea = self.controls.textarea(
      "merge.message",
      &message,
      "Enter merge commit message...",
      3,
      window,
      cx,
    );
    let disabled = !self.can_merge() || self.merging;
    let label = if self.merging {
      "Merging..."
    } else {
      match self.strategy {
        Strategy::Squash => "Squash and merge",
        Strategy::Rebase => "Rebase and merge",
        Strategy::Merge => "Confirm merge",
      }
    };
    let green = merge_green();
    let confirm = Button::new("merge-confirm", p)
      .disabled(disabled)
      .bg(green)
      .hover_bg(green)
      .rounded_r_none()
      .child(if self.merging {
        ui::spinner("merge-merging", 16., p.primary_foreground).into_any_element()
      } else {
        ui::icon(theme::ICON_MERGE, 16., p.primary_foreground).into_any_element()
      })
      .label(label)
      .on_click(self.press("merge.confirm"));
    let menu = Button::new("merge-strategy", p)
      .disabled(disabled)
      .bg(green)
      .hover_bg(green)
      .rounded_l_none()
      .px_2()
      .icon(theme::ICON_CHEVRON_DOWN, 16.)
      .on_click(self.press("merge.menu"));
    let commit_count = preview
      .commits
      .as_ref()
      .map(|c| c.commits.len())
      .filter(|&n| n > 0);

    let buttons = div()
      .relative()
      .flex()
      .items_center()
      .child(confirm)
      .child(div().w(px(1.)).h(px(40.)).bg(p.foreground.opacity(0.3)))
      .child(menu)
      .when(self.menu_open && !disabled, |this| {
        this.child(self.strategy_menu(commit_count, target, p))
      });

    div().flex().justify_center().px_4().child(
      ui::card(p)
        .w(relative(0.75))
        .p_6()
        .flex()
        .flex_col()
        .gap_4()
        .child(
          div()
            .child(heading(if rebase {
              "Commit Message (not used for rebase)"
            } else {
              "Merge Commit Message"
            }))
            .child(
              ui::text(
                div()
                  .id("merge-message")
                  .debug_selector(|| "merge-message".into()),
                TextSize::Sm,
              )
              .child(form::textarea_field(&textarea, true, rebase)),
            )
            .when(rebase, |this| {
              this.child(
                ui::text(div(), TextSize::Xs)
                  .mt_2()
                  .text_color(p.muted_foreground)
                  .child("Rebase keeps the original commit messages."),
              )
            }),
        )
        .child(div().flex().justify_start().child(buttons)),
    )
  }

  /// The `DropdownMenuContent align="end" className="w-64"` radio menu.
  fn strategy_menu(&self, commits: Option<usize>, target: &str, p: &Palette) -> impl IntoElement {
    let sink = self.sink.clone();
    let commits = commits.map_or("multiple".to_string(), |n| n.to_string());
    let items = [
      (
        Strategy::Merge,
        theme::ICON_MERGE,
        "Create a merge commit",
        "All commits from this branch will be added to the base branch via a merge commit"
          .to_string(),
      ),
      (
        Strategy::Squash,
        theme::ICON_COMMIT,
        "Squash and merge",
        format!("The {commits} commits from this branch will be combined into one commit"),
      ),
      (
        Strategy::Rebase,
        theme::ICON_BRANCH_BOLD,
        "Rebase and merge",
        format!("Commits will be rebased onto {target} with a linear history"),
      ),
    ];
    let p = *p;
    deferred(
      div()
        .id("merge-strategy-menu")
        .debug_selector(|| "merge-strategy-menu".into())
        .absolute()
        .top(px(44.))
        .right_0()
        .w(px(256.))
        .p_1()
        .rounded(px(8.))
        .border_1()
        .border_color(p.border)
        .bg(p.popover)
        .text_color(p.popover_foreground)
        .shadow_md()
        .occlude()
        .on_mouse_down_out(move |_, _, cx| {
          sink(
            ScreenAction::Press {
              id: "merge.menu.close".into(),
            },
            cx,
          )
        })
        .children(items.into_iter().map(|(strategy, icon, title, detail)| {
          let id = SharedString::from(format!("merge-strategy-{}", strategy.value()));
          let selector = id.to_string();
          ui::text(div(), TextSize::Sm)
            .id(id)
            .debug_selector(move || selector)
            .relative()
            .flex()
            .flex_col()
            .items_start()
            .py_3()
            .pl_8()
            .pr_2()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |s| s.bg(p.menu_accent))
            .child(
              div()
                .absolute()
                .left_2()
                .top_3()
                .size_4()
                .flex()
                .items_center()
                .justify_center()
                .when(strategy == self.strategy, |this| {
                  this.child(div().size_2().rounded_full().bg(p.popover_foreground))
                }),
            )
            .child(
              div()
                .flex()
                .items_center()
                .gap_2()
                .font_weight(FontWeight::SEMIBOLD)
                .child(ui::icon(icon, 16., p.popover_foreground))
                .child(title),
            )
            .child(
              ui::text(div(), TextSize::Xs)
                .mt_1()
                .pl_6()
                .text_color(p.muted_foreground)
                .child(detail),
            )
            .on_click(self.press(format!("merge.strategy:{}", strategy.value())))
        })),
    )
    .with_priority(1)
  }

  fn files(&self, preview: &Preview, p: &Palette) -> impl IntoElement {
    let count = preview.diff.as_ref().map_or(0, |d| d.committed_files.len());
    let body = match &preview.diff {
      Some(diff) if count > 0 => list(p)
        .children(diff.hunks_by_file.iter().enumerate().map(|(i, file)| {
          let expanded = !self.collapsed.contains(&file.path);
          let id = SharedString::from(format!("merge-file-{}", file.path));
          let selector = id.to_string();
          div()
            .when(i > 0, |this| this.border_t_1().border_color(p.border))
            .child(
              div()
                .id(id)
                .debug_selector(move || selector)
                .w_full()
                .p_3()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(|s| s.bg(p.muted.opacity(0.5)))
                .child(ui::icon(
                  if expanded {
                    theme::ICON_CHEVRON_DOWN
                  } else {
                    theme::ICON_CHEVRON_RIGHT
                  },
                  16.,
                  p.muted_foreground,
                ))
                .child(ui::icon(theme::ICON_FILE_TEXT, 16., p.muted_foreground))
                .child(
                  ui::text(div(), TextSize::Sm)
                    .font_family(theme::MONO_FONT)
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(file.path.clone()),
                )
                .on_click(self.press(format!("merge.file:{}", file.path))),
            )
            .when(expanded && !file.hunks.is_empty(), |this| {
              this.child(
                div()
                  .border_t_1()
                  .border_color(p.border)
                  .bg(p.muted.opacity(0.3))
                  .children(file.hunks.iter().enumerate().map(|(h, hunk)| {
                    hunk_lines(
                      SharedString::from(format!("merge-hunk-{i}-{h}")),
                      &hunk.header,
                      &hunk.lines,
                      p,
                    )
                  })),
              )
            })
        }))
        .into_any_element(),
      _ => empty(p, "No file changes".into()).into_any_element(),
    };
    div()
      .child(heading(format!("Changed Files ({count})")))
      .child(body)
  }
}

/// A hunk's `<pre className="p-2 text-xs font-mono overflow-x-auto">`.
fn hunk_lines(id: SharedString, header: &str, lines: &[String], p: &Palette) -> impl IntoElement {
  let added: Hsla = rgb(0x10b981).into();
  let removed: Hsla = rgb(0xef4444).into();
  let selector = id.to_string();
  ui::text(div(), TextSize::Xs)
    .id(id)
    .debug_selector(move || selector)
    .p_2()
    .font_family(theme::MONO_FONT)
    .overflow_x_scroll()
    .whitespace_nowrap()
    .child(
      div()
        .mb_1()
        .text_color(p.muted_foreground)
        .child(header.to_string()),
    )
    .children(lines.iter().map(|line| {
      let bg = if line.starts_with('+') && !line.starts_with("+++") {
        Some(added.opacity(0.2))
      } else if line.starts_with('-') && !line.starts_with("---") {
        Some(removed.opacity(0.2))
      } else {
        None
      };
      // An empty line still takes its height, as in a `<pre>`.
      let text = if line.is_empty() { " " } else { line.as_str() };
      div()
        .min_w_full()
        .when_some(bg, |this, bg| this.bg(bg))
        .child(text.replace('\t', "    "))
    }))
}

fn commits(preview: &Preview, target: &str, p: &Palette) -> impl IntoElement {
  let total = preview.commits.as_ref().map_or(0, |c| c.total_count);
  let body = match &preview.commits {
    Some(ahead) if !ahead.commits.is_empty() => list(p)
      .children(ahead.commits.iter().enumerate().map(|(i, commit)| {
        let description = if commit.description.is_empty() {
          "(no description)".to_string()
        } else {
          commit.description.clone()
        };
        div()
          .when(i > 0, |this| this.border_t_1().border_color(p.border))
          .p_3()
          .flex()
          .items_start()
          .gap_3()
          .child(
            ui::text(div(), TextSize::Xs)
              .flex_none()
              .bg(p.muted)
              .px(px(6.))
              .py(px(2.))
              .rounded(px(4.))
              .font_family(theme::MONO_FONT)
              .child(commit.short_id.clone()),
          )
          .child(
            div()
              .flex_1()
              .min_w_0()
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_weight(FontWeight::MEDIUM)
                  .truncate()
                  .child(description),
              )
              .child(
                ui::text(div(), TextSize::Xs)
                  .flex()
                  .flex_wrap()
                  .items_center()
                  .gap_2()
                  .mt(px(2.))
                  .text_color(p.muted_foreground)
                  .child(commit.author_name.clone())
                  .when(commit.insertions > 0 || commit.deletions > 0, |this| {
                    this.child(
                      div()
                        .flex()
                        .child(
                          div()
                            .text_color(rgb(0x16a34a))
                            .child(format!("+{}", commit.insertions)),
                        )
                        .child("\u{a0}")
                        .child(
                          div()
                            .text_color(rgb(0xdc2626))
                            .child(format!("-{}", commit.deletions)),
                        ),
                    )
                  }),
              ),
          )
      }))
      .into_any_element(),
    _ => empty(p, format!("No commits ahead of {target}")).into_any_element(),
  };
  div()
    .child(heading(format!("Commits to be merged ({total})")))
    .child(body)
}
