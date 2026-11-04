//! Home: a window's repositories at a glance, in three tabs.
//!
//! - **Code**: a two-column grid of repository cards (GitHub name or folder,
//!   checked-out branch, workspaces). A card opens that repository's home.
//! - **Changes**: each repository's uncommitted changes in its home checkout,
//!   grouped by repository.
//! - **Commits**: a column per repository with its recent history, in shared
//!   time bands (one per day this week, then one per week) so work done at
//!   the same time lines up across repositories. A column opens that
//!   repository's Commits tab.

use std::{collections::HashMap, sync::Arc};

use chrono::{Datelike, Duration, Local, NaiveDate};
use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, AnyElement, Context, FontWeight, SharedString, Window,
};

use super::{workspace_dialog_data::WorkspaceDialogHost, ActionSink, ScreenAction};
use crate::gpui_shell::{
  theme, time,
  ui::{self, zpx, TextSize},
};
use crate::result_ext::ResultExt;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  /// The saved project's name; `None` for an unsaved project.
  pub name: Option<String>,
  pub repos: Vec<RepoCard>,
  /// Bumped when the window's data changes, so the tabs read again.
  #[serde(default)]
  pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCard {
  pub path: String,
  /// GitHub's `owner/name`, or the folder name.
  pub label: String,
  pub home_ref: Option<String>,
  pub missing: bool,
  /// Branches, in stack order, with their depth.
  pub workspaces: Vec<(String, u32)>,
}

/// One uncommitted file in a repository's home checkout.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeChange {
  pub status: String,
  pub path: String,
  pub lines: usize,
}

/// One commit of a repository's home history.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeCommit {
  pub short_id: String,
  pub description: String,
  pub timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeTab {
  #[default]
  Code,
  Changes,
  Commits,
}

/// The press that opens a card's repository.
pub fn open_press(path: &str) -> ScreenAction {
  ScreenAction::Press {
    id: format!("projectHome.open:{path}"),
  }
}

/// The press that opens a repository on one of its tabs.
pub fn open_tab_press(tab: &str, path: &str) -> ScreenAction {
  ScreenAction::Press {
    id: format!("projectHome.{tab}:{path}"),
  }
}

/// A time band: its sort key (the band's first day) and its label.
pub fn band_at(timestamp: &str, today: NaiveDate) -> Option<(NaiveDate, String)> {
  let date = time::day(timestamp)?;
  let age = (today - date).num_days();
  if (0..7).contains(&age) {
    return Some((date, time::day_label_at(timestamp, today)));
  }
  let monday = date - Duration::days(date.weekday().num_days_from_monday() as i64);
  Some((
    monday,
    format!("Week of {} {}", monday.format("%b"), monday.day()),
  ))
}

/// Shared bands across columns, newest first: each band's label and, per
/// column, the indices of that column's commits in it.
pub fn bands(columns: &[Vec<String>], today: NaiveDate) -> Vec<(String, Vec<Vec<usize>>)> {
  let mut keyed: Vec<(NaiveDate, String)> = Vec::new();
  let mut cells: HashMap<NaiveDate, Vec<Vec<usize>>> = HashMap::new();
  for (column, stamps) in columns.iter().enumerate() {
    for (i, stamp) in stamps.iter().enumerate() {
      let Some((key, label)) = band_at(stamp, today) else {
        continue;
      };
      if !keyed.iter().any(|(k, _)| *k == key) {
        keyed.push((key, label));
      }
      cells
        .entry(key)
        .or_insert_with(|| vec![Vec::new(); columns.len()])[column]
        .push(i);
    }
  }
  keyed.sort_by(|a, b| b.0.cmp(&a.0));
  keyed
    .into_iter()
    .map(|(key, label)| (label, cells.remove(&key).unwrap_or_default()))
    .collect()
}

/// What the tabs read, per repository path.
#[derive(Default)]
struct Loaded {
  revision: Option<u64>,
  changes: HashMap<String, Vec<HomeChange>>,
  commits: HashMap<String, Vec<HomeCommit>>,
  loading: bool,
}

pub struct ProjectHomeView {
  route: Route,
  host: Arc<dyn WorkspaceDialogHost>,
  sink: ActionSink,
  dark: bool,
  pub(super) tab: HomeTab,
  loaded: Loaded,
}

const COMMITS_PER_REPO: usize = 30;

impl ProjectHomeView {
  pub fn new(
    route: Route,
    host: Arc<dyn WorkspaceDialogHost>,
    sink: ActionSink,
    dark: bool,
    _cx: &mut Context<Self>,
  ) -> Self {
    Self {
      route,
      host,
      sink,
      dark,
      tab: HomeTab::Code,
      loaded: Loaded::default(),
    }
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.route != route || self.dark != dark {
      self.route = route;
      self.dark = dark;
      cx.notify();
    }
  }

  fn set_tab(&mut self, tab: HomeTab, cx: &mut Context<Self>) {
    self.tab = tab;
    cx.notify();
  }

  /// Reads every repository's changes and history once per revision.
  fn load(&mut self, cx: &mut Context<Self>) {
    if self.tab == HomeTab::Code
      || self.loaded.loading
      || self.loaded.revision == Some(self.route.revision)
    {
      return;
    }
    self.loaded.loading = true;
    let host = self.host.clone();
    let repos: Vec<String> = self
      .route
      .repos
      .iter()
      .filter(|r| !r.missing)
      .map(|r| r.path.clone())
      .collect();
    let revision = self.route.revision;
    cx.spawn(async move |this, cx| {
      let read = cx
        .background_spawn(async move {
          repos
            .into_iter()
            .map(|path| {
              let changes = host.home_changes(&path);
              let commits = host.home_commits(&path, COMMITS_PER_REPO);
              (path, changes, commits)
            })
            .collect::<Vec<_>>()
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.loaded.loading = false;
          view.loaded.revision = Some(revision);
          view.loaded.changes.clear();
          view.loaded.commits.clear();
          for (path, changes, commits) in read {
            view.loaded.changes.insert(path.clone(), changes);
            view.loaded.commits.insert(path, commits);
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn send(&self, action: ScreenAction) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) {
    let sink = self.sink.clone();
    move |_, _, cx| sink(action.clone(), cx)
  }

  fn card(&self, repo: &RepoCard, i: usize, p: &theme::Palette) -> AnyElement {
    let count = repo.workspaces.len();
    div()
      .id(("project-home-card", i))
      .debug_selector({
        let id = format!("project-home-card-{i}");
        move || id
      })
      .role(gpui::Role::Button)
      .aria_label(SharedString::from(format!("Open {}", repo.label)))
      .test_support()
      .flex()
      .flex_col()
      .gap_2()
      .p_4()
      .min_w_0()
      .rounded_lg()
      .border_1()
      .border_color(p.border)
      .bg(p.card)
      .cursor_pointer()
      .hover(|s| s.border_color(p.primary))
      .when(repo.missing, |this| this.opacity(0.5))
      .child(repo_title(repo, p))
      .child(
        ui::text(div(), TextSize::Xs)
          .text_color(p.muted_foreground)
          .truncate()
          .child(if repo.missing {
            format!("{} (not on this machine)", repo.path)
          } else {
            repo.path.clone()
          }),
      )
      .child(
        div()
          .flex()
          .flex_col()
          .gap_1()
          .children(repo.workspaces.iter().take(6).map(|(branch, depth)| {
            div()
              .flex()
              .items_center()
              .gap_1p5()
              .pl(zpx(*depth as f32 * 12.))
              .child(ui::icon(theme::ICON_PR, 12., p.muted_foreground))
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_family(theme::MONO_FONT)
                  .truncate()
                  .child(branch.clone()),
              )
          }))
          .when(count > 6, |this| {
            this.child(
              ui::text(div(), TextSize::Xs)
                .text_color(p.muted_foreground)
                .child(format!("+{} more", count - 6)),
            )
          })
          .when(count == 0 && !repo.missing, |this| {
            this.child(
              ui::text(div(), TextSize::Sm)
                .text_color(p.muted_foreground)
                .child("No workspaces"),
            )
          }),
      )
      .on_click(self.send(open_press(&repo.path)))
      .into_any_element()
  }

  fn code_tab(&self, p: &theme::Palette) -> AnyElement {
    let mut rows = Vec::new();
    let mut it = self
      .route
      .repos
      .iter()
      .enumerate()
      .map(|(i, r)| self.card(r, i, p));
    while let Some(first) = it.next() {
      let second = it.next();
      rows.push(
        div()
          .flex()
          .gap_3()
          .child(div().flex_1().min_w_0().child(first))
          .child(div().flex_1().min_w_0().children(second)),
      );
    }
    div()
      .flex()
      .flex_col()
      .gap_3()
      .children(rows)
      .into_any_element()
  }

  fn changes_tab(&self, p: &theme::Palette) -> AnyElement {
    let mut clean = Vec::new();
    let mut groups = Vec::new();
    for (i, repo) in self.route.repos.iter().enumerate() {
      let files = self
        .loaded
        .changes
        .get(&repo.path)
        .cloned()
        .unwrap_or_default();
      if files.is_empty() {
        clean.push(repo.label.clone());
        continue;
      }
      groups.push(
        div()
          .id(("project-home-changes", i))
          .debug_selector({
            let id = format!("project-home-changes-{i}");
            move || id
          })
          .flex()
          .flex_col()
          .rounded_lg()
          .border_1()
          .border_color(p.border)
          .bg(p.card)
          .overflow_hidden()
          .child(
            div()
              .id(("project-home-changes-open", i))
              .role(gpui::Role::Button)
              .aria_label(SharedString::from(format!(
                "Open changes of {}",
                repo.label
              )))
              .test_support()
              .flex()
              .items_center()
              .gap_2()
              .px_3()
              .py_2()
              .bg(p.muted.opacity(0.5))
              .cursor_pointer()
              .hover(|s| s.bg(p.hover()))
              .child(repo_title(repo, p))
              .child(
                ui::text(div(), TextSize::Xs)
                  .ml_auto()
                  .text_color(p.muted_foreground)
                  .child(format!("{} files", files.len())),
              )
              .on_click(self.send(open_tab_press("changes", &repo.path))),
          )
          .children(files.iter().map(|file| {
            div()
              .flex()
              .items_center()
              .gap_2()
              .px_3()
              .py_1()
              .child(
                ui::text(div(), TextSize::Xs)
                  .w(zpx(14.))
                  .font_weight(FontWeight::SEMIBOLD)
                  .text_color(match file.status.as_str() {
                    "A" => p.success,
                    "D" => p.destructive,
                    _ => p.warning,
                  })
                  .child(file.status.clone()),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_family(theme::MONO_FONT)
                  .min_w_0()
                  .truncate()
                  .child(file.path.clone()),
              )
              .child(
                ui::text(div(), TextSize::Xs)
                  .ml_auto()
                  .text_color(p.muted_foreground)
                  .child(format!("{} lines", file.lines)),
              )
          }))
          .into_any_element(),
      );
    }
    let empty = (!clean.is_empty()).then(|| {
      ui::text(div(), TextSize::Sm)
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_dashed()
        .border_color(p.border)
        .text_color(p.muted_foreground)
        .child(format!("No changes: {}", clean.join(", ")))
    });
    div()
      .flex()
      .flex_col()
      .gap_3()
      .children(groups)
      .children(empty)
      .into_any_element()
  }

  fn commits_tab(&self, p: &theme::Palette) -> AnyElement {
    let repos: Vec<&RepoCard> = self.route.repos.iter().filter(|r| !r.missing).collect();
    let columns: Vec<Vec<HomeCommit>> = repos
      .iter()
      .map(|r| {
        self
          .loaded
          .commits
          .get(&r.path)
          .cloned()
          .unwrap_or_default()
      })
      .collect();
    let stamps: Vec<Vec<String>> = columns
      .iter()
      .map(|c| c.iter().map(|commit| commit.timestamp.clone()).collect())
      .collect();
    let bands = bands(&stamps, Local::now().date_naive());
    let label_w = zpx(110.);
    let header = div()
      .flex()
      .border_b_1()
      .border_color(p.border)
      .child(div().w(label_w).flex_none())
      .children(repos.iter().enumerate().map(|(i, repo)| {
        div()
          .id(("project-home-column", i))
          .role(gpui::Role::Button)
          .aria_label(SharedString::from(format!(
            "Open commits of {}",
            repo.label
          )))
          .test_support()
          .flex_1()
          .min_w(zpx(200.))
          .px_3()
          .py_2()
          .border_l_1()
          .border_color(p.border)
          .cursor_pointer()
          .hover(|s| s.bg(p.hover()))
          .child(repo_title(repo, p))
          .on_click(self.send(open_tab_press("commits", &repo.path)))
      }));
    let rows = bands.into_iter().map(|(label, cells)| {
      div()
        .flex()
        .border_b_1()
        .border_color(p.border)
        .child(
          ui::text(div(), TextSize::Xs)
            .w(label_w)
            .flex_none()
            .px_3()
            .py_2()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.muted_foreground)
            .child(label),
        )
        .children(cells.into_iter().enumerate().map(|(col, indices)| {
          let empty = indices.is_empty();
          div()
            .flex_1()
            .min_w(zpx(200.))
            .flex()
            .flex_col()
            .gap_1()
            .p_1p5()
            .border_l_1()
            .border_color(p.border)
            .when(empty, |this| this.bg(p.muted.opacity(0.3)))
            .children(
              indices
                .into_iter()
                .filter_map(|i| columns[col].get(i))
                .map(|c| {
                  div()
                    .flex()
                    .flex_col()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.card)
                    .child(
                      ui::text(div(), TextSize::Sm)
                        .min_w_0()
                        .truncate()
                        .child(c.description.lines().next().unwrap_or("").to_string()),
                    )
                    .child(
                      ui::text(div(), TextSize::Xs)
                        .text_color(p.muted_foreground)
                        .font_family(theme::MONO_FONT)
                        .child(format!("{} · {}", c.short_id, time::relative(&c.timestamp))),
                    )
                }),
            )
        }))
    });
    div()
      .id("project-home-commits")
      .debug_selector(|| "project-home-commits".into())
      .rounded_lg()
      .border_1()
      .border_color(p.border)
      .overflow_x_scroll()
      .child(
        div()
          .min_w_full()
          .flex()
          .flex_col()
          .child(header)
          .children(rows),
      )
      .into_any_element()
  }
}

/// A repository's name over its checked-out branch.
fn repo_title(repo: &RepoCard, p: &theme::Palette) -> impl IntoElement {
  div()
    .flex()
    .flex_col()
    .min_w_0()
    .child(
      ui::text(div(), TextSize::Lg)
        .font_weight(FontWeight::SEMIBOLD)
        .truncate()
        .child(repo.label.clone()),
    )
    .children(repo.home_ref.clone().map(|branch| {
      ui::text(div(), TextSize::Xs)
        .font_family(theme::MONO_FONT)
        .text_color(p.muted_foreground)
        .child(branch)
    }))
}

impl Render for ProjectHomeView {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.load(cx);
    let p = theme::palette(self.dark);
    let title = self
      .route
      .name
      .clone()
      .unwrap_or_else(|| "Unsaved project".into());
    let workspaces: usize = self.route.repos.iter().map(|r| r.workspaces.len()).sum();
    let tabs = [HomeTab::Code, HomeTab::Changes, HomeTab::Commits];
    let tab_bar = ui::Tabs::segmented("project-home-tabs", &p)
      .tabs(vec![
        ui::Tab::new("project-home-tab-code", "Code").icon(theme::ICON_CODE),
        ui::Tab::new("project-home-tab-changes", "Changes").icon(theme::ICON_FILE_DIFF),
        ui::Tab::new("project-home-tab-commits", "Commits").icon(theme::ICON_COMMIT),
      ])
      .selected(tabs.iter().position(|t| *t == self.tab).unwrap_or(0))
      .on_select({
        let this = cx.entity().downgrade();
        move |ix, _, cx| {
          this
            .update(cx, |view, cx| view.set_tab(tabs[ix], cx))
            .ignore_closed();
        }
      });
    let body = match self.tab {
      HomeTab::Code => self.code_tab(&p),
      HomeTab::Changes => self.changes_tab(&p),
      HomeTab::Commits => self.commits_tab(&p),
    };
    div()
      .id("project-home")
      .debug_selector(|| "project-home".into())
      .size_full()
      .overflow_y_scroll()
      .bg(p.background)
      .text_color(p.foreground)
      .flex()
      .flex_col()
      .gap_4()
      .p_6()
      .child(
        div()
          .flex()
          .items_baseline()
          .gap_3()
          .child(
            ui::text(div().id("project-home-heading"), TextSize::Xl)
              .role(gpui::Role::Heading)
              .aria_label(SharedString::from(title.clone()))
              .test_support()
              .font_weight(FontWeight::SEMIBOLD)
              .child(title),
          )
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child(format!(
                "{} repositories · {} workspaces",
                self.route.repos.len(),
                workspaces
              )),
          ),
      )
      .child(div().child(tab_bar))
      .child(body)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
  }

  /// Noon local time on the day, so the local date is that day anywhere.
  fn stamp(date: NaiveDate) -> String {
    date
      .and_hms_opt(12, 0, 0)
      .unwrap()
      .and_local_timezone(Local)
      .unwrap()
      .to_rfc3339()
  }

  #[test]
  fn this_week_bands_by_day_older_by_week() {
    let today = day(2026, 11, 4); // a Wednesday
    assert_eq!(band_at(&stamp(today), today).unwrap().1, "Today");
    assert_eq!(
      band_at(&stamp(day(2026, 11, 3)), today).unwrap().1,
      "Yesterday"
    );
    let old = band_at(&stamp(day(2026, 10, 22)), today).unwrap();
    assert_eq!(old.0, day(2026, 10, 19), "the Monday of that week");
    assert_eq!(old.1, "Week of Oct 19");
  }

  #[test]
  fn bands_line_up_columns_newest_first() {
    let today = day(2026, 11, 4);
    let columns = vec![
      vec![stamp(today), stamp(today), stamp(day(2026, 10, 20))],
      vec![stamp(day(2026, 11, 3))],
    ];
    let bands = bands(&columns, today);
    let labels: Vec<&str> = bands.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels, ["Today", "Yesterday", "Week of Oct 19"]);
    assert_eq!(bands[0].1, vec![vec![0, 1], vec![]]);
    assert_eq!(bands[1].1, vec![vec![], vec![0]]);
    assert_eq!(bands[2].1, vec![vec![2], vec![]]);
  }

  #[test]
  fn bands_skip_unreadable_timestamps() {
    let today = day(2026, 11, 4);
    assert!(bands(&[vec!["not a date".into()]], today).is_empty());
  }
}
