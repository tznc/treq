//! The workspace Logs tab (`LogsTab`): the repository's check logs with a
//! chart and an SQL explorer, its agent conversations, and the app's setup
//! script and system logs, each as a feed whose lines go to an agent.
//!
//! Read from the backend directly ([`logs_data`]). The prompt for the
//! picked lines goes to the workspace view (`logs.sendToAgent`), which has
//! the backend start an agent on it.
//!
//! [`logs_data`]: super::logs_data

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::{zpx, DismissExt as _};
use crate::result_ext::ResultExt;
use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc, time::Duration};

use gpui_kit::component::input::{Escape, InputEvent, InputState, Textarea, TextareaState};
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, uniform_list, App, Bounds, Context, Div,
  Entity, FontWeight, Hsla, MouseButton, Pixels, ScrollStrategy, SharedString, StyledText,
  Subscription, Task, UniformListScrollHandle, Window,
};

use super::{
  logs_data::{
    agent_chat_records, chart, filter_label, find_matches, format_timestamp, log_lines_prompt,
    sql_result_prompt, toggle, Chart, Host, LineSelection, Source, DEFAULT_QUERY, LEVELS, SERIES,
    SETUP_SCRIPT_SOURCE, TEMPLATES,
  },
  ActionSink, ScreenAction,
};
use crate::{
  core::{
    agent_chat::{AgentChat, AgentChatSummary},
    checks::WorkflowInfo,
    checks_logs::{LogQuery, LogRecordView, SqlResult},
  },
  gpui_shell::{
    popup::{native_entries, NativeMenuItem, PopupRequest, PopupSink},
    theme::{self, Palette},
    ui::{self, Button, Size, TextSize, Variant},
  },
};

/// How long typing in a search box waits before reading again.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
/// A `py-0.5` row of `leading-relaxed` text-xs, as React draws it.
const ROW_HEIGHT: f32 = 21.;
const CHART_HEIGHT: f32 = 170.;

/// The feed's columns: the timestamp, any per-source ones, then level.
const TIMESTAMP_WIDTH: f32 = 110.;
const LEVEL_WIDTH: f32 = 67.;

/// A `PrefixColumn`: header, width and what a row shows.
#[derive(Clone, Copy)]
enum Column {
  Run,
  Job,
  Role,
  Source,
  File,
  Outcome,
}

impl Column {
  fn header(self) -> &'static str {
    match self {
      Column::Run => "Run",
      Column::Job => "Job",
      Column::Role => "Role",
      Column::Source => "Source",
      Column::File => "File",
      Column::Outcome => "Outcome",
    }
  }

  fn width(self) -> f32 {
    match self {
      Column::Run | Column::Role => 67.,
      Column::Outcome => 80.,
      Column::File => 197.,
      Column::Job => 96.,
      Column::Source => 197.,
    }
  }

  fn cell(self, record: &LogRecordView) -> String {
    match self {
      Column::Run => format!("#{}", record.run_id),
      Column::Job | Column::Role | Column::Source | Column::File => record.job_id.clone(),
      Column::Outcome => record.step_name.clone(),
    }
  }
}

/// What a feed shows, by source.
struct FeedSpec {
  output: &'static str,
  line: &'static str,
  columns: &'static [Column],
  /// Whose lines, in the prompt (`promptLabel`).
  prompt: &'static str,
}

fn feed_spec(source: Source) -> FeedSpec {
  match source {
    Source::Checks => FeedSpec {
      output: "repo-logs-output",
      line: "repo-log-line",
      columns: &[Column::Run, Column::Job],
      prompt: "my check logs",
    },
    Source::Agent => FeedSpec {
      output: "agent-chat-output",
      line: "agent-chat-line",
      columns: &[Column::Role],
      prompt: "an agent chat",
    },
    Source::AppSetup => FeedSpec {
      output: "app-setup-logs-output",
      line: "app-setup-log-line",
      columns: &[Column::Run],
      prompt: "my app setup script logs",
    },
    Source::System => FeedSpec {
      output: "system-logs-output",
      line: "system-log-line",
      columns: &[Column::Source],
      prompt: "the Treq app's system logs",
    },
    Source::Reviews => FeedSpec {
      output: "review-history-output",
      line: "review-history-line",
      columns: &[Column::Outcome, Column::File],
      prompt: "my code review history",
    },
  }
}

/// The multi-select filter whose list is open.
#[derive(Clone, Copy, PartialEq)]
enum Open {
  Levels,
  Workflows,
  Templates,
}

/// `LogFeed`'s find bar (`SearchOverlay`).
#[derive(Default)]
struct Find {
  open: bool,
  query: String,
  current: usize,
  input: Option<Entity<InputState>>,
  focus: bool,
  _subscription: Option<Subscription>,
}

/// The explorer's query and its last result.
#[derive(Default)]
struct Explorer {
  sql: String,
  input: Option<Entity<TextareaState>>,
  result: Option<SqlResult>,
  error: Option<String>,
  running: bool,
  _subscription: Option<Subscription>,
}

pub struct LogsView {
  host: Host,
  react: ActionSink,
  popups: PopupSink,
  repo_path: String,
  dark: bool,
  source: Source,
  explorer_open: bool,
  levels: Vec<String>,
  checks: Vec<String>,
  workflows: Vec<WorkflowInfo>,
  search: String,
  search_input: Option<Entity<InputState>>,
  /// `None` while reading.
  records: Option<Vec<LogRecordView>>,
  /// The source `records` were read for, while they are lines of checks.
  records_for: Option<Source>,
  /// The chart above the feed, built once per read.
  chart: Chart,
  chats: Option<Vec<AgentChatSummary>>,
  chat_id: Option<i64>,
  chat: Option<AgentChat>,
  /// Bumped per read, so a slower earlier one is dropped.
  generation: u64,
  selection: LineSelection,
  find: Find,
  explorer: Explorer,
  open: Option<Open>,
  anchors: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
  scroll: UniformListScrollHandle,
  debounce: Option<Task<()>>,
  find_key_watched: bool,
  /// Hidden while another tab shows, so Cmd+F is another view's.
  hidden: bool,
  /// The lines drawn, and the find bar's matches among them.
  visible: Rc<Vec<LogRecordView>>,
  matches: Vec<usize>,
  _subscriptions: Vec<Subscription>,
}

impl LogsView {
  pub fn new(
    repo_path: String,
    source: Option<&str>,
    host: Host,
    react: ActionSink,
    popups: PopupSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let mut this = Self {
      host,
      react,
      popups,
      repo_path,
      dark,
      source: source.map(Source::parse).unwrap_or_default(),
      explorer_open: false,
      levels: Vec::new(),
      // The setup script's runs are stored as a check of their own.
      checks: if source == Some(SETUP_SCRIPT_SOURCE) {
        vec![crate::core::SETUP_SCRIPT_FILENAME.to_string()]
      } else {
        Vec::new()
      },
      workflows: Vec::new(),
      search: String::new(),
      search_input: None,
      records: None,
      records_for: None,
      chart: Chart::default(),
      chats: None,
      chat_id: None,
      chat: None,
      generation: 0,
      selection: LineSelection::default(),
      find: Find::default(),
      explorer: Explorer {
        sql: DEFAULT_QUERY.into(),
        ..Explorer::default()
      },
      open: None,
      anchors: Rc::default(),
      scroll: UniformListScrollHandle::new(),
      debounce: None,
      find_key_watched: false,
      hidden: false,
      visible: Rc::default(),
      matches: Vec::new(),
      _subscriptions: Vec::new(),
    };
    this.read_workflows(cx);
    this.load(cx);
    this
  }

  /// Hidden while another tab shows; it keeps what it read.
  pub fn set_shown(&mut self, shown: bool) {
    self.hidden = !shown;
  }

  pub fn set_repo(&mut self, repo_path: String, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
    if self.repo_path != repo_path {
      self.repo_path = repo_path;
      self.checks.clear();
      self.chats = None;
      self.chat_id = None;
      self.read_workflows(cx);
      self.load(cx);
    }
  }

  /// Runs `work` off the main thread, then `apply`s it unless another read
  /// started meanwhile.
  fn run<T: Send + 'static>(
    &mut self,
    work: impl FnOnce(&dyn super::logs_data::LogsHost, &str) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let (host, repo) = (self.host.clone(), self.repo_path.clone());
    let generation = self.generation;
    cx.spawn(async move |this, cx| {
      let value = cx
        .background_spawn(async move { work(host.as_ref(), &repo) })
        .await;
      this
        .update(cx, |view, cx| {
          if view.generation == generation {
            apply(view, value, cx);
            cx.notify();
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  fn read_workflows(&mut self, cx: &mut Context<Self>) {
    let (host, repo) = (self.host.clone(), self.repo_path.clone());
    cx.spawn(async move |this, cx| {
      let workflows = cx
        .background_spawn(async move { host.workflows(&repo) })
        .await;
      this
        .update(cx, |view, cx| {
          match workflows {
            Ok(workflows) => view.workflows = workflows,
            Err(err) => log::warn!("Failed to list workflows: {err}"),
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn query(&self) -> LogQuery {
    LogQuery {
      levels: (!self.levels.is_empty()).then(|| self.levels.clone()),
      search: (!self.search.is_empty()).then(|| self.search.clone()),
      workflows: (self.source == Source::Checks).then(|| self.checks.clone()),
      ..LogQuery::default()
    }
  }

  /// Reads the source's lines again (`useSWR` on the filters).
  fn load(&mut self, cx: &mut Context<Self>) {
    self.generation += 1;
    self.selection.clear();
    let query = self.query();
    if self.source != Source::Checks {
      self.records_for = None;
    }
    match self.source {
      Source::Agent => return self.load_chats(cx),
      Source::Checks => {
        // A new filter keeps the lines shown until the new ones arrive.
        if self.records_for != Some(Source::Checks) {
          self.records = None;
        }
        let chart_query = query.clone();
        self.run(
          move |host, repo| host.repo_logs(repo, &query),
          |view, records, _| {
            view.records = Some(records.unwrap_or_else(|err| {
              log::warn!("Failed to read check logs: {err}");
              Vec::new()
            }));
            view.records_for = Some(Source::Checks);
          },
          cx,
        );
        // The chart is read on its own, so the lines paint first.
        self.run(
          move |host, repo| chart(&host.log_timeseries(repo, &chart_query).unwrap_or_default()),
          |view, chart, _| view.chart = chart,
          cx,
        );
      }
      Source::AppSetup | Source::System | Source::Reviews => {
        self.records = None;
        self.chart = Chart::default();
        let source = self.source;
        self.run(
          move |host, repo| match source {
            Source::System => host.system_logs(&query),
            Source::Reviews => host.review_history(repo, &query),
            _ => host.app_setup_logs(&query),
          },
          |view, records, _| {
            view.records = Some(records.unwrap_or_else(|err| {
              log::warn!("Failed to read logs: {err}");
              Vec::new()
            }));
          },
          cx,
        );
      }
    }
    cx.notify();
  }

  fn load_chats(&mut self, cx: &mut Context<Self>) {
    self.chart = Chart::default();
    if self.chats.is_none() {
      self.records = None;
      self.run(
        |host, repo| host.agent_chats(repo),
        |view, chats, cx| {
          let chats = chats.unwrap_or_else(|err| {
            log::warn!("Failed to list agent chats: {err}");
            Vec::new()
          });
          view.chat_id = view.chat_id.or(chats.first().map(|c| c.session_id));
          view.chats = Some(chats);
          view.read_chat(cx);
        },
        cx,
      );
      return;
    }
    self.read_chat(cx);
  }

  fn read_chat(&mut self, cx: &mut Context<Self>) {
    let Some(id) = self.chat_id else {
      self.chat = None;
      self.records = Some(Vec::new());
      return;
    };
    self.generation += 1;
    self.records = None;
    self.run(
      move |host, repo| host.agent_chat(repo, id),
      |view, chat, _| {
        view.chat = chat.unwrap_or_else(|err| {
          log::warn!("Failed to read agent chat: {err}");
          None
        });
        view.records = Some(
          view
            .chat
            .as_ref()
            .map(agent_chat_records)
            .unwrap_or_default(),
        );
      },
      cx,
    );
  }

  fn set_source(&mut self, source: Source, cx: &mut Context<Self>) {
    if self.source == source && !self.explorer_open {
      return;
    }
    self.source = source;
    self.explorer_open = false;
    self.levels.clear();
    self.search.clear();
    self.search_input = None;
    self.find = Find::default();
    self.load(cx);
  }

  fn set_search(&mut self, search: String, cx: &mut Context<Self>) {
    if self.search == search {
      return;
    }
    self.search = search;
    if self.source == Source::Agent {
      self.selection.clear();
      cx.notify();
      return;
    }
    self.debounce = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(SEARCH_DEBOUNCE).await;
      this.update(cx, |view, cx| view.load(cx)).ignore_closed();
    }));
  }

  /// The lines shown: the agent chat's are searched here, the rest by the
  /// backend.
  fn shown(&self) -> Vec<LogRecordView> {
    let records = self.records.clone().unwrap_or_default();
    if self.source != Source::Agent || self.search.is_empty() {
      return records;
    }
    let search = self.search.to_lowercase();
    records
      .into_iter()
      .filter(|r| r.body.to_lowercase().contains(&search))
      .collect()
  }

  fn send_to_agent(&self, prompt: String, cx: &mut App) {
    let react = self.react.clone();
    cx.defer(move |cx| {
      react(
        ScreenAction::Field {
          field: "logs.sendToAgent".into(),
          value: prompt.into(),
        },
        cx,
      )
    });
  }

  fn send_lines(&self, records: &[LogRecordView], cx: &mut App) {
    let label = match (self.source, &self.chat) {
      (Source::Agent, Some(chat)) => format!("agent chat \"{}\"", chat.name),
      (source, _) => feed_spec(source).prompt.to_string(),
    };
    self.send_to_agent(log_lines_prompt(records, &label), cx);
  }

  fn run_query(&mut self, cx: &mut Context<Self>) {
    self.explorer.running = true;
    self.explorer.error = None;
    let sql = self.explorer.sql.clone();
    let (host, repo) = (self.host.clone(), self.repo_path.clone());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move { host.run_sql(&repo, &sql) })
        .await;
      this
        .update(cx, |view, cx| {
          view.explorer.running = false;
          match result {
            Ok(result) => view.explorer.result = Some(result),
            Err(err) => {
              view.explorer.result = None;
              view.explorer.error = Some(err);
            }
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
    cx.notify();
  }

  fn popup(&self, request: PopupRequest, cx: &mut App) {
    let popups = self.popups.clone();
    cx.defer(move |cx| popups(request, cx));
  }

  /// A native `<select>`'s list, below the control drawn as `key`.
  fn dropdown(&self, key: &'static str, items: Vec<NativeMenuItem>, cx: &mut App) {
    let Some(anchor) = self.anchors.borrow().get(key).copied() else {
      return;
    };
    self.popup(
      PopupRequest::ShowSelect {
        entries: native_entries(items),
        anchor,
        dark: self.dark,
      },
      cx,
    );
  }

  fn source_menu(&self, cx: &mut Context<Self>) {
    let entity = cx.entity().downgrade();
    let current = self.source;
    let items = Source::ALL
      .into_iter()
      .map(|source| {
        let entity = entity.clone();
        NativeMenuItem::item(source.label(), move |cx| {
          entity
            .update(cx, |view, cx| view.set_source(source, cx))
            .ignore_closed();
        })
        .radio(source == current)
      })
      .collect();
    self.dropdown("log-source", items, cx);
  }

  fn chat_menu(&self, cx: &mut Context<Self>) {
    let Some(chats) = self.chats.as_ref().filter(|c| !c.is_empty()) else {
      return;
    };
    let entity = cx.entity().downgrade();
    let items = chats
      .iter()
      .map(|chat| {
        let (entity, id) = (entity.clone(), chat.session_id);
        NativeMenuItem::item(format!("{} · {}", chat.name, chat.agent), move |cx| {
          entity
            .update(cx, |view, cx| {
              view.chat_id = Some(id);
              view.selection.clear();
              view.read_chat(cx);
              cx.notify();
            })
            .ignore_closed();
        })
      })
      .collect();
    self.dropdown("agent-terminal", items, cx);
  }

  fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.search_input.is_none() {
      let placeholder = match self.source {
        Source::Agent => "Search this conversation…",
        Source::System => "Search system logs…",
        Source::Reviews => "Search reviews…",
        _ => "Search all runs…",
      };
      let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
      self
        ._subscriptions
        .push(cx.subscribe(&input, |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            let value = input.read(cx).value().to_string();
            view.set_search(value, cx);
          }
        }));
      self.search_input = Some(input);
    }
    if self.explorer_open && self.explorer.input.is_none() {
      let sql = self.explorer.sql.clone();
      let input = cx.new(|cx| TextareaState::new(window, cx).rows(6).default_value(sql));
      self.explorer._subscription =
        Some(cx.subscribe(&input, |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.explorer.sql = input.read(cx).value().to_string();
          }
        }));
      self.explorer.input = Some(input);
    }
    if self.find.open {
      let input = match &self.find.input {
        Some(input) => input.clone(),
        None => {
          let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
          self.find._subscription = Some(cx.subscribe(
            &input,
            |view, input, event: &InputEvent, cx| match event {
              InputEvent::Change => {
                view.find.query = input.read(cx).value().to_string();
                view.find.current = 0;
                cx.notify();
              }
              InputEvent::PressEnter { shift, .. } => {
                view.step_match(if *shift { -1 } else { 1 }, cx)
              }
              _ => {}
            },
          ));
          self.find.input = Some(input.clone());
          input
        }
      };
      if std::mem::take(&mut self.find.focus) {
        input.update(cx, |input, cx| {
          input.focus(window, cx);
          input.select_all(window, cx);
        });
      }
    }
  }

  fn watch_find_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.find_key_watched {
      return;
    }
    self.find_key_watched = true;
    let id = window.window_handle().window_id();
    self
      ._subscriptions
      .push(cx.observe_keystrokes(move |view, event, window, cx| {
        let k = &event.keystroke;
        if window.window_handle().window_id() == id
          && k.key == "f"
          && (crate::gpui_shell::keys::held(k.modifiers).platform
            || crate::gpui_shell::keys::held(k.modifiers).control)
          && !crate::gpui_shell::keys::held(k.modifiers).shift
          && !crate::gpui_shell::keys::held(k.modifiers).alt
          && !view.explorer_open
          && !view.hidden
        {
          view.find.open = true;
          view.find.focus = true;
          cx.notify();
        }
      }));
  }

  fn step_match(&mut self, delta: isize, cx: &mut Context<Self>) {
    let matches = find_matches(&self.shown(), &self.find.query);
    if matches.is_empty() {
      return;
    }
    let n = matches.len() as isize;
    self.find.current = (self.find.current as isize + delta).rem_euclid(n) as usize;
    self
      .scroll
      .scroll_to_item(matches[self.find.current], ScrollStrategy::Center);
    cx.notify();
  }
}

/// CSS `capitalize`: each word's first letter in upper case.
fn capitalize(text: &str) -> String {
  text
    .split(' ')
    .map(|word| {
      let mut chars = word.chars();
      chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
    })
    .collect::<Vec<String>>()
    .join(" ")
}

fn severity_color(severity: &str, p: &Palette) -> Hsla {
  match severity {
    "ERROR" => ui::Tone::Danger.text(p),
    "WARN" => ui::Tone::Warning.text(p),
    _ => p.foreground,
  }
}

/// A cell of the feed: `px-3 truncate whitespace-nowrap`.
fn cell(width: Option<f32>) -> Div {
  let el = div()
    .px_3()
    .overflow_hidden()
    .whitespace_nowrap()
    .text_ellipsis();
  match width {
    Some(w) => el.flex_none().w(zpx(w)),
    None => el.flex_1().min_w_0(),
  }
}

/// A checkbox row of a filter's list (`DropdownMenuCheckboxItem`).
fn check_row(id: SharedString, label: String, checked: bool, p: &Palette) -> gpui::Stateful<Div> {
  let accent = p.accent;
  ui::text(div(), TextSize::Sm)
    .id(id.clone())
    .debug_selector(move || id.to_string())
    .relative()
    .flex()
    .items_center()
    .rounded(radius::SM)
    .py_1p5()
    .pl_8()
    .pr_2()
    .cursor_pointer()
    .hover(move |s| s.bg(accent))
    .when(checked, |this| {
      this.child(
        div()
          .absolute()
          .left_2()
          .size(zpx(14.))
          .flex()
          .items_center()
          .justify_center()
          .child(ui::icon(theme::ICON_CHECK, 16., p.foreground)),
      )
    })
    .child(label)
}

impl LogsView {
  /// Records the bounds of its parent as `key`'s.
  fn anchor(&self, key: &'static str) -> impl IntoElement {
    let anchors = self.anchors.clone();
    gpui::canvas(
      move |b, _, _| {
        anchors.borrow_mut().insert(key, b);
      },
      |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
  }

  /// A native `<select>`: `h-8 rounded-md border bg-background px-2`.
  fn select(
    &self,
    key: &'static str,
    label: String,
    min_width: f32,
    disabled: bool,
    p: &Palette,
  ) -> gpui::Stateful<Div> {
    ui::text(div(), TextSize::Sm)
      .id(key)
      .debug_selector(move || key.into())
      .relative()
      .h_8()
      .min_w(zpx(min_width))
      .px_2()
      .flex()
      .items_center()
      .justify_between()
      .gap_2()
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .bg(p.background)
      .when(disabled, |this| this.opacity(0.5))
      .when(!disabled, |this| this.cursor_pointer())
      .child(self.anchor(key))
      .child(label)
      .child(ui::icon(theme::ICON_CHEVRON_DOWN, 14., p.muted_foreground))
  }

  fn search_box(&self, p: &Palette) -> impl IntoElement {
    let label = match self.source {
      Source::Agent => "agent-chat-search",
      Source::System => "system-logs-search",
      Source::AppSetup => "app-setup-logs-search",
      Source::Reviews => "review-history-search",
      Source::Checks => "repo-logs-search",
    };
    ui::text(div(), TextSize::Sm)
      .debug_selector(move || label.into())
      .ml_auto()
      .h_8()
      .w(zpx(384.))
      .min_w(zpx(160.))
      .px_2()
      .flex()
      .items_center()
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .bg(p.background)
      .children(
        self
          .search_input
          .as_ref()
          .map(|i| ui::input(i).appearance(false).bordered(false).px_0()),
      )
  }

  fn top_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let browsing = !self.explorer_open;
    div()
      .flex()
      .items_center()
      .justify_between()
      .gap_2()
      .px_4()
      .py_3()
      .border_b_1()
      .border_color(p.border)
      .child(
        self
          .select("log-source", self.source.label().into(), 0., false, p)
          .on_click(cx.listener(|view, _, _, cx| view.source_menu(cx))),
      )
      .when(self.source == Source::Checks, |this| {
        this.child(
          div()
            .flex()
            .items_center()
            .gap_1()
            .child(
              Button::new("logs-browse-view", p)
                .size(Size::Sm)
                .gap_3()
                .variant(if browsing {
                  Variant::Secondary
                } else {
                  Variant::Ghost
                })
                .icon(theme::ICON_TABLE, 12.)
                .label("Browse")
                .on_click(cx.listener(|view, _, _, cx| {
                  view.explorer_open = false;
                  cx.notify();
                })),
            )
            .child(
              Button::new("logs-explorer-view", p)
                .size(Size::Sm)
                .gap_3()
                .variant(if browsing {
                  Variant::Ghost
                } else {
                  Variant::Secondary
                })
                .icon(theme::ICON_DATABASE, 12.)
                .label("Logs Explorer")
                .on_click(cx.listener(|view, _, _, cx| {
                  view.explorer_open = true;
                  view.find.open = false;
                  cx.notify();
                })),
            ),
        )
      })
  }

  /// `MultiSelectFilter`'s trigger.
  fn filter_button(
    &self,
    key: &'static str,
    label: String,
    open: Open,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    div().relative().child(self.anchor(key)).child(
      Button::new(key, p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .min_w(zpx(130.))
        .justify_between()
        .label(label)
        .child(ui::icon(
          theme::ICON_CHEVRON_DOWN,
          12.,
          p.foreground.opacity(0.6),
        ))
        .on_click(cx.listener(move |view, _, _, cx| {
          view.open = (view.open != Some(open)).then_some(open);
          cx.notify();
        })),
    )
  }

  fn filter_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let levels: Vec<(String, String)> = LEVELS
      .iter()
      .map(|l| (l.to_string(), capitalize(l)))
      .collect();
    let level_label = capitalize(&filter_label(&self.levels, &levels, "All levels", "levels"));
    let mut bar = div()
      .flex()
      .flex_wrap()
      .items_center()
      .gap_2()
      .px_4()
      .py_2()
      .border_b_1()
      .border_color(p.border)
      .bg(p.muted.opacity(0.3))
      .child(self.filter_button("log-level-filter", level_label, Open::Levels, p, cx));
    match self.source {
      Source::Checks if !self.workflows.is_empty() || !self.checks.is_empty() => {
        let options: Vec<(String, String)> = self
          .workflows
          .iter()
          .map(|w| (w.filename.clone(), w.name.clone()))
          .chain(
            self
              .checks
              .iter()
              .any(|c| c == crate::core::SETUP_SCRIPT_FILENAME)
              .then(|| {
                (
                  crate::core::SETUP_SCRIPT_FILENAME.to_string(),
                  "Setup script".to_string(),
                )
              }),
          )
          .collect();
        let label = filter_label(&self.checks, &options, "All checks", "checks");
        bar = bar.child(self.filter_button("check-workflow-filter", label, Open::Workflows, p, cx));
      }
      Source::System => {
        bar = bar.child(
          Button::new("open-log-folder", p)
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .gap_3()
            .icon(theme::ICON_FOLDER_OPEN, 12.)
            .label("Open log folder")
            .on_click(cx.listener(|view, _, _, _| {
              if let Err(err) = view.host.open_system_log_dir() {
                log::warn!("Failed to open the log folder: {err}");
              }
            })),
        );
      }
      _ => {}
    }
    bar.child(self.search_box(p))
  }

  fn agent_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let chats = self.chats.as_deref().unwrap_or_default();
    let label = chats
      .iter()
      .find(|c| Some(c.session_id) == self.chat_id)
      .map_or_else(
        || "No agent terminals".to_string(),
        |c| format!("{} · {}", c.name, c.agent),
      );
    div()
      .flex()
      .flex_wrap()
      .items_center()
      .gap_2()
      .px_4()
      .py_2()
      .border_b_1()
      .border_color(p.border)
      .bg(p.muted.opacity(0.3))
      .child(
        self
          .select("agent-terminal", label, 192., chats.is_empty(), p)
          .on_click(cx.listener(|view, _, _, cx| view.chat_menu(cx))),
      )
      .child(self.search_box(p))
  }

  fn pending(&self, text: &'static str, p: &Palette) -> Div {
    div()
      .p_3()
      .child(ui::Loading::new("logs-loading", p).label(text))
  }

  fn empty_message(&self) -> &'static str {
    match self.source {
      Source::Checks if !self.checks.is_empty() => "No logs recorded for the selected checks yet.",
      Source::Checks => {
        "No check logs recorded yet. Run checks from the Checks button in a workspace header to populate this table."
      }
      Source::Agent => {
        "No agent chat logs yet. Open an agent terminal (not a shell) to start recording the conversation."
      }
      Source::AppSetup => {
        "No app setup script runs yet. Set a script in Application settings and click Run."
      }
      Source::System => "No system logs recorded yet.",
      Source::Reviews => {
        "No code reviews in the last 30 days. Review agent comments and reviews you send to an agent show here."
      }
    }
  }

  /// `LogFeed`.
  fn feed(&mut self, records: Rc<Vec<LogRecordView>>, p: &Palette, cx: &mut Context<Self>) -> Div {
    let spec = feed_spec(self.source);
    if records.is_empty() {
      return ui::text(div(), TextSize::Xs)
        .debug_selector(move || spec.output.into())
        .flex_1()
        .font_family(theme::MONO_FONT)
        .p_3()
        .text_color(p.muted_foreground)
        .child(self.empty_message());
    }
    let selected = self.selection.selected.len();
    let multi = self.selection.multi;
    let toolbar = ui::text(div(), TextSize::Xs)
      .flex()
      .items_center()
      .gap_2()
      .px_3()
      .py_1p5()
      .border_b_1()
      .border_color(p.border)
      .bg(p.muted.opacity(0.3))
      .child(
        Button::new("multi-select-toggle", p)
          .size(Size::Sm)
          .gap_3()
          .variant(if multi {
            Variant::Secondary
          } else {
            Variant::Ghost
          })
          .icon(
            if multi {
              theme::ICON_SQUARE_CHECK
            } else {
              theme::ICON_SQUARE
            },
            12.,
          )
          .label("Multi-select")
          .on_click(cx.listener(|view, _, _, cx| {
            view.selection.toggle_multi();
            cx.notify();
          })),
      )
      .child({
        let n = records.len();
        Button::new("select-all", p)
          .size(Size::Sm)
          .variant(Variant::Ghost)
          .label("Select all")
          .on_click(cx.listener(move |view, _, _, cx| {
            view.selection.select_all(n);
            cx.notify();
          }))
      })
      .when(selected > 0, |this| {
        this
          .child(
            div()
              .debug_selector(|| "selection-count".into())
              .text_color(p.muted_foreground)
              .child(format!("{selected} selected")),
          )
          .child(
            Button::new("clear-selection", p)
              .size(Size::Sm)
              .variant(Variant::Ghost)
              .label("Clear")
              .on_click(cx.listener(|view, _, _, cx| {
                view.selection.clear();
                cx.notify();
              })),
          )
      });
    let header = div()
      .flex()
      .flex_none()
      .py_1()
      .bg(p.muted)
      .text_color(p.muted_foreground)
      .border_b_1()
      .border_color(p.border)
      .child(cell(Some(TIMESTAMP_WIDTH)).child("TIMESTAMP"))
      .children(
        spec
          .columns
          .iter()
          .map(|c| cell(Some(c.width())).child(c.header().to_uppercase())),
      )
      .child(cell(Some(LEVEL_WIDTH)).child("LEVEL"))
      .child(cell(None).child("MESSAGE"));
    let list = uniform_list(
      "log-feed",
      records.len(),
      cx.processor(move |view, range: Range<usize>, _, cx| view.rows(&records, range, cx)),
    )
    .track_scroll(&self.scroll)
    .flex_1()
    .min_h_0();
    let inspected = (selected == 1)
      .then(|| self.selection.selected.first().copied())
      .flatten()
      .and_then(|i| self.visible.get(i).cloned());
    div()
      .flex_1()
      .flex()
      .flex_col()
      .min_h_0()
      .child(toolbar)
      .child(
        div()
          .relative()
          .flex_1()
          .min_h_0()
          .child(
            ui::text(div(), TextSize::Xs)
              .debug_selector(move || spec.output.into())
              .size_full()
              .flex()
              .flex_col()
              .font_family(theme::MONO_FONT)
              .child(header)
              .child(list),
          )
          .children(self.find_bar(p, cx)),
      )
      .children(inspected.map(|record| self.inspect_panel(&record, p, cx)))
  }

  fn rows(
    &self,
    records: &[LogRecordView],
    range: Range<usize>,
    cx: &mut Context<Self>,
  ) -> Vec<gpui::AnyElement> {
    let p = theme::palette(self.dark);
    let spec = feed_spec(self.source);
    let query = self.find.query.trim().to_string();
    let mark = gpui::HighlightStyle {
      background_color: Some(ui::Tone::Warning.color(&p)),
      color: Some(theme::palette(false).foreground),
      ..Default::default()
    };
    range
      .filter_map(|i| records.get(i).map(|r| (i, r)))
      .map(|(i, record)| {
        let selected = self.selection.selected.contains(&i);
        let hover = p.muted.opacity(0.5);
        let body: gpui::AnyElement = if query.is_empty() {
          record.body.clone().into_any_element()
        } else {
          let marks: Vec<_> = super::changes::search::match_ranges(&record.body, &query)
            .into_iter()
            .map(|r| (r, mark))
            .collect();
          StyledText::new(record.body.clone())
            .with_highlights(marks)
            .into_any_element()
        };
        div()
          .id(("log-line", i))
          .debug_selector(move || spec.line.into())
          .h(zpx(ROW_HEIGHT))
          .w_full()
          .overflow_hidden()
          .flex()
          .items_center()
          .cursor_pointer()
          .when(selected, |this| this.bg(p.primary.opacity(0.2)))
          .when(!selected, |this| this.hover(move |s| s.bg(hover)))
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _, _, cx| {
              view.selection.mouse_down(i);
              cx.notify();
            }),
          )
          .on_mouse_move(cx.listener(move |view, _, _, cx| {
            if view.selection.dragging {
              view.selection.mouse_enter(i);
              cx.notify();
            }
          }))
          .child(
            cell(Some(TIMESTAMP_WIDTH))
              .text_color(p.muted_foreground)
              .child(format_timestamp(&record.timestamp)),
          )
          .children(spec.columns.iter().map(|c| {
            let color = match c {
              Column::Job | Column::Role | Column::File => ui::Tone::Primary.text(&p),
              Column::Run | Column::Source | Column::Outcome => p.muted_foreground,
            };
            cell(Some(c.width()))
              .text_color(color)
              .child(c.cell(record))
          }))
          .child(
            cell(Some(LEVEL_WIDTH))
              .text_color(severity_color(&record.severity_text, &p))
              .child(record.severity_text.clone()),
          )
          .child(cell(None).child(body))
          .into_any_element()
      })
      .collect()
  }

  fn inspect_panel(&self, record: &LogRecordView, p: &Palette, cx: &mut Context<Self>) -> Div {
    ui::text(div(), TextSize::Xs)
      .debug_selector(|| "log-inspect-panel".into())
      .flex_none()
      .border_t_1()
      .border_color(p.border)
      .bg(p.muted.opacity(0.2))
      .p_3()
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .gap_2()
          .child(
            div()
              .font_weight(FontWeight::BOLD)
              .child("Log line details"),
          )
          .child(
            Button::new("send-to-agent", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .gap_3()
              .icon(theme::ICON_BOT, 12.)
              .label("Send to agent")
              .on_click(cx.listener(|view, _, _, cx| {
                let chosen: Vec<_> = view
                  .selection
                  .selected
                  .iter()
                  .filter_map(|i| view.visible.get(*i).cloned())
                  .collect();
                view.send_lines(&chosen, cx);
              })),
          ),
      )
      .child(
        div()
          .id("log-inspect-body")
          .mt_2()
          .max_h(zpx(128.))
          .overflow_y_scroll()
          .font_family(theme::MONO_FONT)
          .whitespace_normal()
          .child(record.body.clone()),
      )
  }

  /// `SearchOverlay` over the feed.
  fn find_bar(&self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let input = self.find.input.clone().filter(|_| self.find.open)?;
    let total = self.matches.len();
    let shown = if total == 0 { 0 } else { self.find.current + 1 };
    let muted = p.muted_foreground;
    let empty = total == 0;
    let find_button =
      |id: &'static str, icon: &'static str, label: &'static str, disabled: bool| {
        ui::IconButton::new(id, icon, label, p)
          .size(ui::IconButtonSize::Small)
          .disabled(disabled)
      };
    Some(
      div()
        .id("logs-find")
        .debug_selector(|| "logs-find".into())
        .absolute()
        .top_2()
        .right_3()
        .bg(p.background)
        .border_1()
        .border_color(p.border)
        .rounded(radius::MD)
        .shadow_lg()
        .p_0p5()
        .flex()
        .items_center()
        .gap_0p5()
        .capture_action(cx.listener(|view, _: &Escape, _, cx| {
          view.find.open = false;
          cx.stop_propagation();
          cx.notify();
        }))
        .child(
          ui::text(div(), TextSize::Sm)
            .h_6()
            .w_48()
            .px_2()
            .flex()
            .items_center()
            .rounded(radius::MD)
            .border_1()
            .border_color(p.input)
            .child(ui::input(&input).appearance(false).bordered(false)),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .px_2()
            .whitespace_nowrap()
            .text_color(muted)
            .child(format!("{shown} of {total}")),
        )
        .child(
          find_button(
            "logs-find-prev",
            theme::ICON_CHEVRON_UP,
            "Previous match",
            empty,
          )
          .when(!empty, |this| {
            this.on_click(cx.listener(|view, _, _, cx| view.step_match(-1, cx)))
          }),
        )
        .child(
          find_button(
            "logs-find-next",
            theme::ICON_CHEVRON_DOWN,
            "Next match",
            empty,
          )
          .when(!empty, |this| {
            this.on_click(cx.listener(|view, _, _, cx| view.step_match(1, cx)))
          }),
        )
        .child(
          find_button("logs-find-close", theme::ICON_X, "Close find", false).on_click(cx.listener(
            |view, _, _, cx| {
              view.find.open = false;
              cx.notify();
            },
          )),
        ),
    )
  }

  /// `LogsTimeseriesChart`: stacked bars of records per second.
  fn chart(&self, p: &Palette) -> Option<impl IntoElement> {
    let data = &self.chart;
    if data.labels.is_empty() {
      return None;
    }
    let colors = [
      ui::Tone::Primary.color(p),
      ui::Tone::Warning.color(p),
      ui::Tone::Danger.color(p),
    ];
    let max = data
      .counts
      .iter()
      .map(|c| c.iter().sum::<i64>())
      .max()
      .unwrap_or(0)
      .max(1);
    let (step, top) = nice_axis(max);
    let ticks: Vec<i64> = (0..=top / step).map(|i| i * step).collect();
    let width = self
      .anchors
      .borrow()
      .get("logs-chart")
      .map_or(600., |b| f32::from(b.size.width) - 60.);
    let legend = div()
      .absolute()
      .top(zpx(2.))
      .left_0()
      .right_0()
      .flex()
      .justify_center()
      .gap_3()
      .children(SERIES.iter().zip(colors).map(|((_, label), color)| {
        ui::text(div(), TextSize::Xs)
          .flex()
          .items_center()
          .gap_1()
          .text_color(p.muted_foreground)
          .child(div().size(zpx(8.)).rounded(radius::SM).bg(color))
          .child(*label)
      }));
    let plot_height = CHART_HEIGHT - 52. - 26.;
    let grid = div()
      .absolute()
      .top(zpx(52.))
      .left(zpx(44.))
      .right(zpx(16.))
      .h(zpx(plot_height))
      .children(ticks.iter().map(|t| {
        let y = plot_height - plot_height * (*t as f32 / top as f32);
        div()
          .absolute()
          .left_0()
          .right_0()
          .top(zpx(y))
          .h_px()
          .bg(p.border.opacity(0.3))
      }))
      .child(
        div()
          .absolute()
          .inset_0()
          .flex()
          .items_end()
          .children(data.counts.iter().map(|counts| {
            div()
              .flex_1()
              .h_full()
              .flex()
              .flex_col()
              .justify_end()
              .items_center()
              .child(
                div()
                  .w(gpui::relative(0.6))
                  .max_w(zpx(24.))
                  .flex()
                  .flex_col_reverse()
                  .children(counts.iter().zip(colors).filter(|(n, _)| **n > 0).map(
                    |(n, color)| {
                      div()
                        .w_full()
                        .h(zpx(plot_height * (*n as f32 / top as f32)))
                        .bg(color)
                    },
                  )),
              )
          })),
      );
    let y_labels = div()
      .absolute()
      .top(zpx(52.))
      .left_0()
      .w(zpx(38.))
      .h(zpx(plot_height))
      .children(ticks.iter().map(|t| {
        let y = plot_height - plot_height * (*t as f32 / top as f32);
        ui::text(div(), TextSize::Xs)
          .absolute()
          .right_0()
          .top(zpx(y - 7.))
          .text_color(p.muted_foreground)
          .child(t.to_string())
      }));
    // A handful of evenly spaced times, each wide enough to read in full,
    // however many columns the bars are.
    let columns = data.labels.len();
    let label_width = 64.;
    let labelled = ((width / (label_width + 24.)).floor() as usize)
      .clamp(1, 8)
      .min(columns);
    let x_labels = div()
      .absolute()
      .bottom(zpx(4.))
      .left(zpx(44.))
      .right(zpx(16.))
      .h(zpx(16.))
      .children((0..labelled).map(|k| {
        let column = (((k as f32 + 0.5) * columns as f32 / labelled as f32 - 0.5).round() as usize)
          .min(columns - 1);
        let centre = (column as f32 + 0.5) / columns as f32;
        ui::text(div(), TextSize::Xs)
          .absolute()
          .top_0()
          .left(gpui::relative(centre))
          .ml(zpx(-label_width / 2.))
          .w(zpx(label_width))
          .flex()
          .justify_center()
          .whitespace_nowrap()
          .text_color(p.muted_foreground)
          .child(data.labels[column].clone())
      }));
    Some(
      div()
        .px_2()
        .pt_2()
        .border_b_1()
        .border_color(p.border)
        .child(
          div()
            .debug_selector(|| "logs-timeseries-chart".into())
            .relative()
            .h(zpx(CHART_HEIGHT))
            .child(self.anchor("logs-chart"))
            .child(legend)
            .child(grid)
            .child(y_labels)
            .child(x_labels),
        ),
    )
  }

  /// `LogsSqlExplorer`.
  fn explorer(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
    let has_result = self.explorer.result.is_some() && self.explorer.error.is_none();
    let bar = div()
      .flex()
      .flex_wrap()
      .items_center()
      .gap_2()
      .px_4()
      .py_2()
      .border_b_1()
      .border_color(p.border)
      .bg(p.muted.opacity(0.3))
      .child(
        div().relative().child(self.anchor("template-menu")).child(
          Button::new("template-menu", p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .gap_3()
            .icon(theme::ICON_FILE_CODE, 12.)
            .label("Templates")
            .child(ui::icon(
              theme::ICON_CHEVRON_DOWN,
              12.,
              p.foreground.opacity(0.6),
            ))
            .on_click(cx.listener(|view, _, _, cx| {
              view.open = (view.open != Some(Open::Templates)).then_some(Open::Templates);
              cx.notify();
            })),
        ),
      )
      .child(
        ui::text(div(), TextSize::Xs)
          .flex()
          .gap_1()
          .text_color(p.muted_foreground)
          .child("Read-only queries against the")
          .child(div().font_family(theme::MONO_FONT).child("logs"))
          .child("view."),
      );
    let editor = div()
      .p_3()
      .border_b_1()
      .border_color(p.border)
      .child(
        ui::text(div(), TextSize::Xs)
          .debug_selector(|| "sql-editor".into())
          .font_family(theme::MONO_FONT)
          .children(self.explorer.input.as_ref().map(|input| {
            ui::text(
              Textarea::new(input).font_family(theme::MONO_FONT),
              TextSize::Code,
            )
          })),
      )
      .child(
        div()
          .flex()
          .items_center()
          .justify_end()
          .gap_2()
          .mt_2()
          .when(has_result, |this| {
            this.child(
              Button::new("send-resultset-to-agent", p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .gap_3()
                .icon(theme::ICON_BOT, 12.)
                .label("Send results to agent")
                .on_click(cx.listener(|view, _, _, cx| {
                  if let Some(result) = &view.explorer.result {
                    view.send_to_agent(sql_result_prompt(result, &view.explorer.sql), cx);
                  }
                })),
            )
          })
          .child(
            Button::new("run-query", p)
              .size(Size::Sm)
              .gap_3()
              .disabled(self.explorer.running)
              .child(if self.explorer.running {
                ui::spinner("run-query-spin", 12., p.primary_foreground).into_any_element()
              } else {
                ui::icon(theme::ICON_PLAY, 12., p.primary_foreground).into_any_element()
              })
              .label("Run query")
              .on_click(cx.listener(|view, _, _, cx| view.run_query(cx))),
          ),
      );
    let mut results = div()
      .id("sql-results-scroll")
      .flex_1()
      .min_h_0()
      .overflow_scroll();
    if let Some(error) = &self.explorer.error {
      results = results.child(
        ui::text(div(), TextSize::Xs)
          .debug_selector(|| "sql-error".into())
          .m_3()
          .p_3()
          .flex()
          .items_start()
          .gap_2()
          .rounded(radius::MD)
          .border_1()
          .border_color(ui::Tone::Danger.border(p))
          .bg(ui::Tone::Danger.tint(p))
          .text_color(ui::Tone::Danger.text(p))
          .child(ui::icon(
            theme::ICON_TRIANGLE_ALERT,
            16.,
            ui::Tone::Danger.text(p),
          ))
          .child(div().font_family(theme::MONO_FONT).child(error.clone())),
      );
    } else if let Some(result) = &self.explorer.result {
      results = results.child(
        ui::text(div(), TextSize::Xs)
          .px_4()
          .py_2()
          .border_b_1()
          .border_color(p.border)
          .text_color(p.muted_foreground)
          .child(format!(
            "{} row{}",
            result.row_count,
            if result.row_count == 1 { "" } else { "s" }
          )),
      );
      results = if result.row_count == 0 {
        results.child(
          ui::text(div(), TextSize::Sm)
            .p_4()
            .text_color(p.muted_foreground)
            .child("Query returned no rows."),
        )
      } else {
        results.child(self.result_table(result, p))
      };
    } else {
      results = results.child(
        ui::text(div(), TextSize::Sm)
          .p_4()
          .text_color(p.muted_foreground)
          .child("Run a query to see results."),
      );
    }
    div()
      .debug_selector(|| "logs-sql-explorer".into())
      .flex_1()
      .min_h_0()
      .flex()
      .flex_col()
      .child(bar)
      .child(editor)
      .child(results)
  }

  fn result_table(&self, result: &SqlResult, p: &Palette) -> Div {
    // As wide as each column's longest cell, like an auto table.
    let widths: Vec<f32> = (0..result.columns.len())
      .map(|i| {
        let longest = result
          .rows
          .iter()
          .map(|row| {
            row
              .get(i)
              .and_then(|c| c.as_deref())
              .unwrap_or("NULL")
              .chars()
              .count()
          })
          .chain([result.columns[i].chars().count()])
          .max()
          .unwrap_or(4)
          .min(60);
        longest as f32 * 6.6 + 24.
      })
      .collect();
    let hover = p.muted.opacity(0.5);
    ui::text(div(), TextSize::Xs)
      .debug_selector(|| "sql-results".into())
      .font_family(theme::MONO_FONT)
      .flex()
      .flex_col()
      .min_w_full()
      .child(
        div()
          .flex()
          .bg(p.muted)
          .children(result.columns.iter().zip(&widths).map(|(column, w)| {
            div()
              .flex_none()
              .w(zpx(*w))
              .px_3()
              .py_2()
              .font_weight(FontWeight::MEDIUM)
              .border_b_1()
              .border_color(p.border)
              .whitespace_nowrap()
              .child(column.clone())
          })),
      )
      .children(result.rows.iter().enumerate().map(|(r, row)| {
        div()
          .id(("sql-row", r))
          .flex()
          .hover(move |s| s.bg(hover))
          .children(row.iter().zip(&widths).map(|(value, w)| {
            let cell = div()
              .flex_none()
              .w(zpx(*w))
              .px_3()
              .py_1p5()
              .border_b_1()
              .border_color(p.border)
              .whitespace_normal();
            match value {
              Some(value) => cell.child(value.clone()),
              None => cell.child(div().italic().text_color(p.muted_foreground).child("NULL")),
            }
          }))
      }))
  }

  /// The open filter's or template list's popover, below its button.
  fn popover(&self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let open = self.open?;
    let (key, width) = match open {
      Open::Levels => ("log-level-filter", 160.),
      Open::Workflows => ("check-workflow-filter", 192.),
      Open::Templates => ("template-menu", 288.),
    };
    let anchor = self.anchors.borrow().get(key).copied()?;
    let mut list = div()
      .id("logs-filter-list")
      .debug_selector(|| "logs-filter-list".into())
      .w(zpx(width))
      .p_1()
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_md()
      .occlude()
      .dismiss_with("logs-menu", cx, |view, cx| {
        let open = view.open.take().is_some();
        cx.notify();
        open
      });
    match open {
      Open::Levels => {
        list = list.child(
          check_row(
            "log-level-option-all".into(),
            "All Levels".into(),
            self.levels.is_empty(),
            p,
          )
          .on_click(cx.listener(|view, _, _, cx| {
            view.levels.clear();
            view.load(cx);
          })),
        );
        for level in LEVELS {
          let checked = self.levels.iter().any(|l| l == level);
          list = list.child(
            check_row(
              format!("log-level-option-{level}").into(),
              capitalize(level),
              checked,
              p,
            )
            .on_click(cx.listener(move |view, _, _, cx| {
              toggle(&mut view.levels, level);
              view.load(cx);
            })),
          );
        }
      }
      Open::Workflows => {
        for workflow in &self.workflows {
          let file = workflow.filename.clone();
          let checked = self.checks.contains(&file);
          list = list.child(
            check_row(
              format!("check-workflow-option-{file}").into(),
              workflow.name.clone(),
              checked,
              p,
            )
            .on_click(cx.listener(move |view, _, _, cx| {
              toggle(&mut view.checks, &file);
              view.load(cx);
            })),
          );
        }
      }
      Open::Templates => {
        let accent = p.accent;
        for (i, (label, description, sql)) in TEMPLATES.iter().enumerate() {
          list = list.child(
            div()
              .id(("sql-template", i))
              .debug_selector(move || format!("sql-template-{label}"))
              .flex()
              .flex_col()
              .gap_0p5()
              .px_2()
              .py_2()
              .rounded(radius::SM)
              .cursor_pointer()
              .hover(move |s| s.bg(accent))
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_weight(FontWeight::MEDIUM)
                  .child(*label),
              )
              .child(
                ui::text(div(), TextSize::Xs)
                  .text_color(p.muted_foreground)
                  .child(*description),
              )
              .on_click(cx.listener(move |view, _, window, cx| {
                view.explorer.sql = sql.to_string();
                if let Some(input) = &view.explorer.input {
                  input.update(cx, |s, cx| s.set_value(sql.to_string(), window, cx));
                }
                view.open = None;
                cx.notify();
              })),
          );
        }
      }
    }
    let at = point(anchor.left(), anchor.bottom() + px(4.));
    Some(deferred(anchored().position(at).child(list)).with_priority(1))
  }
}

/// ECharts' `minInterval: 1` value axis: a 1, 2 or 5 step giving about
/// five ticks, and the top it rounds `max` up to.
fn nice_axis(max: i64) -> (i64, i64) {
  let rough = (max as f64 / 5.).max(1.);
  let magnitude = 10f64.powf(rough.log10().floor());
  let step = [1., 2., 5., 10.]
    .into_iter()
    .map(|m| m * magnitude)
    .find(|s| *s >= rough)
    .unwrap_or(10. * magnitude) as i64;
  let step = step.max(1);
  (step, (max + step - 1) / step * step)
}

impl Render for LogsView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.ensure_inputs(window, cx);
    self.watch_find_key(window, cx);
    let p = theme::palette(self.dark);
    let shown = Rc::new(self.shown());
    self.matches = find_matches(&shown, &self.find.query);
    self.visible = shown.clone();
    let body = if self.source == Source::Checks && self.explorer_open {
      self.explorer(&p, cx)
    } else {
      let bar = match self.source {
        Source::Agent => self.agent_bar(&p, cx).into_any_element(),
        _ => self.filter_bar(&p, cx).into_any_element(),
      };
      let loading = match self.source {
        Source::Agent => "Loading agent chats…",
        _ => "Loading logs…",
      };
      let feed = if self.records.is_none() {
        self.pending(loading, &p)
      } else {
        self.feed(shown, &p, cx)
      };
      div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(bar)
        .children(
          (self.source == Source::Checks)
            .then(|| self.chart(&p))
            .flatten(),
        )
        .child(feed)
    };
    let popover = self.popover(&p, cx);
    div()
      .id("logs-tab")
      .debug_selector(|| "logs-tab".into())
      .size_full()
      .flex()
      .flex_col()
      .bg(p.background)
      .text_color(p.foreground)
      .on_mouse_up(
        MouseButton::Left,
        cx.listener(|view, _, _, _| view.selection.mouse_up()),
      )
      .on_mouse_up_out(
        MouseButton::Left,
        cx.listener(|view, _, _, _| view.selection.mouse_up()),
      )
      .child(self.top_bar(&p, cx))
      .child(body)
      .children(popover)
  }
}

#[cfg(test)]
mod tests {
  use super::nice_axis;

  #[test]
  fn axis_steps_are_round() {
    assert_eq!(nice_axis(1), (1, 1));
    assert_eq!(nice_axis(7), (2, 8));
    assert_eq!(nice_axis(23), (5, 25));
    assert_eq!(nice_axis(140), (50, 150));
  }
}
