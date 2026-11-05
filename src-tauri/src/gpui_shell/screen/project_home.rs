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
  /// GitHub's `owner/name` when the origin is on GitHub.
  #[serde(default)]
  pub github: Option<String>,
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
  pub insertions: u32,
  pub deletions: u32,
}

pub use crate::core::projects::RepoStats;

/// Days the activity chart covers.
pub const ACTIVITY_DAYS: usize = 30;

/// Commits per day for the `days` days ending `today`, oldest first.
pub fn daily_activity(stamps: &[String], today: NaiveDate, days: usize) -> Vec<u32> {
  let mut counts = vec![0; days];
  for stamp in stamps {
    let Some(date) = time::day(stamp) else {
      continue;
    };
    let age = (today - date).num_days();
    if age >= 0 && (age as usize) < days {
      counts[days - 1 - age as usize] += 1;
    }
  }
  counts
}

/// Bytes as gigabytes, to two places for small sizes.
pub fn format_gb(bytes: u64) -> String {
  if bytes == 0 {
    return "0 GB".into();
  }
  let gb = bytes as f64 / 1_073_741_824.;
  if gb < 0.01 {
    return "<0.01 GB".into();
  }
  let text = if gb >= 1. {
    format!("{gb:.1}")
  } else {
    format!("{gb:.2}")
  };
  format!("{} GB", text.trim_end_matches('0').trim_end_matches('.'))
}

/// A GitHub count as read, "30+" when the first page was full.
pub fn count_label(count: Option<(usize, bool)>) -> String {
  match count {
    Some((n, true)) => format!("{n}+"),
    Some((n, false)) => n.to_string(),
    None => "–".into(),
  }
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
  keyed.sort_by_key(|k| std::cmp::Reverse(k.0));
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
  stats: HashMap<String, RepoStats>,
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
    if self.loaded.loading || self.loaded.revision == Some(self.route.revision) {
      return;
    }
    self.loaded.loading = true;
    let host = self.host.clone();
    let repos: Vec<(String, Option<String>)> = self
      .route
      .repos
      .iter()
      .filter(|r| !r.missing)
      .map(|r| (r.path.clone(), r.github.clone()))
      .collect();
    let revision = self.route.revision;
    cx.spawn(async move |this, cx| {
      let read = cx
        .background_spawn(async move {
          repos
            .into_iter()
            .map(|(path, github)| {
              let changes = host.home_changes(&path);
              let commits = host.home_commits(&path, COMMITS_PER_REPO);
              let stats = host.repo_stats(&path, github.as_deref());
              (path, changes, commits, stats)
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
          view.loaded.stats.clear();
          for (path, changes, commits, stats) in read {
            view.loaded.changes.insert(path.clone(), changes);
            view.loaded.commits.insert(path.clone(), commits);
            view.loaded.stats.insert(path, stats);
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
      .children(
        self
          .loaded
          .stats
          .get(&repo.path)
          .map(|stats| stats_block(repo, stats, p)),
      )
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
            this.child(ui::EmptyState::new(
              format!("project-card-empty-{i}"),
              "No workspaces",
              p,
            ))
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
                  .child(format!(
                    "{} {}",
                    files.len(),
                    if files.len() == 1 { "file" } else { "files" }
                  )),
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
                  .text_color(file_status_color(&file.status, p))
                  .child(file.status.clone()),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_family(theme::MONO_FONT)
                  .min_w_0()
                  .truncate()
                  .child(file.path.clone()),
              )
              // jj leaves the count at 0 when it defers the diff: say nothing.
              .when(file.lines > 0, |this| {
                this.child(
                  ui::text(div(), TextSize::Xs)
                    .ml_auto()
                    .text_color(p.muted_foreground)
                    .child(format!("{} lines", file.lines)),
                )
              })
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

  /// One repository's commits in one band, drawn as the Code tab's
  /// history: a track line, a dot per commit, then its title, time and
  /// line counts.
  fn history_cell(
    &self,
    commits: Vec<&HomeCommit>,
    newest: bool,
    p: &theme::Palette,
  ) -> AnyElement {
    if commits.is_empty() {
      return div().into_any_element();
    }
    let (green, red) = (ui::Tone::GitOpen.text(p), ui::Tone::GitClosed.text(p));
    div()
      .relative()
      .flex()
      .flex_col()
      .child(
        div()
          .absolute()
          .left(zpx(7.))
          .top_2()
          .bottom_2()
          .w(zpx(2.))
          .bg(p.border),
      )
      .children(commits.into_iter().enumerate().map(|(i, c)| {
        let dot = if newest && i == 0 {
          p.primary
        } else {
          p.muted_foreground
        };
        div()
          .flex()
          .items_start()
          .gap_3()
          .py_2()
          .child(super::code::history_dot(dot, p))
          .child(
            div()
              .flex_1()
              .min_w_0()
              .pt_0p5()
              .child(
                ui::text(div(), TextSize::Sm)
                  .truncate()
                  .child(c.description.lines().next().unwrap_or("").to_string()),
              )
              .child(
                div()
                  .flex()
                  .items_center()
                  .gap_2()
                  .mt_0p5()
                  .child(
                    ui::text(div(), TextSize::Xs)
                      .text_color(p.muted_foreground)
                      .child(time::relative(&c.timestamp)),
                  )
                  .when(c.insertions > 0 || c.deletions > 0, |this| {
                    this.child(
                      ui::text(div(), TextSize::Xs)
                        .ml_auto()
                        .flex()
                        .child(div().text_color(green).child(format!("+{}", c.insertions)))
                        .child("\u{a0}")
                        .child(div().text_color(red).child(format!("-{}", c.deletions))),
                    )
                  }),
              ),
          )
      }))
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
      .gap_6()
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
          .min_w(zpx(220.))
          .px_2()
          .py_1()
          .rounded_md()
          .cursor_pointer()
          .hover(|s| s.bg(p.hover()))
          .child(repo_title(repo, p))
          .on_click(self.send(open_tab_press("commits", &repo.path)))
      }));
    let mut seen = vec![false; repos.len()];
    let rows: Vec<AnyElement> = bands
      .into_iter()
      .map(|(label, cells)| {
        div()
          .flex()
          .gap_6()
          .pt_3()
          .child(
            ui::text(div(), TextSize::Xs)
              .w(label_w)
              .flex_none()
              .pt_2()
              .text_color(p.muted_foreground)
              .child(label),
          )
          .children(cells.into_iter().enumerate().map(|(col, indices)| {
            let newest = !seen[col] && !indices.is_empty();
            if !indices.is_empty() {
              seen[col] = true;
            }
            let commits: Vec<&HomeCommit> = indices
              .into_iter()
              .filter_map(|i| columns[col].get(i))
              .collect();
            div()
              .flex_1()
              .min_w(zpx(220.))
              .px_2()
              .child(self.history_cell(commits, newest, p))
          }))
          .into_any_element()
      })
      .collect();
    div()
      .id("project-home-commits")
      .debug_selector(|| "project-home-commits".into())
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

/// A card's numbers: the 30-day activity line, then the counts.
fn stats_block(repo: &RepoCard, stats: &RepoStats, p: &theme::Palette) -> impl IntoElement {
  let activity = daily_activity(
    &stats.commit_times,
    Local::now().date_naive(),
    ACTIVITY_DAYS,
  );
  let total: u32 = activity.iter().sum();
  let stat = |label: &'static str, value: String| {
    div()
      .flex()
      .flex_col()
      .min_w_0()
      .px_2()
      .py_1()
      .rounded_md()
      .bg(p.muted.opacity(0.5))
      .child(
        ui::text(div(), TextSize::Xs)
          .text_color(p.muted_foreground)
          .child(label),
      )
      .child(
        ui::text(div(), TextSize::Sm)
          .font_weight(FontWeight::SEMIBOLD)
          .child(value),
      )
  };
  let mut cells = Vec::new();
  if repo.github.is_some() {
    cells.push(stat("Open issues", count_label(stats.open_issues)));
    cells.push(stat("Open PRs", count_label(stats.open_prs)));
  }
  cells.push(stat(
    "Workspaces",
    format!("{} · {} archived", stats.workspaces, stats.archived),
  ));
  cells.push(stat("Workspace size", format_gb(stats.workspace_bytes)));
  div()
    .flex()
    .flex_col()
    .gap_2()
    .child(
      div()
        .flex()
        .items_end()
        .gap_2()
        .child(activity_chart(activity, p.primary, p.border))
        .child(
          ui::text(div(), TextSize::Xs)
            .flex_none()
            .text_color(p.muted_foreground)
            .child(format!(
              "{total} {} / 30d",
              if total == 1 { "commit" } else { "commits" }
            )),
        ),
    )
    .child(div().flex().flex_wrap().gap_2().children(cells))
}

/// A daily line chart of commit counts, scaled to the busiest day.
fn activity_chart(counts: Vec<u32>, line: gpui::Hsla, base: gpui::Hsla) -> impl IntoElement {
  let max = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
  div()
    .debug_selector(|| "project-home-activity".into())
    .flex_1()
    .h(zpx(36.))
    .child(
      gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
          let n = counts.len().max(2) as f32 - 1.;
          let point = |i: usize, c: u32| {
            gpui::point(
              bounds.origin.x + bounds.size.width * (i as f32 / n),
              bounds.origin.y + bounds.size.height * (1. - c as f32 / max) - gpui::px(1.),
            )
          };
          let mut axis = gpui::PathBuilder::stroke(gpui::px(1.));
          axis.move_to(gpui::point(bounds.origin.x, bounds.bottom() - gpui::px(1.)));
          axis.line_to(gpui::point(bounds.right(), bounds.bottom() - gpui::px(1.)));
          if let Ok(path) = axis.build() {
            window.paint_path(path, base);
          }
          let mut path = gpui::PathBuilder::stroke(gpui::px(1.5));
          for (i, c) in counts.iter().enumerate() {
            if i == 0 {
              path.move_to(point(i, *c));
            } else {
              path.line_to(point(i, *c));
            }
          }
          if let Ok(path) = path.build() {
            window.paint_path(path, line);
          }
        },
      )
      .size_full(),
    )
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

/// A changed file's status letter colour: diff tokens for added and
/// removed, warning for the rest.
fn file_status_color(status: &str, p: &theme::Palette) -> gpui::Hsla {
  match status {
    "A" => p.diff_added(),
    "D" => p.diff_removed(),
    _ => p.warning,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn daily_activity_counts_commits_per_day_oldest_first() {
    let today = day(2026, 11, 4);
    let stamps = vec![
      stamp(today),
      stamp(today),
      stamp(day(2026, 11, 2)),
      stamp(day(2026, 9, 1)),
    ];
    assert_eq!(daily_activity(&stamps, today, 4), vec![0, 1, 0, 2]);
  }

  /// An added file's letter paints with the diff token, not state green.
  #[test]
  fn added_file_letter_uses_the_diff_token() {
    let p = theme::palette(false);
    assert_eq!(file_status_color("A", &p), p.diff_added());
    assert_eq!(file_status_color("D", &p), p.diff_removed());
    assert_eq!(file_status_color("M", &p), p.warning);
  }

  #[test]
  fn formats_sizes_in_gigabytes() {
    assert_eq!(format_gb(0), "0 GB");
    assert_eq!(format_gb(1_610_612_736), "1.5 GB");
    assert_eq!(format_gb(52_428_800), "0.05 GB");
    assert_eq!(format_gb(4_096), "<0.01 GB");
  }

  #[test]
  fn counts_show_a_plus_when_there_are_more() {
    assert_eq!(count_label(Some((30, true))), "30+");
    assert_eq!(count_label(Some((4, false))), "4");
    assert_eq!(count_label(None), "–");
  }

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
