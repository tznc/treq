//! A repository's GitHub pull requests: a state filter, a search, and
//! pages of results. A tap opens one in [`super::pr_detail`].

use crate::mobile::ui::widgets::TouchButton as _;
use crate::{
  core::remote::{GithubQuery, TreqCommandRequest},
  github::{GhListPage, GhPullRequest},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState},
    IconName, Sizable,
  },
  div, px, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
  Render, ScrollHandle, StatefulInteractiveElement, Styled, Window,
};
use treq_ui::{Badge, Tone};

use super::{on_epoch, pr_detail};
use crate::github_filters::{filter_options, filter_pr_indices, PrFilters};
use crate::mobile::ui::sheet;
use crate::mobile::{
  errors, remote,
  ui::{load, widgets, Load},
};
use std::rc::Rc;

/// How many results a search asks for.
pub const SEARCH_LIMIT: u32 = 50;

/// The list's state filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
  #[default]
  Open,
  Draft,
  Closed,
  Merged,
  All,
}

impl Filter {
  pub const ALL: [Filter; 5] = [
    Filter::Open,
    Filter::Draft,
    Filter::Closed,
    Filter::Merged,
    Filter::All,
  ];

  /// The `state` `ListPrs` takes.
  pub fn value(self) -> &'static str {
    match self {
      Filter::Open => "open",
      Filter::Draft => "draft",
      Filter::Closed => "closed",
      Filter::Merged => "merged",
      Filter::All => "all",
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      Filter::Open => "Open",
      Filter::Draft => "Draft",
      Filter::Closed => "Closed",
      Filter::Merged => "Merged",
      Filter::All => "All",
    }
  }
}

/// The Filters button's count: the set filters, with "Only mine".
pub fn filter_count(filters: &PrFilters, mine: bool) -> usize {
  filters.active_count() + usize::from(mine)
}

pub fn list_request(repo: &str, filter: Filter, page: u32) -> TreqCommandRequest {
  TreqCommandRequest::GithubRead {
    repo: repo.to_string(),
    query: GithubQuery::ListPrs {
      state: filter.value().to_string(),
      page,
    },
  }
}

pub fn search_request(repo: &str, mine: bool, query: &str) -> TreqCommandRequest {
  TreqCommandRequest::GithubRead {
    repo: repo.to_string(),
    query: GithubQuery::SearchOpenPrs {
      mine,
      query: query.trim().to_string(),
      limit: SEARCH_LIMIT,
    },
  }
}

/// Adds a further page to `list`, skipping pull requests already there
/// (one that moved up a page between reads).
pub fn append_page(list: &mut Vec<GhPullRequest>, page: Vec<GhPullRequest>) {
  for pr in page {
    if !list.iter().any(|p| p.number == pr.number) {
      list.push(pr);
    }
  }
}

/// "Open", "Draft", "Merged" or "Closed".
pub fn state_label(pr: &GhPullRequest) -> &'static str {
  match pr.state.as_str() {
    "MERGED" => "Merged",
    "CLOSED" => "Closed",
    _ if pr.is_draft => "Draft",
    _ => "Open",
  }
}

/// A tag for `pr`'s state.
pub fn state_tag(pr: &GhPullRequest, cx: &App) -> Badge {
  let tone = match state_label(pr) {
    "Merged" => Tone::GitMerged,
    "Closed" => Tone::GitClosed,
    "Draft" => Tone::Neutral,
    _ => Tone::GitOpen,
  };
  Badge::new(state_label(pr), &treq_ui::theme::current(cx)).tone(tone)
}

/// Where `pr`'s review stands, when it stands anywhere.
pub fn review_tag(pr: &GhPullRequest, cx: &App) -> Option<Badge> {
  use crate::github::ReviewState;
  let state = crate::core::pr_view::review_state(pr)?;
  let tone = match state {
    ReviewState::Approved => Tone::Success,
    ReviewState::ChangesRequested => Tone::Danger,
    ReviewState::ReviewRequired => Tone::Warning,
  };
  Some(Badge::new(state.label(), &treq_ui::theme::current(cx)).tone(tone))
}

/// The row's second line: number, author and branches.
pub fn detail_line(pr: &GhPullRequest) -> String {
  format!(
    "#{} · {} · {} → {}",
    pr.number, pr.author.login, pr.head_ref_name, pr.base_ref_name
  )
}

pub struct PullRequestsScreen {
  repo: String,
  filter: Filter,
  query: Entity<InputState>,
  /// The search the results are for; empty lists by `filter`.
  searched: String,
  mine: bool,
  /// The Filters sheet's selects, over the loaded pull requests.
  filters: PrFilters,
  list: Load<Vec<GhPullRequest>>,
  page: u32,
  has_more: bool,
  loading_more: bool,
  more_error: Option<String>,
  /// Bumped per fetch, so a slow older read never replaces a newer one.
  generation: u64,
  scroll: ScrollHandle,
}

impl PullRequestsScreen {
  pub(super) fn new(repo: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search open pull requests"));
    cx.subscribe(&query, |this, _, event: &InputEvent, cx| match event {
      InputEvent::PressEnter { .. } => this.search(cx),
      InputEvent::Change => {
        // Clearing the box goes back to the filtered list.
        if this.query.read(cx).value().trim().is_empty() && !this.searched.is_empty() {
          this.search(cx);
        }
        cx.notify();
      }
      _ => {}
    })
    .detach();
    let filter = Filter::default();
    let list = remote::last::<GhListPage<GhPullRequest>>(&list_request(&repo, filter, 1))
      .map_or(Load::Idle, |page| Load::Ready(page.items));
    let mut this = Self {
      repo,
      filter,
      query,
      searched: String::new(),
      mine: false,
      filters: PrFilters::default(),
      list,
      page: 1,
      has_more: false,
      loading_more: false,
      more_error: None,
      generation: 0,
      scroll: ScrollHandle::new(),
    };
    this.fetch(true, cx);
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  fn searching(&self) -> bool {
    !self.searched.is_empty() || self.mine
  }

  fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
    if self.filter == filter && !self.searching() {
      return;
    }
    self.filter = filter;
    self.mine = false;
    self.searched.clear();
    self.list = Load::Loading;
    self.fetch(false, cx);
  }

  fn search(&mut self, cx: &mut Context<Self>) {
    self.searched = self.query.read(cx).value().trim().to_string();
    self.list = Load::Loading;
    self.fetch(false, cx);
  }

  fn toggle_mine(&mut self, cx: &mut Context<Self>) {
    self.mine = !self.mine;
    self.search(cx);
  }

  /// `join` as in [`remote::read_cached`]. A search (or "Mine") reads open
  /// pull requests matching it; otherwise the first page of `filter`.
  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.list.ready().is_none() {
      self.list = Load::Loading;
    }
    self.generation += 1;
    let generation = self.generation;
    self.page = 1;
    self.more_error = None;
    self.loading_more = false;
    if self.searching() {
      let request = search_request(&self.repo, self.mine, &self.searched);
      load::run(
        cx,
        remote::read_cached::<Vec<GhPullRequest>>(request, join),
        move |this, result, _| {
          if this.generation == generation {
            this.has_more = false;
            this.list = Load::from_result(result);
          }
        },
      );
    } else {
      let request = list_request(&self.repo, self.filter, 1);
      load::run(
        cx,
        remote::read_cached::<GhListPage<GhPullRequest>>(request, join),
        move |this, result, _| {
          if this.generation == generation {
            this.has_more = result.as_ref().is_ok_and(|p| p.has_more);
            this.list = Load::from_result(result.map(|p| p.items));
          }
        },
      );
    }
    cx.notify();
  }

  fn load_more(&mut self, cx: &mut Context<Self>) {
    if self.loading_more || !self.has_more {
      return;
    }
    self.loading_more = true;
    self.more_error = None;
    let generation = self.generation;
    let page = self.page + 1;
    let request = list_request(&self.repo, self.filter, page);
    load::run(
      cx,
      remote::read::<GhListPage<GhPullRequest>>(request),
      move |this, result, _| {
        if this.generation != generation {
          return;
        }
        this.loading_more = false;
        match result {
          Ok(next) => {
            this.page = page;
            this.has_more = next.has_more;
            if let Load::Ready(list) = &mut this.list {
              append_page(list, next.items);
            }
          }
          Err(error) => this.more_error = Some(errors::describe(&error).message),
        }
      },
    );
    cx.notify();
  }

  fn state_menu(&mut self, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    let current = (!self.mine).then_some(self.filter);
    let entries = Filter::ALL
      .iter()
      .map(|&filter| {
        let this = this.clone();
        sheet::MenuEntry::new(
          format!("state-{}", filter.value()),
          filter.label(),
          move |_, cx| {
            this
              .update(cx, |screen, cx| screen.set_filter(filter, cx))
              .ok();
          },
        )
        .selected(current == Some(filter))
      })
      .collect();
    sheet::menu("State", entries, cx);
  }

  /// The advanced filters, as desktop's Filters popover offers them.
  fn filters_sheet(&mut self, cx: &mut Context<Self>) {
    let options = filter_options(self.list.ready().map(Vec::as_slice).unwrap_or_default());
    let this = cx.weak_entity();
    let set = |apply: fn(&mut PrFilters, Option<String>)| -> widgets::OnPick {
      let this = this.clone();
      Rc::new(move |value, cx| {
        this
          .update(cx, |screen, cx| {
            apply(&mut screen.filters, value);
            cx.notify();
          })
          .ok();
      })
    };
    let f = &self.filters;
    let mine = this.clone();
    let mut entries = vec![
      sheet::MenuEntry::new(
        "filter-mine",
        if self.mine {
          "Only mine: On"
        } else {
          "Only mine: Off"
        },
        move |_, cx| {
          mine.update(cx, |screen, cx| screen.toggle_mine(cx)).ok();
        },
      )
      .selected(self.mine),
      widgets::filter_field(
        "filter-author",
        "Author",
        f.author.clone(),
        options.authors,
        set(|f, v| f.author = v),
      ),
      widgets::filter_field(
        "filter-label",
        "Label",
        f.label.clone(),
        options.labels,
        set(|f, v| f.label = v),
      ),
      widgets::filter_field(
        "filter-base",
        "Base",
        f.base.clone(),
        options.bases,
        set(|f, v| f.base = v),
      ),
      widgets::filter_field(
        "filter-conflict",
        "Conflicts",
        f.conflict.clone(),
        vec!["conflicting".into(), "clean".into()],
        set(|f, v| f.conflict = v),
      ),
    ];
    if filter_count(&self.filters, self.mine) > 0 {
      entries.push(sheet::MenuEntry::new(
        "filter-clear",
        "Clear filters",
        move |_, cx| {
          this
            .update(cx, |screen, cx| {
              screen.filters = screen.filters.cleared();
              if screen.mine {
                screen.toggle_mine(cx);
              }
              cx.notify();
            })
            .ok();
        },
      ));
    }
    sheet::menu("Filters", entries, cx);
  }
}

impl Render for PullRequestsScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let searching = self.searching();
    let body = match &self.list {
      Load::Idle | Load::Loading => {
        widgets::card(cx).child(widgets::skeleton("pull-requests", 6, cx))
      }
      Load::Failed(error) => widgets::card(cx).child(widgets::failed(
        "pull-requests-failed",
        error,
        cx.listener(|this, _, _, cx| this.reload(cx)),
        cx,
      )),
      Load::Ready(list) if list.is_empty() => widgets::card(cx).child(widgets::empty(
        "pull-requests-empty",
        if searching {
          "No open pull requests match."
        } else {
          "No pull requests."
        },
        cx,
      )),
      Load::Ready(list)
        if filter_pr_indices(list, &self.filters, chrono::Utc::now()).is_empty() =>
      {
        widgets::card(cx).child(widgets::empty(
          "pull-requests-no-match",
          "No loaded pull requests match. Clear the filters to see them all.",
          cx,
        ))
      }
      Load::Ready(list) => widgets::card(cx).children(
        filter_pr_indices(list, &self.filters, chrono::Utc::now())
          .into_iter()
          .map(|ix| &list[ix])
          .enumerate()
          .map(|(i, pr)| {
            let repo = self.repo.clone();
            let number = pr.number;
            widgets::row_full(
              format!("pr-{i}"),
              None,
              pr.title.clone(),
              Some(detail_line(pr).into()),
              Some(state_tag(pr, cx).into_any_element()),
              cx,
            )
            .on_click(move |_, _, cx| pr_detail::open(repo.clone(), number, cx))
          }),
      ),
    };
    let chips = widgets::filter_row(
      "prs",
      if self.mine {
        "Open"
      } else {
        self.filter.label()
      },
      cx.listener(|this, _, _, cx| this.state_menu(cx)),
      filter_count(&self.filters, self.mine),
      cx.listener(|this, _, _, cx| this.filters_sheet(cx)),
    );
    div()
      .id("pull-requests")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .gap_3()
          .child(chips)
          .child(
            h_flex()
              .gap_2()
              .child(
                div()
                  .flex_1()
                  .min_w_0()
                  .child(Input::new(&self.query).large().cleanable(true)),
              )
              .child(
                Button::new("search")
                  .h(px(44.))
                  .w(px(44.))
                  .outline()
                  .icon(IconName::Search)
                  .accessibility_label("Search")
                  .on_click(cx.listener(|this, _, _, cx| this.search(cx)))
                  .touch(),
              ),
          )
          .when(searching, |page| {
            page.child(widgets::muted("Showing open pull requests that match.", cx))
          })
          .child(body)
          .when_some(self.more_error.clone(), |page, error| {
            page.child(widgets::error_text(error, cx))
          })
          .when(self.has_more && !searching, |page| {
            page.child(
              Button::new("load-more")
                .outline()
                .large()
                .w_full()
                .h(px(widgets::CTA_H - 8.))
                .label("Load more")
                .loading(self.loading_more)
                .on_click(cx.listener(|this, _, _, cx| this.load_more(cx)))
                .touch(),
            )
          }),
      )
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::github::GhAuthor;

  pub fn pr(number: u64, state: &str, draft: bool) -> GhPullRequest {
    GhPullRequest {
      number,
      title: format!("PR {number}"),
      state: state.into(),
      url: format!("https://github.com/o/r/pull/{number}"),
      body: None,
      author: GhAuthor {
        login: "ann".into(),
        avatar_url: None,
      },
      labels: Vec::new(),
      head_ref_name: "feature".into(),
      base_ref_name: "main".into(),
      merge_state_status: None,
      created_at: String::new(),
      updated_at: String::new(),
      comments: None,
      is_draft: draft,
      review_decision: None,
      latest_reviews: vec![],
      reviews: vec![],
    }
  }

  #[core::prelude::v1::test]
  fn only_mine_counts_as_a_filter() {
    let filters = crate::github_filters::PrFilters {
      label: Some("bug".into()),
      ..Default::default()
    };
    assert_eq!(filter_count(&filters, false), 1);
    assert_eq!(filter_count(&filters, true), 2);
  }

  #[core::prelude::v1::test]
  fn filters_name_gh_states() {
    let values: Vec<_> = Filter::ALL.iter().map(|f| f.value()).collect();
    assert_eq!(values, ["open", "draft", "closed", "merged", "all"]);
    assert_eq!(Filter::Merged.label(), "Merged");
    assert_eq!(Filter::default(), Filter::Open);
  }

  #[core::prelude::v1::test]
  fn requests_carry_filter_page_and_search() {
    assert_eq!(
      list_request("/r", Filter::Closed, 2),
      TreqCommandRequest::GithubRead {
        repo: "/r".into(),
        query: GithubQuery::ListPrs {
          state: "closed".into(),
          page: 2
        },
      }
    );
    assert_eq!(
      search_request("/r", true, "  fix  "),
      TreqCommandRequest::GithubRead {
        repo: "/r".into(),
        query: GithubQuery::SearchOpenPrs {
          mine: true,
          query: "fix".into(),
          limit: SEARCH_LIMIT
        },
      }
    );
  }

  #[core::prelude::v1::test]
  fn further_pages_skip_repeats() {
    let mut list = vec![pr(3, "OPEN", false), pr(2, "OPEN", false)];
    append_page(&mut list, vec![pr(2, "OPEN", false), pr(1, "OPEN", false)]);
    let numbers: Vec<_> = list.iter().map(|p| p.number).collect();
    assert_eq!(numbers, [3, 2, 1]);
  }

  #[core::prelude::v1::test]
  fn states_and_rows_read_as_words() {
    assert_eq!(state_label(&pr(1, "OPEN", false)), "Open");
    assert_eq!(state_label(&pr(1, "OPEN", true)), "Draft");
    assert_eq!(state_label(&pr(1, "MERGED", false)), "Merged");
    assert_eq!(state_label(&pr(1, "CLOSED", false)), "Closed");
    assert_eq!(
      detail_line(&pr(4, "OPEN", false)),
      "#4 · ann · feature → main"
    );
  }
}
