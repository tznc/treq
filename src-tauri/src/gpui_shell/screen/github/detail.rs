//! `PrDetailPanel`, `IssueDetailPanel` and their actions, and the PR's
//! workspace checks (`PrWorkspaceChecks`).

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::{zpx, DismissExt as _};
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, Context, Div, FontWeight, KeyDownEvent,
  MouseButton, SharedString, Window,
};

use super::{
  parts::{self, named},
  Busy, DetailKey, GithubView, Popover,
};
use crate::gpui_shell::screen::reactions::{self, Reacts};
use crate::{
  core::running_checks::CheckRun,
  github::{GhIssue, GhIssueComment, GhPullRequest, PrCiStatus},
  gpui_shell::{
    form,
    native::ToastTone,
    popup::{Side, TooltipContent},
    screen::{
      code::markdown,
      github_data::{self as data, CiState, GithubHost, IssueChange, PrChange, Tab},
      workspace_modal::{dialog_description, dialog_header, dialog_panel},
    },
    theme::{self, Palette},
    time,
    ui::{self, Button, EscapeExt as _, Size, TextSize, Tone, Variant},
  },
  local_db::Workspace,
};

/// `ISSUE_CLOSE_REASONS`.
const CLOSE_REASONS: [(&str, &str); 2] = [
  ("completed", "Close as completed"),
  ("not planned", "Close as not planned"),
];

impl GithubView {
  /// Runs `work` on the open detail with `busy` set; a failure toasts
  /// `fail`, a success runs `ok` while the same detail shows.
  fn mutate(
    &mut self,
    busy: Busy,
    fail: &'static str,
    work: impl FnOnce(&dyn GithubHost, &DetailKey) -> Result<(), String> + Send + 'static,
    ok: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let Some(key) = self.detail.key.clone() else {
      return;
    };
    if !self.detail.busy.insert(busy) {
      return;
    }
    let generation = self.detail.generation;
    self.run(
      cx,
      move |host| work(host, &key),
      move |view, result, cx| {
        let current = view.detail.generation == generation;
        if current {
          view.detail.busy.remove(&busy);
        }
        match result {
          Ok(()) if current => ok(view, cx),
          Ok(()) => {}
          Err(err) => {
            if busy == Busy::Delete && current {
              view.detail.confirm_delete = false;
            }
            view.toast(ToastTone::Error, fail, err);
          }
        }
      },
    );
  }

  fn post_comment(&mut self, cx: &mut Context<Self>) {
    let body = self.comment.clone();
    if body.trim().is_empty() || self.detail.busy.contains(&Busy::Comment) {
      return;
    }
    let issue = self.route.tab == Tab::Issues;
    self.mutate(
      Busy::Comment,
      "Failed to post comment",
      move |host, key| {
        if issue {
          host.change_issue(&key.repo, key.number, &IssueChange::Comment(body))
        } else {
          host.change_pr(&key.repo, key.number, &PrChange::Comment(body))
        }
      },
      |view, cx| {
        view.comment.clear();
        view.reset_field("github.comment");
        view.load_detail(cx);
      },
      cx,
    );
  }

  /// The open issue's or pull request's comment `id`.
  fn comment_mut(&mut self, id: &str) -> Option<&mut GhIssueComment> {
    let comments = match (&mut self.detail.issue, &mut self.detail.pr) {
      (Some(issue), _) => issue.comments.as_mut(),
      (None, Some(pr)) => pr.comments.as_mut(),
      (None, None) => None,
    }?;
    comments.iter_mut().find(|c| c.id == id)
  }

  fn change_issue(
    &mut self,
    busy: Busy,
    fail: &'static str,
    change: IssueChange,
    cx: &mut Context<Self>,
  ) {
    let deleting = change == IssueChange::Delete;
    self.mutate(
      busy,
      fail,
      move |host, key| host.change_issue(&key.repo, key.number, &change),
      move |view, cx| {
        view.reload_list(false, cx);
        if deleting {
          view.detail.confirm_delete = false;
          view.press("github.close", cx);
        } else {
          view.load_detail(cx);
        }
      },
      cx,
    );
  }

  /// `IssueEditForm`'s Save: the editor closes once the issue is read again.
  fn save_issue(&mut self, cx: &mut Context<Self>) {
    let Some((title, body)) = self.detail.editing.clone() else {
      return;
    };
    if title.trim().is_empty() {
      return;
    }
    let Some(key) = self.detail.key.clone() else {
      return;
    };
    if !self.detail.busy.insert(Busy::Save) {
      return;
    }
    let generation = self.detail.generation;
    self.run(
      cx,
      move |host| {
        host.change_issue(&key.repo, key.number, &IssueChange::Edit { title, body })?;
        Ok(host.issue(&key.repo, key.number).ok())
      },
      move |view, result: Result<Option<GhIssue>, String>, cx| {
        if view.detail.generation != generation {
          return;
        }
        view.detail.busy.remove(&Busy::Save);
        match result {
          Ok(issue) => {
            if issue.is_some() {
              view.detail.issue = issue;
            }
            view.detail.editing = None;
            view.reload_list(false, cx);
          }
          Err(err) => view.toast(ToastTone::Error, "Failed to update issue", err),
        }
      },
    );
  }

  fn change_pr(
    &mut self,
    busy: Busy,
    fail: &'static str,
    change: PrChange,
    cx: &mut Context<Self>,
  ) {
    let head = self.detail.pr.as_ref().map(|pr| pr.head_ref_name.clone());
    self.mutate(
      busy,
      fail,
      move |host, key| host.change_pr(&key.repo, key.number, &change),
      move |view, cx| {
        view.load_detail(cx);
        view.reload_list(false, cx);
        // The workspace's PR badge reads the poller's cache.
        if let Some(head) = &head {
          view.pr_status_changed(head);
        }
      },
      cx,
    );
  }

  fn open_workspace(&mut self, cx: &mut Context<Self>) {
    let Some(pr) = self.detail.pr.clone() else {
      return;
    };
    if !self.detail.busy.insert(Busy::Workspace) {
      return;
    }
    let repo_path = self.route.repo_path.clone();
    let generation = self.detail.generation;
    let head = pr.head_ref_name.clone();
    self.run(
      cx,
      move |host| host.open_pr_workspace(&repo_path, &pr),
      move |view, result, cx| {
        if view.detail.generation == generation {
          view.detail.busy.remove(&Busy::Workspace);
        }
        match result {
          Ok((id, created)) => {
            let title = if created {
              "Workspace created"
            } else {
              "Workspace opened"
            };
            view.toast(ToastTone::Success, title, head);
            view.load_workspaces(cx);
            view.press(format!("github.openWorkspace:{id}"), cx);
          }
          Err(err) => view.toast(ToastTone::Error, "Failed to open workspace", err),
        }
      },
    );
  }

  pub(super) fn detail_panel(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let number = self.route.selected.unwrap_or_default();
    let issue = self.route.tab == Tab::Issues;
    let heading = if issue {
      format!("Issue #{number}")
    } else {
      format!("Pull Request #{number}")
    };
    let header = div()
      .flex()
      .flex_none()
      .items_center()
      .justify_between()
      .px_4()
      .py_3()
      .border_b_1()
      .border_color(p.border)
      .child(
        ui::text(div(), TextSize::Sm)
          .font_weight(FontWeight::SEMIBOLD)
          .text_color(p.muted_foreground)
          .child(heading),
      )
      .child(
        Button::new("github-detail-close", p)
          .aria_label("Close")
          .variant(Variant::Ghost)
          .size(Size::Icon)
          .w(zpx(28.))
          .h(zpx(28.))
          .icon(theme::ICON_X, 16.)
          .on_click(cx.listener(|view, _, _, cx| view.press("github.close", cx))),
      );
    let loaded = self.detail.issue.is_some() || self.detail.pr.is_some();
    let body = if self.detail.loading {
      div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .child(ui::spinner(
          "github-detail-loading",
          20.,
          p.muted_foreground,
        ))
        .into_any_element()
    } else if let (Some(err), false) = (self.detail.error.clone(), loaded) {
      let what = if issue { "issue" } else { "pull request" };
      parts::error_state(
        "github-detail",
        &format!("Could not load {what} #{number}"),
        &err,
        p,
        None,
      )
      .into_any_element()
    } else if let Some(issue) = self.detail.issue.clone().filter(|_| issue) {
      self.issue_content(&issue, p, window, cx).into_any_element()
    } else if let Some(pr) = self.detail.pr.clone().filter(|_| !issue) {
      self.pr_content(&pr, p, window, cx).into_any_element()
    } else {
      div().into_any_element()
    };
    named(div(), "github-detail")
      .flex_1()
      .min_w_0()
      .flex()
      .flex_col()
      .overflow_hidden()
      .child(header)
      .child(body)
  }

  /// The scrolling `px-4 py-3 space-y-4` column of a detail.
  fn content() -> gpui::Stateful<Div> {
    div()
      .id("github-detail-scroll")
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .px_4()
      .py_3()
      .flex()
      .flex_col()
      .gap_4()
  }

  fn title(title: &str) -> Div {
    ui::text(div(), TextSize::Xl)
      .flex_1()
      .min_w_0()
      .font_weight(FontWeight::SEMIBOLD)
      .child(title.to_string())
  }

  fn body(&self, id: &str, body: &Option<String>, p: &Palette) -> Option<Div> {
    let body = body.as_deref().filter(|b| !b.is_empty())?;
    Some(
      div()
        .bg(p.muted.opacity(0.3))
        .rounded(radius::MD)
        .p_3()
        .child(markdown(
          SharedString::from(id.to_string()),
          body,
          13.,
          self.dark,
        )),
    )
  }

  fn comments(
    &self,
    comments: &Option<Vec<GhIssueComment>>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<Div> {
    let comments = comments.as_ref().filter(|c| !c.is_empty())?;
    Some(
      div()
        .flex()
        .flex_col()
        .gap_3()
        .child(parts::section_heading(
          format!("Comments ({})", comments.len()),
          Some(theme::ICON_MESSAGE),
          p,
        ))
        .children(comments.iter().map(|c| {
          parts::comment(
            &c.author.login,
            &time::date(&c.created_at),
            markdown(
              SharedString::from(format!("github-comment-{}", c.id)),
              &c.body,
              13.,
              self.dark,
            )
            .into_any_element(),
            p,
          )
          .child(reactions::bar(
            &self.tips,
            self.reaction_picker.as_deref(),
            &c.id,
            &c.reactions,
            p,
            cx,
          ))
        })),
    )
  }

  /// "Add Comment": the box and the buttons after "Comment".
  fn add_comment(
    &mut self,
    submit_on_enter: bool,
    actions: Vec<gpui::AnyElement>,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Div {
    let comment = self.comment.clone();
    let state = self.textarea(
      "github.comment",
      &comment,
      "Leave a comment...",
      3,
      window,
      cx,
    );
    let busy = self.detail.busy.contains(&Busy::Comment);
    div()
      .flex()
      .flex_col()
      .gap_2()
      .child(parts::section_heading("Add Comment", None, p))
      .child(
        ui::text(div().id("github-comment-box"), TextSize::Sm)
          .debug_selector(|| "github-comment-box".into())
          .when(submit_on_enter, |d| {
            d.capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
              let k = &e.keystroke;
              if k.key == "enter"
                && (crate::gpui_shell::keys::held(k.modifiers).platform
                  || crate::gpui_shell::keys::held(k.modifiers).control)
              {
                cx.stop_propagation();
                view.post_comment(cx);
              }
            }))
          })
          .child(form::textarea_field(&state, false, false)),
      )
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(
            parts::busy_spinner(
              Button::new("github-comment", p)
                .size(Size::Sm)
                .gap_1()
                .disabled(comment.trim().is_empty() || busy),
              "github-comment",
              busy,
              p.primary_foreground,
            )
            .label("Comment")
            .on_click(cx.listener(|view, _, _, cx| view.post_comment(cx))),
          )
          .children(actions),
      )
  }

  /// A small outline action button, with its spinner while `busy`.
  fn action(id: &'static str, label: &str, busy: bool, p: &Palette) -> Button {
    parts::busy_spinner(
      Button::new(id, p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .gap_1()
        .disabled(busy),
      id,
      busy,
      p.foreground,
    )
    .label(label.to_string())
  }

  fn issue_content(
    &mut self,
    issue: &GhIssue,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let head = match self.detail.editing.clone() {
      Some((title, body)) => self
        .edit_form(&title, &body, p, window, cx)
        .into_any_element(),
      None => {
        let destructive = p.destructive;
        let (number, url, title) = (issue.number, issue.url.clone(), issue.title.clone());
        let open = url.clone();
        let copy_link = self.copy_link(url.clone());
        let buttons = div()
          .flex()
          .flex_none()
          .items_center()
          .gap_2()
          .child(
            Button::new("github-edit", p)
              .aria_label("Edit issue")
              .variant(Variant::Ghost)
              .size(Size::Icon)
              .w(zpx(32.))
              .h(zpx(32.))
              .icon(theme::ICON_PENCIL, 16.)
              .on_click(cx.listener({
                let (title, body) = (issue.title.clone(), issue.body.clone().unwrap_or_default());
                move |view, _, _, cx| {
                  view.detail.editing = Some((title.clone(), body.clone()));
                  view.reset_field("github.edit.title");
                  view.reset_field("github.edit.body");
                  cx.notify();
                }
              })),
          )
          .child(
            Button::new("github-delete", p)
              .aria_label("Delete issue")
              .variant(Variant::Ghost)
              .size(Size::Icon)
              .w(zpx(32.))
              .h(zpx(32.))
              .text_color(p.muted_foreground)
              .hover_bg(destructive.opacity(0.1))
              .child(ui::icon(theme::ICON_TRASH, 16., p.muted_foreground))
              .on_click(cx.listener(|view, _, _, cx| {
                view.detail.confirm_delete = true;
                cx.notify();
              })),
          )
          .child(
            Button::new("github-agent", p)
              .size(Size::Sm)
              .gap_1p5()
              .icon(theme::ICON_SPARKLES, 16.)
              .label("Agent...")
              .on_click(cx.listener(move |view, _, _, cx| {
                view.field(
                  "github.startPrompt",
                  serde_json::json!({ "number": number, "url": url, "title": title }),
                  cx,
                )
              })),
          )
          .child(
            parts::open_in_web(
              "github-open-web",
              p,
              Box::new(cx.listener(move |view, _, _, _| view.open_url(&open))),
            )
            .on_right_click(copy_link),
          );
        div()
          .child(
            div()
              .flex()
              .items_start()
              .gap_3()
              .child(Self::title(&issue.title))
              .child(buttons),
          )
          .child(
            div()
              .flex()
              .flex_wrap()
              .items_center()
              .gap_2()
              .mt_1()
              .child(parts::state_chip(&issue.state, false, p))
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child(format!(
                    "#{} opened by {} on {}",
                    issue.number,
                    issue.author.login,
                    time::date(&issue.created_at)
                  )),
              ),
          )
          .children(parts::labels(&issue.labels))
          .into_any_element()
      }
    };
    let editing = self.detail.editing.is_some();
    let closing = self.detail.busy.contains(&Busy::Close);
    let reopening = self.detail.busy.contains(&Busy::Reopen);
    let actions = if issue.state == "OPEN" {
      let split = div()
        .relative()
        .flex()
        .items_center()
        .child(
          Self::action(
            "github-close-issue",
            if closing { "Closing…" } else { "Close Issue" },
            closing,
            p,
          )
          .rounded_r_none()
          .on_click(cx.listener(|view, _, _, cx| {
            view.change_issue(
              Busy::Close,
              "Failed to close issue",
              IssueChange::Close("completed".into()),
              cx,
            )
          })),
        )
        .child(
          Button::new("github-close-options", p)
            .aria_label("Close options")
            .variant(Variant::Outline)
            .size(Size::Sm)
            .rounded_l_none()
            .border_l_0()
            .px_1p5()
            .disabled(closing)
            .icon(theme::ICON_CHEVRON_DOWN, 16.)
            .on_click(cx.listener(|view, _, _, cx| view.toggle_popover(Popover::CloseIssue, cx))),
        )
        .when(self.popover == Some(Popover::CloseIssue) && !closing, |d| {
          d.child(
            deferred(
              parts::menu_panel("github-close-menu", p)
                .absolute()
                .top(zpx(36.))
                .left_0()
                .min_w(zpx(128.))
                .dismiss_with("github-close-issue", cx, |view, cx| {
                  view.close_popover(Popover::CloseIssue, cx)
                })
                .children(CLOSE_REASONS.iter().map(|&(reason, label)| {
                  ui::MenuItem::new(
                    format!("github-close-{}", reason.replace(' ', "-")),
                    label,
                    p,
                  )
                  .on_click(cx.listener(move |view, _, _, cx| {
                    view.popover = None;
                    view.change_issue(
                      Busy::Close,
                      "Failed to close issue",
                      IssueChange::Close(reason.into()),
                      cx,
                    )
                  }))
                })),
            )
            .with_priority(1),
          )
        });
      split.into_any_element()
    } else {
      Self::action(
        "github-reopen",
        if reopening {
          "Reopening…"
        } else {
          "Reopen Issue"
        },
        reopening,
        p,
      )
      .on_click(cx.listener(|view, _, _, cx| {
        view.change_issue(
          Busy::Reopen,
          "Failed to reopen issue",
          IssueChange::Reopen,
          cx,
        )
      }))
      .into_any_element()
    };
    let add = self.add_comment(true, vec![actions], p, window, cx);
    Self::content()
      .child(head)
      .when(!editing, |d| {
        d.children(self.body("github-issue-body", &issue.body, p))
      })
      .children(self.comments(&issue.comments, p, cx))
      .child(add)
  }

  /// `IssueEditForm`.
  fn edit_form(
    &mut self,
    title: &str,
    body: &str,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let title_state = self.input("github.edit.title", title, "Title", window, cx);
    let body_state = self.textarea(
      "github.edit.body",
      body,
      "Description (optional)",
      8,
      window,
      cx,
    );
    let saving = self.detail.busy.contains(&Busy::Save);
    div()
      .flex()
      .flex_col()
      .gap_2()
      .child(ui::text(
        div().child(form::input_field(
          "github-edit-title",
          &title_state,
          false,
          false,
        )),
        TextSize::Sm,
      ))
      .child(ui::text(
        div().child(form::textarea_field(&body_state, false, false)),
        TextSize::Sm,
      ))
      .child(
        div()
          .flex()
          .gap_2()
          .child(
            parts::busy_spinner(
              Button::new("github-edit-save", p)
                .size(Size::Sm)
                .gap_1()
                .disabled(title.trim().is_empty() || saving),
              "github-edit-save",
              saving,
              p.primary_foreground,
            )
            .label("Save")
            .on_click(cx.listener(|view, _, _, cx| view.save_issue(cx))),
          )
          .child(
            Button::new("github-edit-cancel", p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .disabled(saving)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| {
                view.detail.editing = None;
                cx.notify();
              })),
          ),
      )
  }

  /// `DeleteIssueButton`'s confirmation.
  pub(super) fn delete_dialog(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let number = self.route.selected.unwrap_or_default();
    let pending = self.detail.busy.contains(&Busy::Delete);
    let (backdrop, panel) = dialog_panel("github-delete-dialog", 512., window.viewport_size(), p);
    let cancel = |view: &mut Self, cx: &mut Context<Self>| {
      if !view.detail.busy.contains(&Busy::Delete) {
        view.detail.confirm_delete = false;
        cx.notify();
      }
    };
    let panel = panel
      .child(dialog_header(
        format!("Delete issue #{number}?"),
        dialog_description(
          "This permanently deletes the issue and its comments on GitHub. You cannot undo this.",
          p,
        ),
      ))
      .child(
        div()
          .mt_4()
          .flex()
          .justify_end()
          .gap_2()
          .child(
            Button::new("github-delete-cancel", p)
              .variant(Variant::Ghost)
              .disabled(pending)
              .label("Cancel")
              .on_click(cx.listener(move |view, _, _, cx| cancel(view, cx))),
          )
          .child(
            parts::busy_spinner(
              Button::new("github-delete-confirm", p)
                .bg(p.destructive)
                .hover_bg(p.destructive.opacity(0.9))
                .text_color(p.destructive_foreground)
                .gap_1()
                .disabled(pending),
              "github-delete-confirm",
              pending,
              p.destructive_foreground,
            )
            .label("Delete")
            .on_click(cx.listener(|view, _, _, cx| {
              view.change_issue(
                Busy::Delete,
                "Failed to delete issue",
                IssueChange::Delete,
                cx,
              )
            })),
          ),
      );
    deferred(
      anchored().position(point(px(0.), px(0.))).child(
        backdrop
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _, _, cx| cancel(view, cx)),
          )
          .child(
            panel
              .id("github-delete-panel")
              .track_focus(self.dialog_focus())
              .on_escape(cx.listener(move |view, _, _, cx| cancel(view, cx)))
              .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
          ),
      ),
    )
    .with_priority(2)
  }

  fn pr_content(
    &mut self,
    pr: &GhPullRequest,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let existing = self.pr_workspace().cloned();
    let opening = self.detail.busy.contains(&Busy::Workspace);
    let number = pr.number;
    let open = pr.url.clone();
    let copy_link = self.copy_link(pr.url.clone());
    let workspace_icon = if opening {
      ui::spinner("github-workspace-spin", 16., p.foreground).into_any_element()
    } else if existing.is_some() {
      ui::icon(theme::ICON_BRANCH_BOLD, 16., p.foreground).into_any_element()
    } else {
      // The branch with a small plus at its bottom right.
      div()
        .relative()
        .size(zpx(16.))
        .flex_none()
        .child(ui::icon(theme::ICON_BRANCH_BOLD, 16., p.foreground))
        .child(
          div()
            .absolute()
            .bottom(zpx(-2.))
            .right(zpx(-2.))
            .child(ui::icon(theme::ICON_PLUS, 10., p.foreground)),
        )
        .into_any_element()
    };
    let buttons = div()
      .flex()
      .flex_none()
      .items_center()
      .gap_2()
      .child(
        Button::new("github-review", p)
          .size(Size::Sm)
          .gap_1p5()
          .icon(theme::ICON_FILE_DIFF, 16.)
          .label("Review changes")
          .on_click(
            cx.listener(move |view, _, _, cx| view.press(format!("github.review:{number}"), cx)),
          ),
      )
      .child(
        Button::new("github-workspace", p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .gap_1p5()
          .disabled(opening)
          .child(workspace_icon)
          .label(if existing.is_some() {
            "Open Workspace"
          } else {
            "Create Workspace"
          })
          .on_click(cx.listener(|view, _, _, cx| view.open_workspace(cx))),
      )
      .child(
        parts::open_in_web(
          "github-open-web",
          p,
          Box::new(cx.listener(move |view, _, _, _| view.open_url(&open))),
        )
        .on_right_click(copy_link),
      );
    let checks = self.detail.checks.clone().filter(|c| c.total > 0);
    let head = div()
      .child(
        div()
          .flex()
          .items_start()
          .gap_3()
          .child(Self::title(&pr.title))
          .child(buttons),
      )
      .child(
        div()
          .flex()
          .flex_wrap()
          .items_center()
          .gap_2()
          .mt_1()
          .child(parts::state_chip(&pr.state, pr.is_draft, p))
          .children(checks.as_ref().map(|c| self.ci_button(c, p, cx)))
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .font_family(theme::MONO_FONT)
              .child(format!("{} → {}", pr.head_ref_name, pr.base_ref_name)),
          ),
      )
      .child(
        ui::text(div(), TextSize::Sm)
          .mt_1()
          .text_color(p.muted_foreground)
          .child(format!(
            "#{} opened by {} on {}",
            pr.number,
            pr.author.login,
            time::date(&pr.created_at)
          )),
      )
      .children(parts::labels(&pr.labels));
    let checks_list = checks.map(|c| {
      div()
        .flex()
        .flex_col()
        .gap_2()
        .child(parts::section_heading(
          format!("Checks ({}/{})", c.passed, c.total),
          Some(theme::ICON_LIST_CHECKS),
          p,
        ))
        .child(
          div()
            .border_1()
            .border_color(p.border)
            .rounded(radius::MD)
            .overflow_hidden()
            .children(
              data::by_severity(&c.checks)
                .into_iter()
                .enumerate()
                .map(|(i, check)| {
                  let link = check.link.clone();
                  let copy_link = self.copy_link(link.clone());
                  div()
                    .when(i > 0, |d| d.border_t_1().border_color(p.border))
                    .child(
                      parts::check_row(
                        format!("github-check-{i}"),
                        check,
                        p,
                        Some(Box::new(
                          cx.listener(move |view, _, _, _| view.open_url(&link)),
                        )),
                      )
                      .when(!check.link.is_empty(), |row| {
                        row.on_mouse_down(MouseButton::Right, copy_link)
                      }),
                    )
                }),
            ),
        )
    });
    let treq = existing.map(|ws| self.treq_section(&ws, p, cx));
    let busy = |b| self.detail.busy.contains(&b);
    let mut actions: Vec<gpui::AnyElement> = Vec::new();
    if pr.state == "OPEN" {
      let drafting = busy(Busy::Draft);
      let draft = pr.is_draft;
      actions.push(
        Self::action(
          "github-draft",
          if draft {
            "Ready for Review"
          } else {
            "Convert to Draft"
          },
          drafting,
          p,
        )
        .on_click(cx.listener(move |view, _, _, cx| {
          let fail = if draft {
            "Failed to mark ready for review"
          } else {
            "Failed to convert to draft"
          };
          view.change_pr(Busy::Draft, fail, PrChange::Draft(!draft), cx)
        }))
        .into_any_element(),
      );
      let closing = busy(Busy::Close);
      actions.push(
        Self::action(
          "github-close-pr",
          if closing { "Closing…" } else { "Close PR" },
          closing,
          p,
        )
        .on_click(cx.listener(|view, _, _, cx| {
          view.change_pr(
            Busy::Close,
            "Failed to close pull request",
            PrChange::Close,
            cx,
          )
        }))
        .into_any_element(),
      );
    } else if pr.state == "CLOSED" {
      actions.push(
        Self::action("github-reopen-pr", "Reopen PR", busy(Busy::Reopen), p)
          .on_click(cx.listener(|view, _, _, cx| {
            view.change_pr(
              Busy::Reopen,
              "Failed to reopen pull request",
              PrChange::Reopen,
              cx,
            )
          }))
          .into_any_element(),
      );
    }
    let add = self.add_comment(false, actions, p, window, cx);
    Self::content()
      .child(head)
      .children(checks_list)
      .children(treq)
      .children(self.body("github-pr-body", &pr.body, p))
      .children(self.comments(&pr.comments, p, cx))
      .child(add)
  }

  /// `CiStatusButton`: the passed/total pill and its checks popover.
  fn ci_button(&self, ci: &PrCiStatus, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let state = CiState::parse(&ci.state);
    let tone = match state {
      CiState::Success => Tone::Success,
      CiState::Failure => Tone::Danger,
      CiState::Pending => Tone::Warning,
    };
    let (border, text, hover) = (tone.color(p), tone.text(p), tone.tint(p));
    let summary = format!(
      "{}: {}/{} checks passed",
      state.label(),
      ci.passed,
      ci.total
    );
    let failing: Vec<SharedString> = ci
      .checks
      .iter()
      .filter(|c| CiState::of_bucket(&c.bucket) == CiState::Failure)
      .map(|c| SharedString::from(c.name.clone()))
      .collect();
    let tooltip = if failing.is_empty() {
      TooltipContent::Text(summary.into())
    } else {
      TooltipContent::List {
        title: summary.into(),
        items: vec![format!(
          "Failing: {}",
          failing
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join(", ")
        )
        .into()],
      }
    };
    let (icon, _) = parts::ci_style(state, p);
    let icon = ui::icon(icon, 16., text);
    let button = Button::new("github-ci", p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .gap_1()
      .px_2()
      .border_color(border)
      .text_color(text)
      .hover_bg(hover)
      .child(if state == CiState::Pending {
        ui::spin("github-ci-spin", icon).into_any_element()
      } else {
        icon.into_any_element()
      })
      .label(format!("{}/{}", ci.passed, ci.total))
      .on_click(cx.listener(|view, _, _, cx| view.toggle_popover(Popover::CiChecks, cx)));
    let popover =
      (self.popover == Some(Popover::CiChecks)).then(|| {
        deferred(
          named(div(), "github-ci-popover")
            .absolute()
            .top(zpx(36.))
            .right_0()
            .w(zpx(320.))
            .p_2()
            .rounded(radius::MD)
            .border_1()
            .border_color(p.border)
            .bg(p.popover)
            .text_color(p.popover_foreground)
            .shadow_md()
            .occlude()
            .dismiss_with("github-ci-checks", cx, |view, cx| {
              view.close_popover(Popover::CiChecks, cx)
            })
            .child(
              ui::text(div(), TextSize::Xs)
                .px_2()
                .pb_1()
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child("Checks"),
            )
            .child(
              div()
                .id("github-ci-checks")
                .max_h(zpx(320.))
                .overflow_y_scroll()
                .children(data::by_severity(&ci.checks).into_iter().enumerate().map(
                  |(i, check)| {
                    let link = check.link.clone();
                    let copy_link = self.copy_link(link.clone());
                    parts::check_row(
                      format!("github-ci-check-{i}"),
                      check,
                      p,
                      Some(Box::new(
                        cx.listener(move |view, _, _, _| view.open_url(&link)),
                      )),
                    )
                    .when(!check.link.is_empty(), |row| {
                      row.on_mouse_down(MouseButton::Right, copy_link)
                    })
                  },
                )),
            ),
        )
        .with_priority(1)
      });
    div()
      .relative()
      .child(
        self
          .tips
          .wrap("github-ci", Side::Bottom, Some(tooltip), button, cx),
      )
      .children(popover)
  }

  /// `PrWorkspaceChecks`: the workflow checks of the PR's workspace.
  fn treq_section(&self, ws: &Workspace, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let id = ws.id;
    let header = div()
      .flex()
      .items_center()
      .justify_between()
      .child(parts::section_heading("Treq checks", None, p))
      .when(self.logs_enabled, |d| {
        d.child(
          Button::new("github-open-logs", p)
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .gap_0()
            .child(ui::icon(theme::ICON_SCROLL_TEXT, 12., p.foreground).mr_1())
            .label("Open in Logs")
            .on_click(
              cx.listener(move |view, _, _, cx| view.press(format!("github.openLogs:{id}"), cx)),
            ),
        )
      });
    named(div(), "github-treq-checks")
      .flex()
      .flex_col()
      .gap_2()
      .child(header)
      .child(
        div()
          .border_1()
          .border_color(p.border)
          .rounded(radius::MD)
          .overflow_hidden()
          .child(self.checks_panel(ws, p, cx)),
      )
  }

  fn run_check(&self, ws: &Workspace, filename: &str, job: Option<&str>, cx: &mut Context<Self>) {
    let run = match job {
      Some(job) => CheckRun::Job {
        file: filename.into(),
        job: job.into(),
      },
      None => CheckRun::Workflow(filename.into()),
    };
    self.run_treq_check(ws, run, cx);
  }

  /// `WorkspaceChecksPanel`.
  fn checks_panel(&self, ws: &Workspace, p: &Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
    let Some(checks) = self.detail.treq.clone() else {
      return ui::text(div(), TextSize::Sm)
        .flex()
        .items_center()
        .gap_2()
        .p_4()
        .text_color(p.muted_foreground)
        .child(ui::spinner(
          "github-checks-loading",
          16.,
          p.muted_foreground,
        ))
        .child("Loading checks…")
        .into_any_element();
    };
    let running = &self.running_keys(cx);
    let is_running =
      |file: &str, job: Option<&str>| running.contains(&data::run_key(ws.id, file, job));
    let setup_running = checks
      .setup
      .as_ref()
      .is_some_and(|s| s.configured && (s.status == "pending" || s.status == "running"));
    let can_run = checks.trusted && !setup_running;
    let mut panel = named(div(), "workspace-checks")
      .flex()
      .flex_col()
      .gap_3()
      .p_4();
    if !checks.workflows.is_empty() && !checks.trusted {
      let (border, bg, fg) = (
        Tone::Warning.border(p),
        Tone::Warning.tint(p),
        Tone::Warning.text(p),
      );
      panel = panel.child(
        ui::text(div(), TextSize::Sm)
          .flex()
          .items_center()
          .justify_between()
          .rounded(radius::MD)
          .border_1()
          .border_color(border)
          .bg(bg)
          .px_4()
          .py_3()
          .child(
            div()
              .flex()
              .items_center()
              .gap_2()
              .text_color(fg)
              .child(ui::icon(theme::ICON_SHIELD_CHECK, 16., fg))
              .child("Trust this repository to enable running workflow checks."),
          )
          .child(
            Button::new("github-trust", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .ml_4()
              .label("Trust Repository")
              .on_click(cx.listener(|view, _, _, cx| {
                let repo_path = view.route.repo_path.clone();
                view.run(
                  cx,
                  move |host| host.trust_repo(&repo_path),
                  |view, result, cx| match result {
                    Ok(()) => view.load_treq(cx),
                    Err(err) => log::warn!("github panel: trust: {err}"),
                  },
                );
              })),
          ),
      );
    }
    if checks.workflows.is_empty() {
      return panel
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child("No workflows found. Add YAML files to ")
            .child(
              div()
                .font_family(theme::MONO_FONT)
                .child(".treq/workflows/"),
            )
            .child(" to define checks.")
            .flex()
            .flex_wrap(),
        )
        .into_any_element();
    }
    for (w, wf) in checks.workflows.iter().enumerate() {
      let latest = checks.latest.get(&wf.filename);
      let wf_running = is_running(&wf.filename, None);
      let status = if wf_running {
        Some("running")
      } else {
        latest.map(|r| r.status.as_str())
      };
      let file = wf.filename.clone();
      let ws_run = ws.clone();
      let header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(p.border)
        .child(
          div()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .child(status_icon(format!("github-wf-{w}-status"), status, 16., p))
            .child(
              div()
                .min_w_0()
                .child(
                  ui::text(div(), TextSize::Sm)
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(wf.name.clone()),
                )
                .child(
                  ui::text(div(), TextSize::Xs)
                    .font_family(theme::MONO_FONT)
                    .text_color(p.muted_foreground)
                    .truncate()
                    .child(match latest {
                      Some(run) => format!("{} · run #{} {}", wf.filename, run.id, run.status),
                      None => wf.filename.clone(),
                    }),
                ),
            ),
        )
        .child(
          run_button(
            format!("github-run-{w}"),
            &wf.name,
            latest.is_some(),
            wf_running,
            !can_run,
            Variant::Outline,
            p,
          )
          .on_click(cx.listener(move |view, _, _, cx| view.run_check(&ws_run, &file, None, cx))),
        );
      let jobs =
        wf.jobs
          .iter()
          .enumerate()
          .map(|(j, job)| {
            let run_job = latest.and_then(|r| r.jobs.iter().find(|rj| rj.job_id == job.id));
            let job_running = is_running(&wf.filename, Some(&job.id));
            let (file, job_id, ws_run) = (wf.filename.clone(), job.id.clone(), ws.clone());
            div()
              .when(j > 0, |d| d.border_t_1().border_color(p.border))
              .px_4()
              .py_2()
              .flex()
              .items_center()
              .justify_between()
              .child(
                ui::text(div(), TextSize::Sm)
                  .flex()
                  .items_center()
                  .gap_2()
                  .child(status_icon(
                    format!("github-wf-{w}-job-{j}-status"),
                    if job_running {
                      Some("running")
                    } else {
                      run_job.map(|r| r.status.as_str())
                    },
                    14.,
                    p,
                  ))
                  .child(job.name.clone()),
              )
              .child(
                run_button(
                  format!("github-run-{w}-{j}"),
                  &job.name,
                  run_job.is_some(),
                  job_running,
                  !can_run || wf_running,
                  Variant::Ghost,
                  p,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                  view.run_check(&ws_run, &file, Some(&job_id), cx)
                })),
              )
          })
          .collect::<Vec<_>>();
      let history = latest.map(|_| self.run_history(w, &wf.filename, p, cx));
      panel = panel.child(
        named(div(), format!("workflow-card-{w}"))
          .rounded(radius::MD)
          .border_1()
          .border_color(p.border)
          .bg(p.card)
          .text_color(p.card_foreground)
          .child(header)
          .child(div().children(jobs))
          .children(history),
      );
    }
    panel.into_any_element()
  }

  /// `RunHistory`: a workflow's past runs, each opening to its job logs.
  fn run_history(
    &self,
    w: usize,
    filename: &str,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let open = self.detail.history_open.contains(filename);
    let file = filename.to_string();
    let muted = p.muted;
    let toggle = ui::text(named(div(), format!("github-history-{w}")), TextSize::Xs)
      .flex()
      .w_full()
      .items_center()
      .gap_1()
      .px_4()
      .py_2()
      .font_weight(FontWeight::MEDIUM)
      .text_color(p.muted_foreground)
      .cursor_pointer()
      .hover(move |s| s.bg(muted.opacity(0.5)))
      .child(chevron(open, p))
      .child("Run history")
      .on_click(cx.listener(move |view, _, _, cx| {
        if view.detail.history_open.remove(&file) {
          cx.notify();
        } else {
          view.detail.history_open.insert(file.clone());
          view.load_history(file.clone(), cx);
        }
      }));
    let runs = open.then(|| match self.detail.history.get(filename) {
      None => div()
        .mx_4()
        .mb_2()
        .child(ui::spinner(
          SharedString::from(format!("github-history-{w}-spin")),
          12.,
          p.muted_foreground,
        ))
        .into_any_element(),
      Some(runs) => div()
        .children(runs.iter().enumerate().map(|(i, run)| {
          let open = self.detail.runs_open.contains(&run.id);
          let run_id = run.id;
          let jobs: Vec<String> = run
            .jobs
            .iter()
            .filter(|j| j.has_logs)
            .map(|j| j.job_id.clone())
            .collect();
          div()
            .when(i > 0, |d| d.border_t_1().border_color(p.border))
            .child(
              ui::text(
                named(div(), format!("run-history-item-{run_id}")),
                TextSize::Sm,
              )
              .flex()
              .w_full()
              .items_center()
              .gap_2()
              .px_6()
              .py_1p5()
              .cursor_pointer()
              .hover(move |s| s.bg(muted.opacity(0.5)))
              .child(chevron(open, p))
              .child(status_icon(
                format!("github-run-{run_id}-status"),
                Some(&run.status),
                14.,
                p,
              ))
              .child(
                ui::text(div(), TextSize::Xs)
                  .font_family(theme::MONO_FONT)
                  .child(format!("#{run_id}")),
              )
              .child(
                ui::text(div(), TextSize::Xs)
                  .text_color(p.muted_foreground)
                  .child(time::run_time(&run.started_at)),
              )
              .on_click(cx.listener(move |view, _, _, cx| {
                if view.detail.runs_open.remove(&run_id) {
                  cx.notify();
                  return;
                }
                view.detail.runs_open.insert(run_id);
                for job in &jobs {
                  if !view.detail.logs.contains_key(&(run_id, job.clone())) {
                    view.load_logs(run_id, job.clone(), cx);
                  }
                }
                cx.notify();
              })),
            )
            .when(open, |d| {
              d.child(
                div()
                  .flex()
                  .flex_col()
                  .gap_2()
                  .px_6()
                  .pb_3()
                  .children(run.jobs.iter().map(|job| self.job_logs(run_id, job, p))),
              )
            })
        }))
        .into_any_element(),
    });
    div()
      .border_t_1()
      .border_color(p.border)
      .child(toggle)
      .children(runs)
  }

  /// `RunJobLogs`.
  fn job_logs(
    &self,
    run_id: i64,
    job: &crate::core::RunJobSummary,
    p: &Palette,
  ) -> impl IntoElement {
    let body = if !job.has_logs {
      ui::text(div(), TextSize::Xs)
        .text_color(p.muted_foreground)
        .child("No logs recorded.")
        .into_any_element()
    } else {
      match self.detail.logs.get(&(run_id, job.job_id.clone())) {
        None => ui::spinner(
          SharedString::from(format!("github-logs-{run_id}-{}-spin", job.job_id)),
          12.,
          p.muted_foreground,
        )
        .into_any_element(),
        Some(Err(err)) => ui::text(div(), TextSize::Xs)
          .text_color(p.destructive)
          .child(err.clone())
          .into_any_element(),
        Some(Ok(lines)) => {
          let text = if lines.is_empty() {
            "(empty)".to_string()
          } else {
            log_text(lines)
          };
          ui::text(
            div().id(SharedString::from(format!(
              "github-logs-{run_id}-{}",
              job.job_id
            ))),
            TextSize::Xs,
          )
          .max_h(zpx(256.))
          .overflow_y_scroll()
          .rounded(radius::SM)
          .bg(p.muted)
          .p_2()
          .font_family(theme::MONO_FONT)
          .child(text)
          .into_any_element()
        }
      }
    };
    named(div(), format!("run-job-logs-{run_id}-{}", job.job_id))
      .child(
        ui::text(div(), TextSize::Xs)
          .mb_1()
          .flex()
          .items_center()
          .gap_2()
          .font_weight(FontWeight::MEDIUM)
          .child(status_icon(
            format!("github-logs-{run_id}-{}-status", job.job_id),
            Some(&job.status),
            14.,
            p,
          ))
          .child(job.job_id.clone()),
      )
      .child(body)
  }
}

/// A job log as text, stderr lines marked.
fn log_text(lines: &[data::LogLine]) -> String {
  lines
    .iter()
    .map(|l| {
      if l.stream == "stderr" {
        format!("[stderr] {}", l.body)
      } else {
        l.body.clone()
      }
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn chevron(open: bool, p: &Palette) -> impl IntoElement {
  ui::icon(
    if open {
      theme::ICON_CHEVRON_DOWN
    } else {
      theme::ICON_CHEVRON_RIGHT
    },
    12.,
    p.muted_foreground,
  )
}

/// `ChecksStatusIcon`.
fn status_icon(
  id: impl Into<SharedString>,
  status: Option<&str>,
  size: f32,
  p: &Palette,
) -> gpui::AnyElement {
  match status {
    Some("passed") => {
      ui::icon(theme::ICON_CIRCLE_CHECK, size, Tone::Success.color(p)).into_any_element()
    }
    Some("failed") => {
      ui::icon(theme::ICON_CIRCLE_X, size, Tone::Danger.color(p)).into_any_element()
    }
    Some("running" | "pending") => {
      ui::spinner(gpui::ElementId::Name(id.into()), size, p.muted_foreground).into_any_element()
    }
    _ => ui::icon(theme::ICON_CIRCLE_DOT, size, p.muted_foreground).into_any_element(),
  }
}

/// `RunButton`: Run or Re-run, with a spinner while running.
fn run_button(
  id: String,
  name: &str,
  rerun: bool,
  running: bool,
  disabled: bool,
  variant: Variant,
  p: &Palette,
) -> Button {
  let icon = if running {
    ui::spinner(SharedString::from(format!("{id}-spin")), 12., p.foreground).into_any_element()
  } else if rerun {
    ui::icon(theme::ICON_ROTATE_CW, 12., p.foreground).into_any_element()
  } else {
    ui::icon(theme::ICON_PLAY, 12., p.foreground).into_any_element()
  };
  let verb = if rerun { "Re-run" } else { "Run" };
  Button::new(id, p)
    .aria_label(format!("{verb} {name}"))
    .variant(variant)
    .size(Size::Sm)
    .gap_0()
    .disabled(disabled || running)
    .child(icon)
    .child(div().ml_1().child(if rerun { "Re-run" } else { "Run" }))
}

impl Reacts for GithubView {
  fn reaction_picker(&mut self) -> &mut Option<String> {
    &mut self.reaction_picker
  }

  fn react(&mut self, id: &str, content: &'static str, cx: &mut Context<Self>) {
    let Some(comment) = self.comment_mut(id) else {
      return;
    };
    let on = reactions::toggle(&mut comment.reactions, content);
    cx.notify();
    let id = id.to_string();
    self.run(
      cx,
      {
        let id = id.clone();
        move |host| host.react(&id, content, on)
      },
      move |view, result, _| {
        if let Err(err) = result {
          if let Some(comment) = view.comment_mut(&id) {
            reactions::toggle(&mut comment.reactions, content);
          }
          view.toast(ToastTone::Error, "Failed to react", err);
        }
      },
    );
  }
}
