//! `NewPrDialog`: a pull request from a workspace (`CreatePrFromWorkspaceForm`)
//! or from any branch (`CreatePrForm`).

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::{zpx, DismissExt as _};
use tauri_plugin_gpui::gpui::{
  deferred, div, prelude::*, AnyElement, Context, Div, FontWeight, SharedString, Window,
};

use super::{
  parts::{self, named},
  GithubView, Popover,
};
use crate::{
  gpui_shell::{
    form,
    native::ToastTone,
    screen::github_data as data,
    theme::{self, Palette},
    ui::{self, Button, Size, TextSize, Variant},
  },
  local_db::Workspace,
};

/// `PR_TYPES`: the split button's label, and its menu item.
const PR_TYPES: [(bool, &str, &str, &str); 2] = [
  (
    false,
    "Create Pull Request",
    "Create pull request",
    "Open a pull request that is ready for review",
  ),
  (
    true,
    "Draft Pull Request",
    "Create draft pull request",
    "Cannot be merged until marked ready for review",
  ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gpui_shell::screen) enum NewPrTab {
  Workspace,
  Branch,
}

/// `WorkspacePrFields`, for the picked workspace.
#[derive(Debug, Clone, Default)]
pub(in crate::gpui_shell::screen) struct WorkspaceFields {
  pub title: String,
  pub body: String,
  /// `None` until edited, so the base follows the default branch.
  pub base: Option<String>,
}

/// `CreatePrForm`'s fields.
#[derive(Debug, Clone, Default)]
pub(in crate::gpui_shell::screen) struct BranchFields {
  pub title: String,
  pub body: String,
  pub head: String,
  pub base: Option<String>,
}

/// The dialog's state while React keeps it open.
pub(in crate::gpui_shell::screen) struct NewPr {
  pub tab: NewPrTab,
  /// The workspaces that can take a PR, once read.
  pub workspaces: Option<Vec<Workspace>>,
  pub default_branch: Option<String>,
  pub selected: Option<i64>,
  pub fields: WorkspaceFields,
  pub branch: BranchFields,
  pub draft: bool,
  pub pending: bool,
  /// The branch form's last failure.
  pub error: Option<String>,
}

impl NewPr {
  pub fn set_field(&mut self, field: &str, value: String) {
    match field {
      "github.newPr.title" => self.fields.title = value,
      "github.newPr.base" => self.fields.base = Some(value),
      "github.newPr.body" => self.fields.body = value,
      "github.newPr.branch.title" => self.branch.title = value,
      "github.newPr.branch.head" => self.branch.head = value,
      "github.newPr.branch.base" => self.branch.base = Some(value),
      "github.newPr.branch.body" => self.branch.body = value,
      _ => {}
    }
  }

  fn default_base(&self) -> String {
    self.default_branch.clone().unwrap_or_default()
  }

  /// The picked workspace's base: as edited, else its target or the default branch.
  pub fn workspace_base(&self) -> String {
    if let Some(base) = &self.fields.base {
      return base.clone();
    }
    self
      .picked()
      .and_then(|ws| ws.target_branch.clone())
      .unwrap_or_else(|| self.default_base())
  }

  pub fn branch_base(&self) -> String {
    self
      .branch
      .base
      .clone()
      .unwrap_or_else(|| self.default_base())
  }

  pub fn picked(&self) -> Option<&Workspace> {
    let id = self.selected?;
    self.workspaces.as_ref()?.iter().find(|ws| ws.id == id)
  }
}

impl GithubView {
  pub(super) fn open_new_pr(&mut self, cx: &mut Context<Self>) {
    self.new_pr = Some(NewPr {
      tab: NewPrTab::Workspace,
      workspaces: None,
      default_branch: None,
      selected: None,
      fields: WorkspaceFields::default(),
      branch: BranchFields::default(),
      draft: false,
      pending: false,
      error: None,
    });
    let repo_path = self.route.repo_path.clone();
    self.run(
      cx,
      move |host| {
        let workspaces = host.workspaces(&repo_path).unwrap_or_else(|err| {
          log::warn!("github panel: workspaces: {err}");
          Vec::new()
        });
        let statuses = host.pr_statuses(&repo_path);
        // When the default branch cannot be read, every workspace shows and
        // the base is typed.
        let default = host.default_branch(&repo_path).ok();
        let offered = data::workspaces_without_open_pr(workspaces, &statuses, default.as_deref());
        (offered, default)
      },
      |view, (offered, default), _| {
        if let Some(new_pr) = &mut view.new_pr {
          new_pr.workspaces = Some(offered);
          new_pr.default_branch = default;
        }
      },
    );
  }

  fn cancel_new_pr(&mut self, cx: &mut Context<Self>) {
    self.press("github.close", cx);
  }

  fn pick_workspace(&mut self, id: i64, cx: &mut Context<Self>) {
    let Some(new_pr) = &mut self.new_pr else {
      return;
    };
    if new_pr.selected == Some(id) {
      return;
    }
    new_pr.selected = Some(id);
    // `WorkspacePrFields` is keyed by workspace: it starts over.
    let ws = new_pr.picked().cloned();
    new_pr.fields = WorkspaceFields {
      title: ws
        .as_ref()
        .map(|ws| data::conventional_pr_title(&ws.title, &ws.branch_name))
        .unwrap_or_default(),
      body: ws.and_then(|ws| ws.description).unwrap_or_default(),
      base: None,
    };
    new_pr.draft = false;
    for field in [
      "github.newPr.title",
      "github.newPr.base",
      "github.newPr.body",
    ] {
      self.reset_field(field);
    }
    cx.notify();
  }

  /// `WorkspacePrFields`' create: push, then `gh pr create`.
  fn create_workspace_pr(&mut self, cx: &mut Context<Self>) {
    let Some(repo) = self.repo() else {
      return;
    };
    let Some(new_pr) = &mut self.new_pr else {
      return;
    };
    let Some(ws) = new_pr.picked().cloned() else {
      return;
    };
    let (title, body, base, draft) = (
      new_pr.fields.title.clone(),
      new_pr.fields.body.clone(),
      new_pr.workspace_base(),
      new_pr.draft,
    );
    if new_pr.pending || title.trim().is_empty() || base.trim().is_empty() {
      return;
    }
    new_pr.pending = true;
    let repo_path = self.route.repo_path.clone();
    let branch = ws.branch_name.clone();
    self.run(
      cx,
      move |host| {
        // gh needs the branch on the remote; an in-sync push does nothing.
        host.push_workspace(&repo_path, ws.id)?;
        let number = host.create_pr(&repo, &title, &body, &base, &branch, draft)?;
        // Pending until the PR is cached, or a second click makes another.
        host.refresh_pr_status(&repo_path, &branch);
        Ok((repo, number))
      },
      move |view, result: Result<(String, u64), String>, cx| {
        if let Some(new_pr) = &mut view.new_pr {
          new_pr.pending = false;
        }
        match result {
          Ok((repo, number)) => {
            view
              .host
              .notify(crate::gpui_shell::native::NativeEvent::WorkspaceChanged {
                repo_path: view.route.repo_path.clone(),
              });
            view.pr_created_toast(&repo, number, draft);
            view.reload_list(false, cx);
            view.press(format!("github.created:{number}"), cx);
          }
          Err(err) => view.toast(ToastTone::Error, "Failed to create PR", err),
        }
      },
    );
  }

  /// `CreatePrForm`'s create.
  fn create_branch_pr(&mut self, cx: &mut Context<Self>) {
    let Some(repo) = self.repo() else {
      return;
    };
    let Some(new_pr) = &mut self.new_pr else {
      return;
    };
    let f = new_pr.branch.clone();
    let (base, draft) = (new_pr.branch_base(), new_pr.draft);
    if new_pr.pending
      || f.title.trim().is_empty()
      || f.head.trim().is_empty()
      || base.trim().is_empty()
    {
      return;
    }
    new_pr.pending = true;
    new_pr.error = None;
    self.run(
      cx,
      move |host| {
        host
          .create_pr(&repo, &f.title, &f.body, &base, &f.head, draft)
          .map(|n| (n, f.head))
      },
      |view, result, cx| {
        if let Some(new_pr) = &mut view.new_pr {
          new_pr.pending = false;
        }
        match result {
          Ok((number, head)) => {
            view.reload_list(false, cx);
            view.pr_status_changed(&head);
            view.press(format!("github.created:{number}"), cx);
          }
          Err(err) => {
            if let Some(new_pr) = &mut view.new_pr {
              new_pr.error = Some(err);
            }
          }
        }
      },
    );
  }

  pub(super) fn new_pr_dialog(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let tab = self.new_pr.as_ref().map_or(NewPrTab::Workspace, |n| n.tab);
    let view = cx.weak_entity();
    let tabs = ui::Tabs::new("github-new-pr-tabs", p)
      .tab(ui::Tab::new(
        "github-new-pr-tab-workspace",
        "From workspace",
      ))
      .tab(ui::Tab::new("github-new-pr-tab-branch", "From branch"))
      .selected(match tab {
        NewPrTab::Workspace => 0,
        NewPrTab::Branch => 1,
      })
      .on_select(move |ix, _, cx| {
        view
          .update(cx, |view, cx| {
            if let Some(new_pr) = &mut view.new_pr {
              new_pr.tab = [NewPrTab::Workspace, NewPrTab::Branch][ix];
            }
            view.popover = None;
            cx.notify();
          })
          .ok();
      });
    let workspace = tab == NewPrTab::Workspace;
    let (content, submit) = match tab {
      NewPrTab::Workspace => self.workspace_form(p, window, cx),
      NewPrTab::Branch => self.branch_form(p, window, cx),
    };
    let cancel = Button::new("github-new-pr-cancel", p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .label("Cancel")
      .on_click(cx.listener(|view, _, _, cx| view.cancel_new_pr(cx)))
      .into_any_element();
    let can_create = submit.is_some_and(|(_, pending, disabled)| !pending && !disabled);
    let footer = match submit {
      Some((draft, pending, disabled)) => vec![
        cancel,
        self
          .split_button(draft, pending, disabled, workspace, p, cx)
          .into_any_element(),
      ],
      None => vec![cancel],
    };
    ui::Dialog::new("github-new-pr-dialog", p)
      .overlay(window.viewport_size())
      .width(576.)
      .title("New Pull Request")
      .focus(self.dialog_focus())
      // Escape closes the open menu first.
      .on_close(ui::handler(cx, |view, _, cx| {
        if view.popover.take().is_some() {
          cx.notify();
        } else {
          view.cancel_new_pr(cx);
        }
      }))
      .on_primary(ui::handler(cx, move |view, _, cx| {
        if workspace {
          view.create_workspace_pr(cx)
        } else {
          view.create_branch_pr(cx)
        }
      }))
      .primary_disabled(!can_create)
      .child(
        div()
          .child(div().flex().child(tabs))
          .child(div().pt_4().child(content)),
      )
      .footer(footer)
  }

  /// The form, and the create button's draft, pending and disabled states
  /// once there is something to create.
  fn workspace_form(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> (AnyElement, Option<(bool, bool, bool)>) {
    let Some(new_pr) = self.new_pr.as_ref() else {
      return (div().into_any_element(), None);
    };
    let default = new_pr.default_base();
    let selected = new_pr.selected;
    let list = match &new_pr.workspaces {
      None => muted_line("Loading workspaces…", p).into_any_element(),
      Some(list) if list.is_empty() => {
        muted_line("Every workspace already has an open pull request.", p).into_any_element()
      }
      Some(list) => div()
        .id("github-new-pr-workspaces")
        .max_h(zpx(240.))
        .overflow_y_scroll()
        .rounded(radius::MD)
        .border_1()
        .border_color(p.border)
        .children(list.iter().enumerate().map(|(i, ws)| {
          let id = ws.id;
          let picked = selected == Some(id);
          named(div(), format!("github-new-pr-ws-{id}"))
            .w_full()
            .px_3()
            .py_2()
            .when(i + 1 < list.len(), |d| {
              d.border_b_1().border_color(p.border)
            })
            .cursor_pointer()
            .when(picked, |d| d.bg(p.selected()))
            .when(!picked, |d| d.hover(move |s| s.bg(p.hover())))
            .on_click(cx.listener(move |view, _, _, cx| view.pick_workspace(id, cx)))
            .child(
              ui::text(div(), TextSize::Sm)
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .child(if ws.title.is_empty() {
                  ws.branch_name.clone()
                } else {
                  ws.title.clone()
                }),
            )
            .child(
              ui::text(div(), TextSize::Sm)
                .flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .text_color(p.muted_foreground)
                .font_family(theme::MONO_FONT)
                .child(ui::icon(theme::ICON_BRANCH_BOLD, 12., p.muted_foreground))
                .child(div().truncate().child(ws.branch_name.clone()))
                .child(div().flex_none().child("→"))
                .child(
                  div()
                    .truncate()
                    .child(ws.target_branch.clone().unwrap_or_else(|| default.clone())),
                ),
            )
        }))
        .into_any_element(),
    };
    let fields = new_pr.picked().is_some().then(|| {
      let f = new_pr.fields.clone();
      let base = new_pr.workspace_base();
      (f, base, new_pr.draft, new_pr.pending)
    });
    let mut submit = None;
    let rest = match fields {
      None => div().into_any_element(),
      Some((f, base, draft, pending)) => {
        let title = self.input("github.newPr.title", &f.title, "Title", window, cx);
        let base_state = self.input("github.newPr.base", &base, "Base branch", window, cx);
        let body = self.textarea(
          "github.newPr.body",
          &f.body,
          "Description (optional)",
          4,
          window,
          cx,
        );
        let disabled = f.title.trim().is_empty() || base.trim().is_empty();
        submit = Some((draft, pending, disabled));
        div()
          .flex()
          .flex_col()
          .gap_3()
          .child(field(form::input_field(
            "github-new-pr-title",
            &title,
            false,
            false,
          )))
          .child(field(form::input_field(
            "github-new-pr-base",
            &base_state,
            true,
            false,
          )))
          .child(field(form::textarea_field(&body, false, false)))
          .into_any_element()
      }
    };
    (
      div()
        .flex()
        .flex_col()
        .gap_3()
        .child(list)
        .child(rest)
        .into_any_element(),
      submit,
    )
  }

  fn branch_form(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> (AnyElement, Option<(bool, bool, bool)>) {
    let Some(new_pr) = self.new_pr.as_ref() else {
      return (div().into_any_element(), None);
    };
    let (f, base, draft, pending, error) = (
      new_pr.branch.clone(),
      new_pr.branch_base(),
      new_pr.draft,
      new_pr.pending,
      new_pr.error.clone(),
    );
    let title = self.input("github.newPr.branch.title", &f.title, "Title", window, cx);
    let head = self.input(
      "github.newPr.branch.head",
      &f.head,
      "Head branch",
      window,
      cx,
    );
    let base_state = self.input("github.newPr.branch.base", &base, "Base branch", window, cx);
    let body = self.textarea(
      "github.newPr.branch.body",
      &f.body,
      "Description (optional)",
      4,
      window,
      cx,
    );
    let disabled = f.title.trim().is_empty() || f.head.trim().is_empty() || base.trim().is_empty();
    let content = div()
      .flex()
      .flex_col()
      .gap_3()
      .child(field(form::input_field(
        "github-new-pr-branch-title",
        &title,
        false,
        false,
      )))
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(div().flex_1().child(field(form::input_field(
            "github-new-pr-branch-head",
            &head,
            true,
            false,
          ))))
          .child(
            ui::text(div(), TextSize::Sm)
              .flex_none()
              .text_color(p.muted_foreground)
              .child("→"),
          )
          .child(div().flex_1().child(field(form::input_field(
            "github-new-pr-branch-base",
            &base_state,
            true,
            false,
          )))),
      )
      .child(field(form::textarea_field(&body, false, false)))
      .children(error.map(|err| {
        ui::text(div(), TextSize::Sm)
          .text_color(p.destructive)
          .child(err)
      }));
    (content.into_any_element(), Some((draft, pending, disabled)))
  }

  /// `PrTypeSplitButton`: the main part creates; the menu picks draft or ready.
  fn split_button(
    &self,
    draft: bool,
    pending: bool,
    disabled: bool,
    workspace: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let label = PR_TYPES.iter().find(|t| t.0 == draft).map_or("", |t| t.1);
    let create = parts::busy_spinner(
      Button::new("github-new-pr-create", p)
        .size(Size::Sm)
        .gap_1()
        .rounded_r_none()
        .disabled(disabled || pending),
      "github-new-pr-create",
      pending,
      p.primary_foreground,
    )
    .label(label)
    .on_click(cx.listener(move |view, _, _, cx| {
      if workspace {
        view.create_workspace_pr(cx)
      } else {
        view.create_branch_pr(cx)
      }
    }));
    let menu_button = Button::new("github-new-pr-type", p)
      .aria_label("Pull request type")
      .size(Size::Sm)
      .rounded_l_none()
      .border_l_1()
      .border_color(p.primary_foreground.opacity(0.2))
      .px_1p5()
      .disabled(pending)
      .icon(theme::ICON_CHEVRON_DOWN, 16.)
      .on_click(cx.listener(|view, _, _, cx| view.toggle_popover(Popover::PrType, cx)));
    let menu = (self.popover == Some(Popover::PrType) && !pending).then(|| {
      deferred(
        parts::menu_panel("github-new-pr-type-menu", p)
          .absolute()
          .top(zpx(36.))
          .left_0()
          .w(zpx(320.))
          .dismiss_with("github-pr-type", cx, |view, cx| {
            view.close_popover(Popover::PrType, cx)
          })
          .children(PR_TYPES.iter().map(|&(is_draft, _, item, description)| {
            ui::text(
              named(
                div(),
                SharedString::from(if is_draft {
                  "github-new-pr-type-draft"
                } else {
                  "github-new-pr-type-ready"
                }),
              ),
              TextSize::Sm,
            )
            .relative()
            .flex()
            .flex_col()
            .gap_0p5()
            .py_1p5()
            .pl_8()
            .pr_2()
            .rounded(radius::SM)
            .cursor_pointer()
            .hover(move |s| s.bg(p.hover()))
            .child(
              div()
                .absolute()
                .left_2()
                .top(zpx(8.))
                .size(zpx(14.))
                .flex()
                .items_center()
                .justify_center()
                .when(is_draft == draft, |d| {
                  d.child(div().size_2().rounded_full().bg(p.popover_foreground))
                }),
            )
            .child(div().font_weight(FontWeight::MEDIUM).child(item))
            .child(
              ui::text(div(), TextSize::Xs)
                .text_color(p.muted_foreground)
                .child(description),
            )
            .on_click(cx.listener(move |view, _, _, cx| {
              if let Some(new_pr) = &mut view.new_pr {
                new_pr.draft = is_draft;
              }
              view.popover = None;
              cx.notify();
            }))
          })),
      )
      .with_priority(3)
    });
    div()
      .relative()
      .flex()
      .items_center()
      .child(create)
      .child(menu_button)
      .children(menu)
  }
}

fn field(el: impl IntoElement) -> Div {
  ui::text(div(), TextSize::Sm).child(el)
}

fn muted_line(text: &str, p: &Palette) -> Div {
  ui::text(div(), TextSize::Sm)
    .text_color(p.muted_foreground)
    .child(text.to_string())
}
