//! A shell window's state, as React's Dashboard keeps it: which repository
//! and page show, the selection, which dialogs are open and what they start
//! with, the workspace view's tab, and the window's preferences.
//!
//! Plain data: the methods change it and return the [`Effect`]s the window
//! carries out (reading the backend, asking for a folder, starting an
//! agent). [`super::snapshots`] draws it.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::{
  data::Data,
  remote::{Remote, RemoteEffect},
};
use crate::{
  gpui_shell::{confirm::Question, native::ToastTone},
  local_db::Workspace,
};

/// `DEFAULT_SIDEBAR_WIDTH` and its clamp (`sidebarWidthStore`).
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 280.;
pub const MIN_SIDEBAR_WIDTH: f32 = 200.;
pub const MAX_SIDEBAR_WIDTH: f32 = 480.;
/// `zoomSettingsStore`.
pub const DEFAULT_ZOOM: u32 = 100;
pub const ZOOM_STEP: u32 = 5;
pub const MIN_ZOOM: u32 = 50;
pub const MAX_ZOOM: u32 = 200;
/// `useGpuiTerminalPane`'s `KEPT`.
const KEPT_COMMANDS: usize = 20;

/// The workspace view's tab (`GpuiWorkspaceTab`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
  #[default]
  Overview,
  Commits,
  Changes,
  Logs,
}

impl Tab {
  pub fn value(self) -> &'static str {
    match self {
      Tab::Overview => "overview",
      Tab::Commits => "commits",
      Tab::Changes => "changes",
      Tab::Logs => "logs",
    }
  }

  pub fn parse(value: &str) -> Option<Self> {
    [Tab::Overview, Tab::Commits, Tab::Changes, Tab::Logs]
      .into_iter()
      .find(|t| t.value() == value)
  }
}

/// The GitHub view's tab (`GitHubTab`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubTab {
  Issues,
  Prs,
}

/// Where in the GitHub view the window is, as its URL route said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Github {
  pub tab: GithubTab,
  /// `GitHubStateFilter`: draft, open, closed or all.
  pub filter: String,
  pub selected: Option<u64>,
  pub creating: bool,
  /// The pull request whose review shows (`/review`).
  pub review: Option<u64>,
  /// Bumped to read the selected PR again once a review was submitted.
  pub reload: u64,
}

impl Github {
  fn list(tab: GithubTab, filter: &str) -> Self {
    Self {
      tab,
      filter: filter.into(),
      selected: None,
      creating: false,
      review: None,
      reload: 0,
    }
  }
}

/// Dashboard's `viewMode`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Page {
  /// `show-workspace`, and `session`.
  #[default]
  Workspace,
  Settings {
    tab: String,
  },
  Github(Github),
  Linear,
  MergePreview {
    workspace_id: i64,
  },
}

/// The Code tab's file browser, opened on a file or folder (full paths).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileBrowser {
  pub selected_file: Option<String>,
  pub expanded_dir: Option<String>,
}

/// `ShowWorkspace`'s state that GPUI's tabs read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceView {
  pub tab: Tab,
  /// `showFileBrowserInCode`.
  pub file_browser: Option<FileBrowser>,
  pub scroll_to_commit: Option<String>,
  pub show_committed: bool,
  /// `LogSource`: "checks" or "system".
  pub logs_source: String,
  pub logs_key: u64,
  /// Bumped when the workspace's files change or the window gains focus.
  pub changes_revision: u64,
}

impl Default for WorkspaceView {
  fn default() -> Self {
    Self {
      tab: Tab::Overview,
      file_browser: None,
      scroll_to_commit: None,
      show_committed: true,
      logs_source: "checks".into(),
      logs_key: 0,
      changes_revision: 0,
    }
  }
}

/// Files dropped on another workspace, waiting for a confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeMove {
  pub files: Vec<String>,
  pub source_branch: String,
  pub destination_branch: String,
  pub destination_label: String,
}

/// The agent prompt dialog's start (`runPromptRequest`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AgentPrompt {
  pub prompt: Option<String>,
  pub workspace_id: Option<i64>,
  /// An `IssueAttachment`, as the dialog reads it.
  pub issue: Option<Value>,
}

/// Which of Dashboard's dialogs are open, and what they start with.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dialogs {
  /// `unifiedDialogDefaults` as `GpuiWorkspaceModal` sends them, and the
  /// key that starts each open fresh.
  pub workspace_dialog: Option<(u64, Value)>,
  pub workspace_dialog_key: u64,
  /// `renameTarget`: repository, workspace id and branch.
  pub rename: Option<(String, i64, String)>,
  /// The New Workspaces dialog, and the branch it stacks new workspaces on
  /// (`None`: the default branch).
  pub open_workspaces: Option<Option<String>>,
  pub deletion: bool,
  pub change_move: Option<ChangeMove>,
  pub agent_prompt: Option<AgentPrompt>,
  /// The prompt history, focused on an entry.
  pub prompt_history: Option<Option<i64>>,
  pub stash: bool,
  pub palette: bool,
  pub shortcuts: bool,
  pub workspace_picker: bool,
  pub branch_switcher: bool,
  pub file_picker: bool,
}

impl Dialogs {
  /// Whether any dialog or overlay is open, so a shortcut does not open
  /// another over it.
  pub fn any_open(&self) -> bool {
    self.workspace_dialog.is_some()
      || self.open_workspaces.is_some()
      || self.rename.is_some()
      || self.deletion
      || self.change_move.is_some()
      || self.agent_prompt.is_some()
      || self.prompt_history.is_some()
      || self.stash
      || self.palette
      || self.shortcuts
      || self.workspace_picker
      || self.branch_switcher
      || self.file_picker
  }
}

/// `TerminalSessionSummary`, as far as the sidebar reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSummary {
  /// The pane's column id; `None` from older summaries.
  pub id: Option<String>,
  /// The session's name ("AI Review" for a review's agent).
  pub name: Option<String>,
  pub agent: bool,
  pub branch: Option<String>,
  pub repo_path: Option<String>,
  pub streaming: bool,
}

/// How the window draws: the theme setting, the OS appearance, the zoom
/// and the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
  /// `theme`: system, light or dark.
  pub theme: String,
  pub system_dark: bool,
  pub zoom: u32,
  pub sidebar_width: f32,
  /// `default_agent`, which a remote repository's sessions start.
  pub default_agent: Option<String>,
}

impl Default for Prefs {
  fn default() -> Self {
    Self {
      theme: "system".into(),
      system_dark: false,
      zoom: DEFAULT_ZOOM,
      sidebar_width: DEFAULT_SIDEBAR_WIDTH,
      default_agent: None,
    }
  }
}

impl Prefs {
  /// `actualTheme() === "dark"`.
  pub fn dark(&self) -> bool {
    match self.theme.as_str() {
      "dark" => true,
      "light" => false,
      _ => self.system_dark,
    }
  }
}

/// What a folder the user picks is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderPurpose {
  /// Open it in this window.
  OpenRepo,
  /// Open it in a new window.
  OpenInNewWindow,
  /// Add it as a supporting repository.
  AddSupporting,
  /// Put it in place of the supporting repository at this path.
  LocateSupporting(String),
}

/// What the window does for a change of its state.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
  /// Read the window's data again; `true` bumps the screens' revision, as
  /// `invalidateQueries` does.
  Refresh {
    revision: bool,
  },
  /// Asks for a folder, for `purpose`.
  PickFolder(FolderPurpose),
  /// Persists app setting `key`.
  SaveSetting {
    key: String,
    value: String,
  },
  /// Starts an agent session (`handleCreateSessionFromSidebar` and
  /// `launchAgentSession`); `prompt` and `mode` seed it.
  StartAgent {
    repo_path: String,
    workspace_id: Option<i64>,
    agent: Option<String>,
    prompt: Option<String>,
    mode: Option<String>,
    name: Option<String>,
    /// Shows the session's workspace once it starts (`launchAgentSession`).
    select: bool,
  },
  /// Opens the folder at this path in this window (`openRepositoryAtPath`),
  /// first offering to initialize a repository in it if it has none.
  OpenRepoAt(String),
  /// Opens the folder at this path in a new window, first offering to
  /// initialize a repository in it if it has none.
  OpenInNewWindow(String),
  /// Initializes a Git repository in the folder at `path` (confirmed),
  /// then opens it here or in a new window.
  InitRepo {
    path: String,
    new_window: bool,
  },
  /// Reads the app settings the window follows again.
  ReadPrefs,
  /// Drops a `treq send` attachment.
  DismissSendAsset(String),
  /// Removes the supporting repository at this path (confirmed).
  RemoveSupporting(String),
  /// Opens the folder of the app's logs.
  OpenLogDir,
  /// Clears the window's repository (Factory Reset, confirmed).
  FactoryReset,
  /// Force-rebases the workspace onto `branch` (the Developer menu).
  ForceRebase {
    repo_path: String,
    workspace_id: i64,
    branch: String,
  },
  Toast {
    tone: ToastTone,
    title: String,
    description: String,
  },
  /// An AI review of the workspace finished: toast its comment count, with
  /// a button to its Changes tab.
  ReviewDone {
    repo_path: String,
    workspace_id: i64,
    branch: String,
  },
  OpenUrl(String),
  /// The remote side's ([`super::remote`]).
  Remote(Box<RemoteEffect>),
}

impl From<RemoteEffect> for Effect {
  fn from(effect: RemoteEffect) -> Self {
    Self::Remote(Box::new(effect))
  }
}

/// The toast for a folder that is not a repository and stays so.
pub fn not_a_repository() -> Effect {
  Effect::Toast {
    tone: ToastTone::Error,
    title: "Not a Git repository".into(),
    description: "Please select a folder that contains a git repository.".into(),
  }
}

/// A confirmation on screen ([`crate::gpui_shell::confirm`]), and what
/// answering it does.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirm {
  pub question: Question,
  /// What the verb does.
  pub confirmed: Effect,
  /// What Cancel does, if anything.
  pub declined: Option<Effect>,
}

impl Confirm {
  /// Offers to initialize a Git repository in the folder at `path`, then
  /// open it here or in a new window. Declined, the folder doesn't open.
  pub fn init_repo(path: &str, new_window: bool) -> Self {
    Self {
      question: Question::new(
        "Initialize a Git repository here?",
        format!("{path} isn't a Git repository yet. Treq needs one to create workspaces."),
        "Initialize",
      ),
      confirmed: Effect::InitRepo {
        path: path.to_string(),
        new_window,
      },
      declined: Some(not_a_repository()),
    }
  }

  /// Removes the supporting repository at `path` from the window.
  pub fn remove_supporting(path: &str) -> Self {
    let name = path
      .rsplit(['/', '\\'])
      .find(|s| !s.is_empty())
      .unwrap_or(path);
    Self {
      question: Question::new(
        format!("Remove {name} from Treq?"),
        format!("{path} leaves this window. Its files and workspaces aren't deleted."),
        "Remove repository",
      )
      .destructive(),
      confirmed: Effect::RemoveSupporting(path.to_string()),
      declined: None,
    }
  }

  pub fn factory_reset() -> Self {
    Self {
      question: Question::new(
        "Reset Treq to factory settings?",
        "This clears the saved repository path and returns to the start screen.",
        "Reset",
      )
      .destructive(),
      confirmed: Effect::FactoryReset,
      declined: None,
    }
  }
}

/// A shell window's state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
  /// The window's repository; empty while it shows onboarding.
  pub main_repo: String,
  /// A supporting repository put on screen in the sidebar.
  pub supporting: Option<String>,
  pub page: Page,
  /// Where closing Settings, GitHub or Linear goes (`previousViewModeRef`).
  pub back: Page,
  /// `selectedWorkspace`, by id in the repository on screen.
  pub selected: Option<i64>,
  /// The sidebar's multi-selection.
  pub multi: BTreeSet<i64>,
  /// Where a Shift range starts: an index into the sidebar's rows.
  pub last_index: Option<usize>,
  /// The active agent session's pane id.
  pub active_session: Option<i64>,
  pub view: WorkspaceView,
  pub dialogs: Dialogs,
  /// Folded workspace groups.
  pub collapsed: BTreeSet<String>,
  pub prefs: Prefs,
  pub terminals: Vec<TerminalSummary>,
  /// The terminal pane's commands (`GpuiTerminalCommand`), newest last.
  pub commands: Vec<Value>,
  pub nonce: u64,
  /// A workspace to select once it is listed.
  pub pending_select: Option<PendingSelect>,
  /// The remote dialogs, endpoint and repository.
  pub remote: Remote,
  /// The catalog skill a `treq://skill/install` link asked to install,
  /// until Settings' confirmation is answered.
  pub skill_install: Option<String>,
  /// The confirmation on screen, over everything else.
  pub confirm: Option<Confirm>,
}

/// A workspace to select once the window's data lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSelect {
  pub repo_path: String,
  pub id: i64,
  /// Also show the workspace view (`handleSelectWorkspace`), not only
  /// select it (`handleOpenSession`).
  pub page: bool,
  /// Open its Logs tab on the check logs.
  pub logs: bool,
}

impl Model {
  pub fn new(repo: Option<String>) -> Self {
    Self {
      main_repo: repo.unwrap_or_default(),
      ..Self::default()
    }
  }

  /// `repoPath`: the repository on screen.
  pub fn repo(&self) -> &str {
    match &self.supporting {
      Some(path) if !self.main_repo.is_empty() => path,
      _ => &self.main_repo,
    }
  }

  pub fn has_repo(&self) -> bool {
    !self.main_repo.is_empty()
  }

  /// The selected workspace, as the cache last listed it.
  pub fn selected_workspace<'a>(&self, data: &'a Data) -> Option<&'a Workspace> {
    let id = self.selected?;
    data.workspaces(self.repo()).iter().find(|w| w.id == id)
  }

  /// `effectiveDefaultBranch`.
  pub fn default_branch(&self, data: &Data) -> String {
    data.effective_default_branch(self.repo())
  }

  /// `setRepoPath` and `onRepositoryOpened`.
  pub fn open_repo(&mut self, path: &str) -> Vec<Effect> {
    let mut effects = Vec::new();
    // A local repository takes the screen from a remote one.
    if self.remote.repository.take().is_some() {
      self.remote.form.connected = false;
      effects.push(Effect::from(RemoteEffect::Show(None)));
    }
    self.main_repo = path.to_string();
    self.supporting = None;
    self.selected = None;
    self.active_session = None;
    self.multi.clear();
    self.page = Page::Workspace;
    self.view = WorkspaceView::default();
    effects.push(Effect::Refresh { revision: true });
    effects
  }

  /// Puts `confirm` on screen, unless one already is, so a second click
  /// can't stack another.
  pub fn ask(&mut self, confirm: Confirm) {
    if self.confirm.is_none() {
      self.confirm = Some(confirm);
    }
  }

  /// The confirmation answered: what its verb, or Cancel, does.
  pub fn answer(&mut self, yes: bool) -> Vec<Effect> {
    let Some(confirm) = self.confirm.take() else {
      return Vec::new();
    };
    match yes {
      true => vec![confirm.confirmed],
      false => confirm.declined.into_iter().collect(),
    }
  }

  /// `activateRepo`: puts a linked repository (or the main one) on screen.
  pub fn activate_repo(&mut self, path: &str, data: &Data) {
    let linked = data.supporting().iter().any(|r| r.exists && r.path == path);
    let next = linked.then(|| path.to_string());
    if next == self.supporting {
      return;
    }
    self.supporting = next;
    self.multi.clear();
  }

  /// Shows workspace `id` of the repository on screen (`setSelectedWorkspace`),
  /// starting its view over on the Code tab.
  pub(super) fn set_selected(&mut self, id: Option<i64>) {
    if self.selected != id {
      self.view.tab = Tab::Overview;
      self.view.scroll_to_commit = None;
    }
    self.selected = id;
  }

  /// `handleSelectWorkspace`.
  pub fn select_workspace(&mut self, workspace: Option<&Workspace>, data: &Data) {
    if let Some(workspace) = workspace {
      self.activate_repo(&workspace.repo_path, data);
    }
    self.set_selected(workspace.map(|w| w.id));
    self.page = Page::Workspace;
  }

  /// `handleReturnToDashboard`.
  pub fn return_home(&mut self) {
    self.set_selected(None);
    self.active_session = None;
  }

  /// `handleSelectRepoHome`.
  pub fn select_repo_home(&mut self, path: &str, data: &Data) {
    self.activate_repo(path, data);
    self.set_selected(None);
    self.page = Page::Workspace;
  }

  pub(super) fn leave_for(&mut self, page: Page) {
    let same = std::mem::discriminant(&self.page) == std::mem::discriminant(&page);
    if !same {
      self.back = std::mem::replace(&mut self.page, page);
    } else {
      self.page = page;
    }
  }

  /// `openSettings`: on its Repository tab, unless it already shows.
  pub fn open_settings(&mut self) {
    if !matches!(self.page, Page::Settings { .. }) {
      self.leave_for(Page::Settings {
        tab: "repository".into(),
      });
    }
  }

  /// A `treq://skill/install` link: Settings' Skills tab confirms it.
  pub fn open_skill_install(&mut self, skill_id: String) {
    self.open_settings();
    if let Page::Settings { tab } = &mut self.page {
      *tab = "skills".into();
    }
    self.skill_install = Some(skill_id);
  }

  /// `closeSettings`.
  pub fn close_settings(&mut self) {
    self.page = std::mem::take(&mut self.back);
  }

  /// `openGitHub`: the open issues.
  pub fn open_github(&mut self) {
    self.leave_for(Page::Github(Github::list(GithubTab::Issues, "open")));
  }

  /// `openGitHubPr`: the pull request, under the filter that lists it.
  pub fn open_github_pr(&mut self, number: u64, state: &str) {
    let filter = match state.to_uppercase().as_str() {
      "OPEN" => "open",
      "CLOSED" => "closed",
      _ => "all",
    };
    let mut page = Github::list(GithubTab::Prs, filter);
    page.selected = Some(number);
    self.leave_for(Page::Github(page));
  }

  /// `openLinear`.
  pub fn open_linear(&mut self) {
    self.leave_for(Page::Linear);
  }

  /// `handleOpenMergePreview`.
  pub fn open_merge_preview(&mut self) {
    if let Some(workspace_id) = self.selected {
      self.page = Page::MergePreview { workspace_id };
    }
  }

  /// `handleWorkspaceMultiSelect` and the sidebar's click handler, for row
  /// `id` at `index` of the sidebar's rows.
  pub fn click_workspace(
    &mut self,
    rows: &[Workspace],
    id: i64,
    shift: bool,
    toggle: bool,
    data: &Data,
  ) {
    let Some(index) = rows.iter().position(|w| w.id == id) else {
      return;
    };
    if shift {
      if let Some(from) = self.last_index.filter(|&i| i < rows.len()) {
        let (start, end) = (from.min(index), from.max(index));
        self.multi = rows[start..=end].iter().map(|w| w.id).collect();
        return;
      }
    }
    self.last_index = Some(index);
    if toggle {
      // The open workspace's row is highlighted too, so the first Ctrl+click
      // adds to it rather than starting a selection without it.
      let shown = !matches!(
        self.page,
        Page::Github(_) | Page::Linear | Page::Settings { .. }
      );
      if self.multi.is_empty() && shown {
        if let Some(open) = self
          .selected
          .filter(|s| *s != id && rows.iter().any(|w| w.id == *s))
        {
          self.multi.insert(open);
        }
      }
      if !self.multi.remove(&id) {
        self.multi.insert(id);
      }
      return;
    }
    self.multi.clear();
    let workspace = rows[index].clone();
    self.select_workspace(Some(&workspace), data);
  }

  /// A click on the home row or the list: drops the multi-selection.
  pub fn clear_multi(&mut self) {
    self.multi.clear();
    self.last_index = None;
  }

  /// `leaveRemovedWorkspaces`: off a removed workspace, onto its stack
  /// parent when it has one.
  pub fn leave_removed(&mut self, repo: &str, ids: &[i64], data: &Data) {
    self.multi.retain(|id| !ids.contains(id));
    if repo != self.repo() {
      return;
    }
    let Some(selected) = self.selected.filter(|id| ids.contains(id)) else {
      return;
    };
    let workspaces = data.workspaces(repo);
    let target = workspaces
      .iter()
      .find(|w| w.id == selected)
      .and_then(|w| w.target_branch.clone());
    let parent = workspaces
      .iter()
      .find(|w| Some(&w.branch_name) == target.as_ref() && !ids.contains(&w.id))
      .map(|w| w.id);
    match parent {
      Some(parent) => {
        self.set_selected(Some(parent));
        self.active_session = None;
      }
      None => self.return_home(),
    }
  }

  /// A workspace removed outside this view (the CLI, another window): the
  /// view leaves it once a fresh list no longer has it.
  pub fn workspaces_listed(&mut self, repo: &str, before: &[i64], now: &[Workspace]) {
    if repo == self.repo() {
      self.multi.retain(|id| now.iter().any(|w| w.id == *id));
    }
    let Some(selected) = self.selected else {
      return;
    };
    if repo == self.repo() && before.contains(&selected) && !now.iter().any(|w| w.id == selected) {
      self.return_home();
    }
  }

  /// `setUnifiedDialogDefaults`: opens the workspace dialog afresh.
  pub fn open_workspace_dialog(&mut self, defaults: Value) {
    self.dialogs.workspace_dialog_key += 1;
    self.dialogs.workspace_dialog = Some((self.dialogs.workspace_dialog_key, defaults));
  }

  /// Opens the New Workspaces dialog, stacking on `stack_on` (the default
  /// branch when `None`).
  pub fn new_workspaces(&mut self, stack_on: Option<String>) {
    if !self.repo().is_empty() {
      self.dialogs.open_workspaces = Some(stack_on);
    }
  }

  /// `handleCreateStackedWorkspace`: on the selected workspace, or the home
  /// branch.
  pub fn stack(&mut self, data: &Data) -> Vec<Effect> {
    match self.selected_workspace(data).cloned() {
      Some(workspace) => self.stack_on(&workspace),
      None => self.stack_home(data),
    }
    Vec::new()
  }

  /// `openHomeStackDialog`: on the home branch, or the default branch until
  /// it is known.
  pub fn stack_home(&mut self, data: &Data) {
    self.new_workspaces(data.home_stack_branch(self.repo()));
  }

  /// `handleAddAfter`.
  pub fn stack_on(&mut self, workspace: &Workspace) {
    self.new_workspaces(Some(workspace.branch_name.clone()));
  }

  /// `handleStashApplyToNewWorkspace`.
  pub fn apply_stash_to_new(&mut self, entry: &Value, data: &Data) {
    self.dialogs.stash = false;
    let source = entry["workspace_id"]
      .as_i64()
      .and_then(|id| data.workspaces(self.repo()).iter().find(|w| w.id == id))
      .cloned();
    let commit = entry["short_commit_id"].as_str().unwrap_or_default();
    let files = entry["files_changed"].as_array().map_or(0, Vec::len);
    let mut defaults = json!({
      "source": match &source {
        Some(workspace) => workspace_source(workspace),
        None => json!({"kind": "home"}),
      },
      "applyStashId": entry["id"],
      "applyStashCommit": {
        "hash": commit,
        "message": format!("Stash from {}", entry["workspace_label"].as_str().unwrap_or_default()),
        "timestamp": entry["created_at"],
      },
      "preSelectedCommits": [commit],
      "activeTab": "commits",
      "description": format!(
        "Apply stash {commit} ({files} files, +{}/-{})",
        entry["additions"], entry["deletions"]
      ),
    });
    if let Some(workspace) = &source {
      defaults["targetBranch"] = json!(workspace.branch_name);
    }
    self.open_workspace_dialog(defaults);
  }

  /// A command for the terminal pane (`terminalPane()?.…`).
  pub fn terminal(&mut self, kind: &str, extra: Value) {
    self.nonce += 1;
    let mut command = json!({"nonce": self.nonce, "kind": kind});
    if let (Some(command), Value::Object(extra)) = (command.as_object_mut(), extra) {
      command.extend(extra);
    }
    self.commands.push(command);
    let excess = self.commands.len().saturating_sub(KEPT_COMMANDS);
    self.commands.drain(..excess);
  }

  /// `handleStartShellFromSidebar` and `handleStartHomeShellFromSidebar`.
  pub fn open_shell(&mut self, workspace: Option<&Workspace>) {
    match workspace {
      Some(workspace) => {
        self.set_selected(Some(workspace.id));
        let dir = full_path(workspace);
        self.terminal("newShell", json!({"dir": dir}));
      }
      None => {
        self.set_selected(None);
        let dir = self.repo().to_string();
        self.terminal("newShell", json!({"dir": dir}));
      }
    }
  }

  /// `handleCreateSessionFromSidebar`: a new agent in the workspace.
  pub fn start_agent(&mut self, workspace_id: Option<i64>, data: &Data) -> Effect {
    let workspace = workspace_id.and_then(|id| self.workspace_in(self.repo(), id, data));
    self.set_selected(workspace.map(|w| w.id));
    Effect::StartAgent {
      repo_path: self.repo().to_string(),
      workspace_id,
      agent: None,
      prompt: None,
      mode: None,
      name: None,
      select: false,
    }
  }

  /// Workspace `id` of repository `repo`, as last listed.
  pub fn workspace_in(&self, repo: &str, id: i64, data: &Data) -> Option<Workspace> {
    data.workspaces(repo).iter().find(|w| w.id == id).cloned()
  }

  /// Selects workspace `id` of `repo` once a read lists it (a workspace
  /// just made, or one GitHub opened); now if it is listed already.
  pub fn select_when_listed(&mut self, pending: PendingSelect, data: &Data) -> Vec<Effect> {
    self.pending_select = Some(pending);
    if self.resolve_pending(data) {
      Vec::new()
    } else {
      vec![Effect::Refresh { revision: true }]
    }
  }

  /// Selects the pending workspace if `data` lists it.
  pub fn resolve_pending(&mut self, data: &Data) -> bool {
    let Some(pending) = self.pending_select.clone() else {
      return false;
    };
    let Some(workspace) = self.workspace_in(&pending.repo_path, pending.id, data) else {
      return false;
    };
    self.pending_select = None;
    if pending.page {
      self.select_workspace(Some(&workspace), data);
    } else {
      self.activate_repo(&workspace.repo_path, data);
      self.set_selected(Some(workspace.id));
    }
    if pending.logs {
      self.open_logs("checks");
    }
    true
  }

  /// `handleViewFullPrompt`.
  pub fn view_full_prompt(&mut self, id: i64) {
    self.dialogs.prompt_history = Some(Some(id));
  }

  /// `handleRunPrompt`: the past prompt, in the prompt dialog.
  pub fn run_prompt(&mut self, prompt: String, workspace_id: Option<i64>) {
    self.dialogs.prompt_history = None;
    self.dialogs.agent_prompt = Some(AgentPrompt {
      prompt: Some(prompt),
      workspace_id,
      issue: None,
    });
  }

  /// `handleStartPromptFromIssue`.
  pub fn start_prompt_from_issue(&mut self, issue: Value) {
    self.dialogs.agent_prompt = Some(AgentPrompt {
      prompt: None,
      workspace_id: None,
      issue: Some(issue),
    });
  }

  /// Opens the Code tab's file browser on a path from Cmd+P (relative to
  /// the repository) or a full one (`sessionSelectedFile`).
  pub fn open_file(&mut self, path: &str) {
    let selected = if path.starts_with('/') {
      path.to_string()
    } else {
      format!("{}/{path}", self.repo().trim_end_matches('/'))
    };
    let dir = selected
      .rsplit_once('/')
      .map(|(dir, _)| dir.to_string())
      .unwrap_or_default();
    self.view.file_browser = Some(FileBrowser {
      selected_file: Some(selected),
      expanded_dir: Some(dir),
    });
  }

  /// `handleOverviewEntryClick`, for an entry of `working_directory`.
  pub fn open_entry(&mut self, working_directory: &str, name: &str, directory: bool) {
    let full = format!("{working_directory}/{name}");
    self.view.file_browser = Some(if directory {
      FileBrowser {
        selected_file: None,
        expanded_dir: Some(full),
      }
    } else {
      FileBrowser {
        selected_file: Some(full),
        expanded_dir: None,
      }
    });
  }

  /// `openLogs`: the Logs tab, started over on `source`.
  pub fn open_logs(&mut self, source: &str) {
    self.view.logs_source = source.into();
    self.view.logs_key += 1;
    self.view.tab = Tab::Logs;
  }

  /// `Cmd`+`=`/`-`: the zoom, a step at a time.
  pub fn zoom(&mut self, by: i32) -> Effect {
    let next = (self.prefs.zoom as i32 + by).clamp(MIN_ZOOM as i32, MAX_ZOOM as i32) as u32;
    self.prefs.zoom = next;
    Effect::SaveSetting {
      key: "ui_zoom".into(),
      value: next.to_string(),
    }
  }

  /// The resize handle let go at `width`, or reset with `None`.
  pub fn resize_sidebar(&mut self, width: Option<f32>) -> Effect {
    let width = width
      .filter(|w| w.is_finite())
      .unwrap_or(DEFAULT_SIDEBAR_WIDTH)
      .round()
      .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    self.prefs.sidebar_width = width;
    Effect::SaveSetting {
      key: "workspace_sidebar_width".into(),
      value: (width as i64).to_string(),
    }
  }

  /// Folds or unfolds a repository's workspace group, kept per main
  /// repository as `writeCollapsedRepos` does.
  pub fn toggle_repo(&mut self, path: &str) -> Effect {
    if !self.collapsed.remove(path) {
      self.collapsed.insert(path.to_string());
    }
    Effect::SaveSetting {
      key: collapsed_key(&self.main_repo),
      value: serde_json::to_string(&self.collapsed).unwrap_or_default(),
    }
  }

  /// Applies the app settings the window reads (`applySettingsRecord`).
  pub fn apply_settings(&mut self, get: impl Fn(&str) -> Option<String>) {
    if let Some(theme) = get("theme").filter(|t| ["system", "light", "dark"].contains(&t.as_str()))
    {
      self.prefs.theme = theme;
    }
    if let Some(zoom) = get("ui_zoom").and_then(|z| z.parse::<u32>().ok()) {
      if (MIN_ZOOM..=MAX_ZOOM).contains(&zoom) {
        self.prefs.zoom = zoom;
      }
    }
    if let Some(width) = get("workspace_sidebar_width")
      .and_then(|w| w.parse::<f32>().ok())
      .filter(|w| w.is_finite())
    {
      self.prefs.sidebar_width = width.round().clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    }
    self.prefs.default_agent = get("default_agent").filter(|a| !a.is_empty());
    self.collapsed = get(&collapsed_key(&self.main_repo))
      .and_then(|raw| serde_json::from_str(&raw).ok())
      .unwrap_or_default();
  }

  /// Factory Reset, once confirmed: back to onboarding, with no remote
  /// repository either.
  pub fn reset(&mut self) {
    let cutoffs = std::mem::take(&mut self.remote.cutoffs);
    *self = Self {
      prefs: self.prefs.clone(),
      ..Self::default()
    };
    self.remote.cutoffs = cutoffs;
  }
}

/// A workspace's checkout (`getFullWorkspacePath`).
pub fn full_path(workspace: &Workspace) -> String {
  crate::core::context_menu::full_workspace_path(workspace)
}

/// The setting the folded groups of a main repository are kept under.
pub fn collapsed_key(main_repo: &str) -> String {
  format!("sidebar_collapsed_repos:{main_repo}")
}

/// `dialogSource` for a workspace.
pub fn workspace_source(workspace: &Workspace) -> Value {
  json!({
    "kind": "workspace",
    "id": workspace.id,
    "branchName": workspace.branch_name,
    "targetBranch": workspace.target_branch,
  })
}

/// The finished-review toast's title.
pub fn review_done_title(comments: usize) -> String {
  match comments {
    0 => "Review done · no comments".into(),
    1 => "Review done · 1 comment".into(),
    n => format!("Review done · {n} comments"),
  }
}
