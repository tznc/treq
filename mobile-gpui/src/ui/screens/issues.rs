//! The repository's GitHub issues: open or closed, paged, searchable over
//! what's loaded, and the sheet that writes one. GitHub is reached with the
//! host's `gh`, so the host's sign-in is the one used.

use std::rc::Rc;

use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};
use treq_lib::{
  core::remote::{GithubAction, GithubQuery, TreqCommandRequest},
  github::{GhIssue, GhListPage, GhReactionGroup},
};
use treq_ui::{Dot, Tone};

use super::{issue, mutation_message, on_epoch};
use crate::{
  errors,
  remote::{self, IdempotencyKeys, Mutation},
  ui::{load, sheet, widgets, Load},
};

/// `gh issue close --reason` values with their action labels, as the
/// desktop app offers them.
pub const CLOSE_REASONS: [(&str, &str); 2] = [
  ("completed", "Close as completed"),
  ("not planned", "Close as not planned"),
];

/// GitHub's `ReactionContent`s with their emoji, in GitHub's order.
pub const REACTIONS: [(&str, &str); 8] = [
  ("THUMBS_UP", "👍"),
  ("THUMBS_DOWN", "👎"),
  ("LAUGH", "😄"),
  ("HOORAY", "🎉"),
  ("CONFUSED", "😕"),
  ("HEART", "❤️"),
  ("ROCKET", "🚀"),
  ("EYES", "👀"),
];

/// The emoji for a `ReactionContent`; "?" for one GitHub added later.
pub fn emoji(content: &str) -> &'static str {
  REACTIONS
    .iter()
    .find(|(c, _)| *c == content)
    .map_or("?", |(_, e)| e)
}

/// "Open" or "Closed".
pub fn state_label(state: &str) -> &'static str {
  match state {
    "CLOSED" => "Closed",
    _ => "Open",
  }
}

/// Whether `issue` matches a search: its title contains the words, or the
/// query is its number (with or without `#`).
pub fn matches(issue: &GhIssue, query: &str) -> bool {
  let query = query.trim();
  if query.is_empty() {
    return true;
  }
  if let Ok(number) = query.trim_start_matches('#').parse::<u64>() {
    if number == issue.number {
      return true;
    }
  }
  !query.starts_with('#') && issue.title.to_lowercase().contains(&query.to_lowercase())
}

/// Adds the next page, skipping issues already listed (one can move up a
/// page while paging).
pub fn append_page(items: &mut Vec<GhIssue>, page: Vec<GhIssue>) {
  for issue in page {
    if !items.iter().any(|i| i.number == issue.number) {
      items.push(issue);
    }
  }
}

/// Flips the viewer's `content` reaction in `groups`, as GitHub will once
/// the change lands. Returns whether the viewer now reacts.
pub fn toggle_reaction(groups: &mut Vec<GhReactionGroup>, content: &str) -> bool {
  match groups.iter().position(|g| g.content == content) {
    Some(index) => {
      let group = &mut groups[index];
      let on = !group.viewer_has_reacted;
      group.viewer_has_reacted = on;
      group.count = if on {
        group.count + 1
      } else {
        group.count.saturating_sub(1)
      };
      if group.count == 0 {
        groups.remove(index);
      }
      on
    }
    None => {
      groups.push(GhReactionGroup {
        content: content.into(),
        count: 1,
        viewer_has_reacted: true,
      });
      true
    }
  }
}

pub fn read(repo: &str, query: GithubQuery) -> TreqCommandRequest {
  TreqCommandRequest::GithubRead {
    repo: repo.to_string(),
    query,
  }
}

fn list_request(repo: &str, state: Filter, after: Option<String>) -> TreqCommandRequest {
  read(
    repo,
    GithubQuery::ListIssues {
      state: state.as_str().into(),
      after,
    },
  )
}

/// What an issue page calls after it changed an issue.
pub type OnChanged = Rc<dyn Fn(&mut App)>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
  Open,
  Closed,
}

impl Filter {
  fn as_str(self) -> &'static str {
    match self {
      Self::Open => "open",
      Self::Closed => "closed",
    }
  }
}

struct Listed {
  items: Vec<GhIssue>,
  has_more: bool,
  end_cursor: Option<String>,
}

impl From<GhListPage<GhIssue>> for Listed {
  fn from(page: GhListPage<GhIssue>) -> Self {
    Self {
      items: page.items,
      has_more: page.has_more,
      end_cursor: page.end_cursor,
    }
  }
}

pub struct IssuesScreen {
  repo: String,
  filter: Filter,
  list: Load<Listed>,
  loading_more: bool,
  /// Why the last refresh or "Load more" failed, while the list shows.
  refresh_error: Option<String>,
  query: Entity<InputState>,
  creating: bool,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
}

impl IssuesScreen {
  pub(super) fn new(repo: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search loaded issues"));
    cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        cx.notify();
      }
    })
    .detach();
    let mut this = Self {
      repo,
      filter: Filter::Open,
      list: Load::Idle,
      loading_more: false,
      refresh_error: None,
      query,
      creating: false,
      message: None,
      keys: IdempotencyKeys::default(),
    };
    this.show_cached();
    this.fetch(true, cx);
    this
  }

  fn show_cached(&mut self) {
    self.list = remote::last::<GhListPage<GhIssue>>(&list_request(&self.repo, self.filter, None))
      .map_or(Load::Idle, |page| Load::Ready(page.into()));
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.list.ready().is_none() {
      self.list = Load::Loading;
    }
    let filter = self.filter;
    let request = list_request(&self.repo, filter, None);
    load::run(
      cx,
      remote::read_cached::<GhListPage<GhIssue>>(request, join),
      move |this, result, _| {
        if this.filter != filter {
          return;
        }
        this.loading_more = false;
        match result {
          Ok(page) => {
            this.list = Load::Ready(page.into());
            this.refresh_error = None;
          }
          Err(error) if this.list.ready().is_some() => {
            this.refresh_error = Some(format!(
              "Couldn't refresh: {}",
              errors::describe(&error).message
            ));
          }
          Err(error) => this.list = Load::Failed(error),
        }
      },
    );
    cx.notify();
  }

  fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
    if self.filter == filter {
      return;
    }
    self.filter = filter;
    self.refresh_error = None;
    self.show_cached();
    self.fetch(true, cx);
  }

  fn load_more(&mut self, cx: &mut Context<Self>) {
    let Some(after) = self.list.ready().and_then(|l| l.end_cursor.clone()) else {
      return;
    };
    if self.loading_more {
      return;
    }
    self.loading_more = true;
    let filter = self.filter;
    load::run(
      cx,
      remote::read::<GhListPage<GhIssue>>(list_request(&self.repo, filter, Some(after))),
      move |this, result, _| {
        if this.filter != filter {
          return;
        }
        this.loading_more = false;
        match result {
          Ok(page) => {
            if let Load::Ready(listed) = &mut this.list {
              append_page(&mut listed.items, page.items);
              listed.has_more = page.has_more;
              listed.end_cursor = page.end_cursor;
            }
            this.refresh_error = None;
          }
          Err(error) => {
            this.refresh_error = Some(format!(
              "Couldn't load more: {}",
              errors::describe(&error).message
            ));
          }
        }
      },
    );
    cx.notify();
  }

  pub(super) fn menu(&mut self, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    let new = this.clone();
    sheet::menu(
      "Issues",
      vec![
        sheet::MenuEntry::new("menu-new-issue", "New issue", move |_, cx| {
          new.update(cx, |screen, cx| screen.create_sheet(cx)).ok();
        }),
        sheet::MenuEntry::new("menu-refresh", "Refresh", move |_, cx| {
          this.update(cx, |screen, cx| screen.reload(cx)).ok();
        }),
      ],
      cx,
    );
  }

  fn create_sheet(&mut self, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    form(
      "New issue",
      String::new(),
      String::new(),
      "Create issue",
      Rc::new(move |title, body, cx| {
        this
          .update(cx, |screen, cx| screen.create(title, body, cx))
          .ok();
      }),
      cx,
    );
  }

  fn create(&mut self, title: String, body: String, cx: &mut Context<Self>) {
    if self.creating {
      return;
    }
    let inputs = [self.repo.clone(), title.clone(), body.clone()];
    let key = self
      .keys
      .key_for("issue-create", &[&inputs[0], &inputs[1], &inputs[2]]);
    self.creating = true;
    self.message = None;
    let request = TreqCommandRequest::GithubWrite {
      repo: self.repo.clone(),
      action: GithubAction::CreateIssue { title, body },
      idempotency_key: key,
    };
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      this.creating = false;
      let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
      this.keys.settle("issue-create", &refs, &outcome);
      let created = match &outcome {
        Ok(Mutation::Applied(value)) => value.get("number").and_then(|n| n.as_u64()),
        _ => None,
      };
      this.message = Some(match &outcome {
        Ok(Mutation::Applied(_)) => (
          true,
          created.map_or("Issue created.".into(), |n| format!("Issue #{n} created.")),
        ),
        other => (
          false,
          errors::describe(&mutation_message(other, "").1).message,
        ),
      });
      if this.filter != Filter::Open {
        this.filter = Filter::Open;
        this.show_cached();
      }
      this.reload(cx);
      if let Some(number) = created {
        this.open_issue(number, format!("#{number}"), cx);
      }
    });
    cx.notify();
  }

  fn open_issue(&mut self, number: u64, title: String, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    issue::open(
      self.repo.clone(),
      number,
      title,
      Rc::new(move |cx| {
        this.update(cx, |screen, cx| screen.reload(cx)).ok();
      }),
      cx,
    );
  }

  fn row(&self, issue: &GhIssue, cx: &mut Context<Self>) -> AnyElement {
    let theme = cx.theme();
    let palette = treq_ui::theme::current(cx);
    let number = issue.number;
    let title = issue.title.clone();
    let tone = if issue.state == "CLOSED" {
      Tone::Neutral
    } else {
      Tone::Success
    };
    let mut detail = format!("#{number} · {}", issue.author.login);
    if !issue.labels.is_empty() {
      let labels: Vec<&str> = issue.labels.iter().map(|l| l.name.as_str()).collect();
      detail = format!("{detail} · {}", labels.join(", "));
    }
    h_flex()
      .id(SharedString::from(format!("issue-{number}")))
      .min_h(px(widgets::ROW_H))
      .px_4()
      .py_2()
      .gap_3()
      .items_center()
      .border_b_1()
      .border_color(theme.border)
      .active(|s| s.bg(theme.accent))
      .child(Dot::new(tone, &palette))
      .child(
        v_flex()
          .flex_1()
          .min_w_0()
          // Long titles wrap rather than cut off.
          .child(div().child(title.clone()))
          .child(
            div()
              .text_sm()
              .truncate()
              .text_color(theme.muted_foreground)
              .child(detail),
          ),
      )
      .child(
        div()
          .text_color(theme.muted_foreground)
          .child(IconName::ChevronRight),
      )
      .on_click(cx.listener(move |this, _, _, cx| {
        this.open_issue(number, title.clone(), cx);
      }))
      .into_any_element()
  }

  fn filter_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let button = |id: &'static str, label: &'static str, filter: Filter, current: Filter| {
      Button::new(id)
        .large()
        .flex_1()
        .h(px(44.))
        .label(label)
        .map(|b| {
          if filter == current {
            b.primary()
          } else {
            b.outline()
          }
        })
    };
    h_flex()
      .gap_2()
      .child(
        button("filter-open", "Open", Filter::Open, self.filter)
          .on_click(cx.listener(|this, _, _, cx| this.set_filter(Filter::Open, cx))),
      )
      .child(
        button("filter-closed", "Closed", Filter::Closed, self.filter)
          .on_click(cx.listener(|this, _, _, cx| this.set_filter(Filter::Closed, cx))),
      )
  }
}

impl Render for IssuesScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let query = self.query.read(cx).value().to_string();
    let body = match &self.list {
      Load::Idle | Load::Loading => {
        widgets::card(cx).child(widgets::centered_message("Loading issues…", cx))
      }
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .gap_3()
        .child(widgets::error_text(errors::describe(error).message, cx))
        .child(
          h_flex().child(
            Button::new("retry")
              .outline()
              .large()
              .label("Retry")
              .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
          ),
        ),
      Load::Ready(listed) if listed.items.is_empty() => {
        widgets::card(cx).child(widgets::centered_message(
          match self.filter {
            Filter::Open => "No open issues.",
            Filter::Closed => "No closed issues.",
          },
          cx,
        ))
      }
      Load::Ready(listed) => {
        let shown: Vec<GhIssue> = listed
          .items
          .iter()
          .filter(|i| matches(i, &query))
          .cloned()
          .collect();
        if shown.is_empty() {
          widgets::card(cx).child(widgets::centered_message("No loaded issues match.", cx))
        } else {
          let rows: Vec<AnyElement> = shown.iter().map(|i| self.row(i, cx)).collect();
          widgets::card(cx).children(rows)
        }
      }
    };
    let has_more = self
      .list
      .ready()
      .is_some_and(|l| l.has_more && l.end_cursor.is_some());
    let (border, background) = (cx.theme().border, cx.theme().background);
    v_flex()
      .size_full()
      .child(
        div()
          .id("issues")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .child(
            widgets::page()
              .gap_3()
              .child(self.filter_bar(cx))
              .child(Input::new(&self.query).large().cleanable(true))
              .children(
                self
                  .refresh_error
                  .clone()
                  .map(|error| widgets::error_text(error, cx)),
              )
              .children(self.message.clone().map(|(ok, text)| {
                if ok {
                  widgets::muted(text, cx)
                } else {
                  widgets::error_text(text, cx)
                }
              }))
              .child(body)
              .when(has_more, |page| {
                page.child(
                  Button::new("load-more")
                    .outline()
                    .large()
                    .w_full()
                    .h(px(44.))
                    .label("Load more")
                    .loading(self.loading_more)
                    .on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
                )
              }),
          ),
      )
      .child(
        h_flex()
          .p_3()
          .border_t_1()
          .border_color(border)
          .bg(background)
          .child(
            Button::new("new-issue")
              .primary()
              .large()
              .w_full()
              .h(px(widgets::CTA_H - 8.))
              .icon(IconName::Plus)
              .label("New issue")
              .loading(self.creating)
              .on_click(cx.listener(|this, _, _, cx| this.create_sheet(cx))),
          ),
      )
  }
}

/// What [`FormSheet`] hands back: the title and body.
pub type OnSubmit = Rc<dyn Fn(String, String, &mut App)>;

/// Opens the sheet that writes an issue's title and body.
pub fn form(
  heading: &str,
  title: String,
  body: String,
  submit_label: &'static str,
  on_submit: OnSubmit,
  cx: &mut App,
) {
  sheet::open(Some(heading.to_string().into()), cx, move |window, cx| {
    cx.new(|cx| FormSheet::new(title, body, submit_label, on_submit, window, cx))
  });
}

/// Whether a form with `title` can be sent: GitHub needs a title.
pub fn title_ready(title: &str) -> bool {
  !title.trim().is_empty()
}

/// An issue's title and body, new or edited.
pub struct FormSheet {
  title: Entity<InputState>,
  body: Entity<TextareaState>,
  submit_label: &'static str,
  error: Option<String>,
  on_submit: OnSubmit,
}

impl FormSheet {
  fn new(
    title: String,
    body: String,
    submit_label: &'static str,
    on_submit: OnSubmit,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let title = cx.new(|cx| {
      InputState::new(window, cx)
        .placeholder("Title")
        .default_value(title)
    });
    // Redraw as the title changes, so the submit button follows it.
    cx.subscribe(&title, |_, _, _: &InputEvent, cx| cx.notify())
      .detach();
    Self {
      title,
      body: cx.new(|cx| {
        TextareaState::new(window, cx)
          .placeholder("Description (optional)")
          .rows(6)
          .default_value(body)
      }),
      submit_label,
      error: None,
      on_submit,
    }
  }

  fn submit(&mut self, cx: &mut Context<Self>) {
    let title = self.title.read(cx).value().trim().to_string();
    if !title_ready(&title) {
      return;
    }
    let body = self.body.read(cx).value().to_string();
    sheet::close(cx);
    (self.on_submit)(title, body, cx);
  }
}

impl Render for FormSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    v_flex()
      .px_4()
      .pb_4()
      .gap_3()
      .child(Input::new(&self.title).large())
      .child(Textarea::new(&self.body))
      .when_some(self.error.clone(), |sheet, error| {
        sheet.child(widgets::error_text(error, cx))
      })
      .child(
        Button::new("issue-form-submit")
          .primary()
          .large()
          .w_full()
          .h(px(widgets::CTA_H - 8.))
          .label(self.submit_label)
          .disabled(!title_ready(&self.title.read(cx).value()))
          .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use treq_lib::github::GhAuthor;

  #[core::prelude::v1::test]
  fn an_issue_needs_a_title_to_submit() {
    assert!(!title_ready(""));
    assert!(!title_ready("  \n"));
    assert!(title_ready("Crash on start"));
  }

  fn issue(number: u64, title: &str) -> GhIssue {
    GhIssue {
      number,
      title: title.into(),
      state: "OPEN".into(),
      url: String::new(),
      body: None,
      author: GhAuthor {
        login: "ty".into(),
        avatar_url: None,
      },
      labels: Vec::new(),
      created_at: String::new(),
      updated_at: String::new(),
      comments: None,
    }
  }

  fn group(content: &str, count: u32, mine: bool) -> GhReactionGroup {
    GhReactionGroup {
      content: content.into(),
      count,
      viewer_has_reacted: mine,
    }
  }

  #[core::prelude::v1::test]
  fn search_matches_titles_and_numbers() {
    let i = issue(42, "Crash on Launch");
    assert!(matches(&i, ""));
    assert!(matches(&i, "  launch "));
    assert!(matches(&i, "#42"));
    assert!(matches(&i, "42"));
    assert!(!matches(&i, "#4"));
    assert!(!matches(&i, "settings"));
  }

  #[core::prelude::v1::test]
  fn states_read_as_words() {
    assert_eq!(state_label("OPEN"), "Open");
    assert_eq!(state_label("CLOSED"), "Closed");
  }

  #[core::prelude::v1::test]
  fn pages_append_without_duplicates() {
    let mut items = vec![issue(3, "a"), issue(2, "b")];
    append_page(&mut items, vec![issue(2, "b"), issue(1, "c")]);
    let numbers: Vec<u64> = items.iter().map(|i| i.number).collect();
    assert_eq!(numbers, [3, 2, 1]);
  }

  #[core::prelude::v1::test]
  fn reactions_toggle_counts_and_drop_empty_groups() {
    let mut groups = vec![group("HEART", 1, true), group("EYES", 2, false)];
    assert!(!toggle_reaction(&mut groups, "HEART"));
    assert_eq!(groups, [group("EYES", 2, false)]);
    assert!(toggle_reaction(&mut groups, "EYES"));
    assert_eq!(groups, [group("EYES", 3, true)]);
    assert!(toggle_reaction(&mut groups, "ROCKET"));
    assert_eq!(groups.last(), Some(&group("ROCKET", 1, true)));
  }

  #[core::prelude::v1::test]
  fn emoji_for_known_reactions() {
    assert_eq!(emoji("EYES"), "👀");
    assert_eq!(emoji("UNKNOWN"), "?");
  }
}
