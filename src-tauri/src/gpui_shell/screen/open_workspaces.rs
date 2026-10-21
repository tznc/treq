//! The sidebar's Open Workspaces dialog: opens a workspace for each pasted
//! branch name or GitHub pull request or branch link, and for each pull
//! request picked from the list beside it, all at once.

use crate::gpui_shell::ui::zpx;
use std::{
  collections::{HashMap, HashSet},
  sync::Arc,
  time::Duration,
};

use gpui_kit::{
  base::TestSupportExt as _,
  component::input::{Input, InputEvent, InputState, Textarea, TextareaState},
};
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, Context, Div, Entity, FocusHandle,
  FontWeight, KeyDownEvent, MouseButton, SharedString, Subscription, Task, Window,
};

use super::{
  github::parts,
  open_workspaces_data::{self as data, Checked, Line, OpenWorkspacesHost, Route, Target},
  workspace_data::WorkspaceHost,
  workspace_modal::{
    checkbox, dialog_description, dialog_header, dialog_panel, label_xs, press, Check,
  },
  ActionSink, ScreenAction,
};
use crate::{
  github::GhPullRequest,
  gpui_shell::{
    native::{NativeEvent, ToastTone},
    theme::{self, Palette},
    ui::{self, Button, TextSize, Variant},
  },
};

/// A workspace's id and whether it was made, or why it could not be.
type Opened = Result<(i64, bool), String>;

/// How long typing pauses before the lines are checked or the pull
/// requests searched.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// The pull request list beside the lines.
#[derive(Default)]
pub(super) struct PrList {
  pub(super) loading: bool,
  pub(super) error: Option<String>,
  pub(super) items: Vec<GhPullRequest>,
  /// The list holds only the user's own pull requests.
  pub(super) mine: bool,
  /// Bumped per load, so an older answer is dropped.
  generation: u64,
}

struct Fields {
  lines: Entity<TextareaState>,
  search: Entity<InputState>,
}

pub struct OpenWorkspacesDialog {
  route: Route,
  host: Arc<dyn OpenWorkspacesHost>,
  sink: ActionSink,
  dark: bool,
  focus: FocusHandle,
  fields: Option<Fields>,
  _subscriptions: Vec<Subscription>,
  pub(super) text: String,
  /// Each repository's `owner/name`, once read.
  remotes: Option<Vec<(String, String)>>,
  /// The repositories have been fetched, so their branches can be checked.
  fetched: bool,
  /// Each line's check, by its text.
  pub(super) checks: HashMap<String, Checked>,
  checking: bool,
  _check: Option<Task<()>>,
  /// The repository on screen's `owner/name`: the pull request list shows
  /// only for a GitHub repository.
  pub(super) github: Option<String>,
  pub(super) prs: PrList,
  pub(super) query: String,
  _search: Option<Task<()>>,
  /// The picked pull requests, in the order picked.
  pub(super) chosen: Vec<GhPullRequest>,
  /// Branches of the repository on screen that have a workspace.
  open_branches: HashSet<String>,
  pub(super) pending: bool,
  /// What failed in the last open, a line per workspace.
  pub(super) errors: Vec<String>,
  pub(super) dismissed: bool,
}

impl OpenWorkspacesDialog {
  pub fn new(
    route: Route,
    host: Arc<dyn OpenWorkspacesHost>,
    sink: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let mut view = Self {
      route,
      host,
      sink,
      dark,
      focus: cx.focus_handle(),
      fields: None,
      _subscriptions: Vec::new(),
      text: String::new(),
      remotes: None,
      fetched: false,
      checks: HashMap::new(),
      checking: false,
      _check: None,
      github: None,
      prs: PrList::default(),
      query: String::new(),
      _search: None,
      chosen: Vec::new(),
      open_branches: HashSet::new(),
      pending: false,
      errors: Vec::new(),
      dismissed: false,
    };
    view.load(cx);
    view
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  /// Reads the remotes, then lists pull requests and fetches, so pasted
  /// branches check against the remote's latest.
  fn load(&mut self, cx: &mut Context<Self>) {
    let (host, route) = (self.host.clone(), self.route.clone());
    cx.spawn(async move |this, cx| {
      let (remotes, open) = cx
        .background_spawn({
          let (host, route) = (host.clone(), route.clone());
          async move {
            let open: HashSet<String> =
              super::github_data::GithubHost::workspaces(&*host, &route.repo_path)
                .unwrap_or_default()
                .into_iter()
                .map(|w| w.branch_name)
                .collect();
            (data::remotes(&*host, &route.repos), open)
          }
        })
        .await;
      let _ = this.update(cx, |view, cx| {
        view.github = remotes
          .iter()
          .find(|(path, _)| *path == view.route.repo_path)
          .map(|(_, name)| name.clone());
        view.remotes = Some(remotes);
        view.open_branches = open;
        view.load_prs(cx);
        cx.notify();
      });
      cx.background_spawn(async move {
        for repo in &route.repos {
          host.fetch(repo);
        }
      })
      .await;
      let _ = this.update(cx, |view, cx| {
        view.fetched = true;
        view.check_lines(cx);
      });
    })
    .detach();
  }

  /// The user's open pull requests, or everyone's when they have none; with
  /// a search, everyone's that match.
  fn load_prs(&mut self, cx: &mut Context<Self>) {
    let Some(repo) = self.github.clone() else {
      return;
    };
    self.prs.generation += 1;
    let generation = self.prs.generation;
    self.prs.loading = true;
    self.prs.error = None;
    let (host, query) = (self.host.clone(), self.query.trim().to_string());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move {
          if !query.is_empty() {
            return host.open_prs(&repo, false, &query).map(|prs| (prs, false));
          }
          match host.open_prs(&repo, true, "")? {
            mine if mine.is_empty() => host.open_prs(&repo, false, "").map(|prs| (prs, false)),
            mine => Ok((mine, true)),
          }
        })
        .await;
      let _ = this.update(cx, |view, cx| {
        if view.prs.generation != generation {
          return;
        }
        view.prs.loading = false;
        match result {
          Ok((items, mine)) => {
            view.prs.items = items;
            view.prs.mine = mine;
          }
          Err(err) => view.prs.error = Some(err),
        }
        cx.notify();
      });
    })
    .detach();
    cx.notify();
  }

  fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
    if query == self.query {
      return;
    }
    self.query = query;
    self._search = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(DEBOUNCE).await;
      let _ = this.update(cx, |view, cx| view.load_prs(cx));
    }));
  }

  fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
    self.text = text;
    self.errors.clear();
    cx.notify();
    self._check = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(DEBOUNCE).await;
      let _ = this.update(cx, |view, cx| view.check_lines(cx));
    }));
  }

  /// Checks the lines not checked yet.
  fn check_lines(&mut self, cx: &mut Context<Self>) {
    let Some(remotes) = self.remotes.clone().filter(|_| self.fetched) else {
      return;
    };
    let unchecked: Vec<(String, Line)> = data::parse_lines(&self.text)
      .into_iter()
      .filter(|(raw, _)| !self.checks.contains_key(raw))
      .collect();
    if unchecked.is_empty() {
      return;
    }
    self.checking = true;
    cx.notify();
    let (host, repo) = (self.host.clone(), self.route.repo_path.clone());
    cx.spawn(async move |this, cx| {
      let checked = cx
        .background_spawn(async move {
          unchecked
            .into_iter()
            .map(|(raw, line)| {
              let checked = data::check(&*host, &repo, &remotes, &line);
              (raw, checked)
            })
            .collect::<Vec<_>>()
        })
        .await;
      let _ = this.update(cx, |view, cx| {
        view.checks.extend(checked);
        view.checking = false;
        // Lines typed while checking.
        view.check_lines(cx);
        cx.notify();
      });
    })
    .detach();
  }

  /// Each line and its check; `None` while it is checked.
  pub(super) fn lines(&self) -> Vec<(String, Option<&Checked>)> {
    data::parse_lines(&self.text)
      .into_iter()
      .map(|(raw, _)| {
        let checked = self.checks.get(&raw);
        (raw, checked)
      })
      .collect()
  }

  /// The workspaces the dialog will open: the lines', then the picked pull
  /// requests', each branch once.
  pub(super) fn targets(&self) -> Vec<Target> {
    let mut seen = HashSet::new();
    let lines = self.lines();
    let from_lines = lines
      .iter()
      .filter_map(|(_, checked)| checked.and_then(|c| c.as_ref().ok()).cloned());
    let from_prs = self.chosen.iter().map(|pr| {
      let existing = self.open_branches.contains(&pr.head_ref_name);
      Target::from_pr(&self.route.repo_path, pr, existing)
    });
    from_lines
      .chain(from_prs)
      .filter(|t| seen.insert((t.repo_path.clone(), t.branch.clone())))
      .collect()
  }

  /// Lines still to check.
  fn unchecked(&self) -> usize {
    self.lines().iter().filter(|(_, c)| c.is_none()).count()
  }

  pub(super) fn can_submit(&self) -> bool {
    !self.pending && self.unchecked() == 0 && !self.targets().is_empty()
  }

  fn toggle_pr(&mut self, pr: &GhPullRequest, cx: &mut Context<Self>) {
    match self.chosen.iter().position(|c| c.number == pr.number) {
      Some(at) => {
        self.chosen.remove(at);
      }
      None => self.chosen.push(pr.clone()),
    }
    self.errors.clear();
    cx.notify();
  }

  fn close(&mut self, cx: &mut Context<Self>) {
    self.dismissed = true;
    press(&self.sink, "openWorkspaces.close", cx);
    cx.notify();
  }

  fn toast(&self, tone: ToastTone, title: &str, description: String) {
    WorkspaceHost::notify(
      &*self.host,
      NativeEvent::Toast {
        title: title.into(),
        description,
        tone,
        action: None,
      },
    );
  }

  /// Opens every target, a repository at a time.
  fn submit(&mut self, cx: &mut Context<Self>) {
    if !self.can_submit() {
      return;
    }
    let targets = self.targets();
    self.pending = true;
    self.errors.clear();
    cx.notify();
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let results = cx
        .background_spawn({
          let (host, targets) = (host.clone(), targets.clone());
          async move {
            let mut repos: Vec<&str> = Vec::new();
            for target in &targets {
              if !repos.contains(&target.repo_path.as_str()) {
                repos.push(&target.repo_path);
              }
            }
            let mut results = Vec::new();
            for repo in repos {
              let batch: Vec<&Target> = targets.iter().filter(|t| t.repo_path == repo).collect();
              let requests: Vec<_> = batch.iter().map(|t| t.request()).collect();
              let opened = host.open_workspaces(repo, &requests);
              results.extend(batch.into_iter().cloned().zip(opened));
            }
            results
          }
        })
        .await;
      let _ = this.update(cx, |view, cx| view.opened(results, cx));
    })
    .detach();
  }

  fn opened(&mut self, results: Vec<(Target, Opened)>, cx: &mut Context<Self>) {
    self.pending = false;
    let mut repos: Vec<String> = Vec::new();
    let (mut created, mut reused, mut first) = (0, 0, None);
    for (target, result) in &results {
      match result {
        Ok((id, made)) => {
          if *made {
            created += 1;
          } else {
            reused += 1;
          }
          first.get_or_insert((target.repo_path.clone(), *id));
          if !repos.contains(&target.repo_path) {
            repos.push(target.repo_path.clone());
          }
        }
        Err(err) => self.errors.push(format!("{}: {err}", target.branch)),
      }
    }
    for repo_path in repos {
      WorkspaceHost::notify(&*self.host, NativeEvent::WorkspaceChanged { repo_path });
    }
    let opened = summary(created, reused);
    if self.errors.is_empty() {
      self.toast(ToastTone::Success, "Workspaces opened", opened);
      if let Some((repo_path, id)) = first {
        (self.sink)(
          ScreenAction::Field {
            field: "openWorkspaces.opened".into(),
            value: serde_json::json!({ "repoPath": repo_path, "id": id }),
          },
          cx,
        );
      }
      self.close(cx);
    } else {
      self.toast(
        ToastTone::Error,
        "Some workspaces could not be opened",
        format!("{} failed. {opened}", self.errors.len()),
      );
      // What opened is open now, and a retry should say so.
      self.checks.clear();
      self.open_branches.extend(
        results
          .iter()
          .filter(|(t, r)| r.is_ok() && t.repo_path == self.route.repo_path)
          .map(|(t, _)| t.branch.clone()),
      );
      self.check_lines(cx);
    }
    cx.notify();
  }

  fn fields(&mut self, window: &mut Window, cx: &mut Context<Self>) -> &Fields {
    if self.fields.is_none() {
      let lines = cx.new(|cx| {
        TextareaState::new(window, cx)
          .auto_grow(4, 8)
          .placeholder("feat/login\nhttps://github.com/acme/app/pull/42")
      });
      let search =
        cx.new(|cx| InputState::new(window, cx).placeholder("Search open pull requests"));
      // `autoFocus`.
      lines.update(cx, |lines, cx| lines.focus(window, cx));
      self
        ._subscriptions
        .push(cx.subscribe(&lines, |view, lines, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            let text = lines.read(cx).value().to_string();
            view.set_text(text, cx);
          }
        }));
      self._subscriptions.push(
        cx.subscribe(&search, |view, search, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            let query = search.read(cx).value().to_string();
            view.set_query(query, cx);
          }
        }),
      );
      self.fields = Some(Fields { lines, search });
    }
    self.fields.as_ref().expect("fields")
  }

  /// The lines' column: the text, what it will open, and the buttons.
  fn lines_column(
    &self,
    lines: &Entity<TextareaState>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let checks = self.lines();
    let targets = self.targets();
    let unchecked = checks.iter().filter(|(_, c)| c.is_none()).count();
    let invalid = checks
      .iter()
      .filter(|(_, c)| matches!(c, Some(Err(_))))
      .count();
    let existing = targets.iter().filter(|t| t.existing).count();
    let new = targets.len() - existing;
    let status = if unchecked > 0 {
      format!("Checking {}...", plural(unchecked, "1 line", "lines"))
    } else if targets.is_empty() {
      "Nothing to open yet.".to_string()
    } else {
      summary_future(new, existing)
    };
    div()
      .flex()
      .flex_col()
      .flex_1()
      .min_w_0()
      .gap_3()
      .child(
        div()
          .flex()
          .flex_col()
          .gap(zpx(6.))
          .child(label_xs("Branches or links"))
          .child(
            div()
              .debug_selector(|| "open-workspaces-lines".into())
              .child(ui::text(
                Textarea::new(lines)
                  .aria_label("Branches or links")
                  .disabled(self.pending),
                TextSize::Sm,
              )),
          )
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child("One per line: a branch name, or a GitHub pull request or branch link of one of your repositories."),
          ),
      )
      .child(
        div()
          .id("open-workspaces-status")
          .role(gpui::Role::Status)
          .aria_label(SharedString::from(status.clone()))
          .test_support()
          .flex()
          .items_center()
          .gap_2()
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::MEDIUM)
              .child(status),
          )
          .when(invalid > 0, |this| {
            let invalid = plural(invalid, "1 line can't be opened", "lines can't be opened");
            this.child(
              ui::text(div(), TextSize::Sm)
                .id("open-workspaces-invalid")
                .role(gpui::Role::Alert)
                .aria_label(SharedString::from(invalid.clone()))
                .test_support()
                .text_color(p.destructive)
                .child(invalid),
            )
          }),
      )
      .child(
        div()
          .id("open-workspaces-results")
          .flex()
          .flex_col()
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .gap_1()
          .children(checks.iter().enumerate().map(|(i, (raw, checked))| result_row(i, raw, *checked, &self.route.repo_path, p)))
          .children(self.chosen.iter().map(|pr| {
            let existing = self.open_branches.contains(&pr.head_ref_name);
            target_row(
              SharedString::from(format!("open-workspaces-pr-{}", pr.number)),
              &Target::from_pr(&self.route.repo_path, pr, existing),
              &self.route.repo_path,
              p,
            )
          })),
      )
      .when(!self.errors.is_empty(), |this| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "open-workspaces-errors".into())
            .flex()
            .flex_col()
            .gap_1()
            .text_color(p.destructive)
            .children(self.errors.iter().enumerate().map(|(i, error)| {
              div()
                .id(("open-workspaces-error", i))
                .role(gpui::Role::Alert)
                .aria_label(SharedString::from(error.clone()))
                .test_support()
                .child(error.clone())
            })),
        )
      })
      .child(
        div()
          .flex()
          .justify_end()
          .gap_2()
          .child(
            Button::new("open-workspaces-cancel", p)
              .variant(Variant::Outline)
              .disabled(self.pending)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.close(cx))),
          )
          .child(
            Button::new("open-workspaces-submit", p)
              .disabled(!self.can_submit())
              .label(if self.pending {
                "Opening...".to_string()
              } else {
                match targets.len() {
                  0 => "Open Workspaces".to_string(),
                  1 => "Open 1 Workspace".to_string(),
                  n => format!("Open {n} Workspaces"),
                }
              })
              .on_click(cx.listener(|view, _, _, cx| view.submit(cx))),
          ),
      )
  }

  /// The pull requests' column: a search and a list to pick from.
  fn prs_column(&self, search: &Entity<InputState>, p: &Palette, cx: &mut Context<Self>) -> Div {
    let heading = if !self.query.trim().is_empty() {
      "Matching pull requests"
    } else if self.prs.mine {
      "Your pull requests"
    } else {
      "Open pull requests"
    };
    let list: gpui::AnyElement = if self.prs.loading && self.prs.items.is_empty() {
      parts::loading("open-workspaces-prs-loading", p).into_any_element()
    } else if let Some(err) = &self.prs.error {
      parts::error_state(
        "open-workspaces-prs",
        "Could not load pull requests",
        err,
        p,
        Some(Box::new(cx.listener(|view, _, _, cx| view.load_prs(cx)))),
      )
      .into_any_element()
    } else if self.prs.items.is_empty() {
      parts::empty_state(theme::ICON_PR, "No open pull requests.", p).into_any_element()
    } else {
      div()
        .children(self.prs.items.iter().map(|pr| self.pr_row(pr, p, cx)))
        .into_any_element()
    };
    div()
      .flex()
      .flex_col()
      .w(zpx(420.))
      .flex_none()
      .min_h_0()
      .border_l_1()
      .border_color(p.border)
      .pl_4()
      .gap_2()
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .child(
            div()
              .id("open-workspaces-prs-heading")
              .role(gpui::Role::Heading)
              .aria_label(heading)
              .test_support()
              .child(label_xs(heading)),
          )
          .when(!self.chosen.is_empty(), |this| {
            this.child(
              ui::text(div(), TextSize::Xs)
                .text_color(p.muted_foreground)
                .child(format!("{} selected", self.chosen.len())),
            )
          }),
      )
      .child(
        div()
          .debug_selector(|| "open-workspaces-search".into())
          .child(ui::text(
            Styled::h(
              Input::new(search)
                .aria_label("Search pull requests")
                .prefix(ui::icon(theme::ICON_SEARCH, 14., p.muted_foreground)),
              zpx(28.),
            ),
            TextSize::Sm,
          )),
      )
      .child(
        div()
          .id("open-workspaces-prs")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .rounded_md()
          .border_1()
          .border_color(p.border)
          .child(list),
      )
  }

  /// A pull request with its checkbox: `PrListItem` without the branches,
  /// which do not fit.
  fn pr_row(&self, pr: &GhPullRequest, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let checked = self.chosen.iter().any(|c| c.number == pr.number);
    let n = pr.number;
    let muted = p.muted;
    let pr_for_click = pr.clone();
    div()
      .id(SharedString::from(format!("open-workspaces-pr-row-{n}")))
      .role(gpui::Role::CheckBox)
      .aria_label(SharedString::from(format!("#{n} {}", pr.title)))
      .aria_toggled(if checked {
        gpui::accesskit::Toggled::True
      } else {
        gpui::accesskit::Toggled::False
      })
      .test_support()
      .w_full()
      .px_3()
      .py(zpx(10.))
      .border_b_1()
      .border_color(p.border)
      .flex()
      .items_start()
      .gap_2()
      .cursor_pointer()
      .when(checked, |d| d.bg(p.primary.opacity(0.1)))
      .when(!checked, |d| d.hover(move |s| s.bg(muted.opacity(0.5))))
      .child(div().mt(zpx(4.)).child(checkbox(
        SharedString::from(format!("open-workspaces-pr-check-{n}")),
        if checked { Check::On } else { Check::Off },
        self.pending,
        p,
      )))
      .child(parts::pr_list_item(pr, false, p))
      .on_click(cx.listener(move |view, _, _, cx| {
        if !view.pending {
          view.toggle_pr(&pr_for_click, cx);
        }
      }))
  }
}

/// `n` as "1 thing" or "n things".
fn plural(n: usize, one: &str, many: &str) -> String {
  if n == 1 {
    one.to_string()
  } else {
    format!("{n} {many}")
  }
}

/// What the dialog will open, before it does.
fn summary_future(new: usize, existing: usize) -> String {
  let created = plural(
    new,
    "1 workspace will be created",
    "workspaces will be created",
  );
  match (new, existing) {
    (_, 0) => created,
    (0, _) => plural(
      existing,
      "1 workspace is already open",
      "workspaces are already open",
    ),
    _ => format!("{created}, {existing} already open"),
  }
}

/// What the dialog opened.
fn summary(created: usize, reused: usize) -> String {
  match (created, reused) {
    (c, 0) => format!("Created {}.", plural(c, "1 workspace", "workspaces")),
    (0, r) => format!(
      "Opened {}.",
      plural(r, "1 existing workspace", "existing workspaces")
    ),
    (c, r) => format!(
      "Created {}, opened {r} existing.",
      plural(c, "1 workspace", "workspaces")
    ),
  }
}

/// A pasted line and what it opens, or why it can't.
fn result_row(
  index: usize,
  raw: &str,
  checked: Option<&Checked>,
  repo_path: &str,
  p: &Palette,
) -> gpui::AnyElement {
  let id = SharedString::from(format!("open-workspaces-line-{index}"));
  match checked {
    Some(Ok(target)) => target_row(id, target, repo_path, p).into_any_element(),
    Some(Err(reason)) => row(id, format!("{raw}: {reason}"), p)
      .child(ui::icon(theme::ICON_CIRCLE_ALERT, 14., p.destructive))
      .child(mono(raw))
      .child(
        div()
          .min_w_0()
          .truncate()
          .text_color(p.destructive)
          .child(reason.clone()),
      )
      .test_support()
      .into_any_element(),
    None => row(id.clone(), format!("{raw}: checking"), p)
      .child(ui::spinner(id, 14., p.muted_foreground))
      .child(mono(raw))
      .test_support()
      .into_any_element(),
  }
}

fn target_row(id: SharedString, target: &Target, repo_path: &str, p: &Palette) -> impl IntoElement {
  let mut detail = Vec::new();
  if let Some(n) = target.pr {
    detail.push(format!("PR #{n}"));
  }
  if target.repo_path != repo_path {
    detail.push(repo_name(&target.repo_path).to_string());
  }
  detail.push(
    if target.existing {
      "already open"
    } else {
      "new"
    }
    .to_string(),
  );
  let detail = detail.join(" · ");
  row(id, format!("{}: {detail}", target.branch), p)
    .child(ui::icon(
      theme::ICON_CHECK,
      14.,
      if target.existing {
        p.muted_foreground
      } else {
        p.success
      },
    ))
    .child(mono(&target.branch))
    .child(
      div()
        .min_w_0()
        .truncate()
        .text_color(p.muted_foreground)
        .child(detail),
    )
    .test_support()
}

/// A line of the list under the text, named by its line and what it opens.
fn row(id: SharedString, name: String, p: &Palette) -> gpui::Stateful<Div> {
  let selector = id.to_string();
  ui::text(div(), TextSize::Sm)
    .id(id)
    .debug_selector(move || selector)
    .role(gpui::Role::ListItem)
    .aria_label(SharedString::from(name))
    .flex()
    .flex_none()
    .items_center()
    .gap_2()
    .min_w_0()
    .text_color(p.foreground)
}

fn mono(text: &str) -> Div {
  div()
    .flex_none()
    .max_w(zpx(260.))
    .truncate()
    .font_family(theme::MONO_FONT)
    .child(text.to_string())
}

/// A repository's folder name.
fn repo_name(path: &str) -> &str {
  path
    .trim_end_matches('/')
    .rsplit('/')
    .next()
    .unwrap_or(path)
}

impl Render for OpenWorkspacesDialog {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.dismissed {
      return div().into_any_element();
    }
    let p = theme::palette(self.dark);
    let (lines, search) = {
      let fields = self.fields(window, cx);
      (fields.lines.clone(), fields.search.clone())
    };
    let split = self.github.is_some();
    let viewport = window.viewport_size();
    let (backdrop, panel) = dialog_panel(
      "open-workspaces-dialog",
      if split { 1040. } else { 560. },
      viewport,
      &p,
    );
    let keys = cx.listener(|view, e: &KeyDownEvent, _, cx| {
      let k = &e.keystroke;
      let held = crate::gpui_shell::keys::held(k.modifiers);
      if k.key == "escape" {
        cx.stop_propagation();
        view.close(cx);
      } else if k.key == "enter" && (held.platform || held.control) {
        cx.stop_propagation();
        view.submit(cx);
      }
    });
    let body = div()
      .flex()
      .gap_4()
      .h(zpx(460.))
      .max_h(viewport.height - px(160.))
      .pt_4()
      .child(self.lines_column(&lines, &p, cx))
      .when(split, |this| this.child(self.prs_column(&search, &p, cx)));
    let panel = panel
      .child(dialog_header(
        "Open Workspaces",
        dialog_description(
          if split {
            "Open a workspace for each branch or link, and each pull request you pick."
          } else {
            "Open a workspace for each branch or link."
          },
          &p,
        ),
      ))
      .child(body);
    deferred(
      anchored().position(point(px(0.), px(0.))).child(
        backdrop
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _, _, cx| {
              if !view.pending {
                view.close(cx)
              }
            }),
          )
          .child(
            panel
              .id("open-workspaces-panel")
              .track_focus(&self.focus)
              .capture_key_down(keys)
              .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
          ),
      ),
    )
    .with_priority(2)
    .into_any_element()
  }
}
