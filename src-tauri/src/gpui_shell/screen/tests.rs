use std::{
  cell::RefCell,
  rc::Rc,
  sync::{Arc, Mutex},
};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;

/// A screen in a test window, and the actions it sent.
struct Harness {
  cx: VisualTestContext,
  screen: Entity<Screen>,
  sent: Rc<RefCell<Vec<ScreenAction>>>,
}

/// The backend, as the screens see it, recording what they asked of it.
#[derive(Default)]
struct FakeHost {
  remote_ssh: bool,
  /// What `check_prerequisites` reports.
  installed: Vec<(&'static str, bool)>,
  /// App and repository settings, by key.
  settings: Mutex<settings_data::Values>,
  /// What `.treq/config.yaml` sets.
  config: crate::repo_config::RepoConfig,
  setup_script: String,
  /// The skill registry, as `skill_catalog` returns it.
  skills: Vec<crate::core::skills::SkillCatalogSkillView>,
  /// What the workspace header reads.
  loaded: workspace_data::Loaded,
  pr: workspace_data::PrCache,
  /// What the merge preview reads, and whether merging fails.
  merge: merge::Preview,
  merge_error: Option<String>,
  calls: Mutex<Vec<String>>,
}

impl FakeHost {
  fn calls(&self) -> Vec<String> {
    self.calls.lock().unwrap().clone()
  }

  fn record(&self, call: String) {
    self.calls.lock().unwrap().push(call);
  }
}

impl FakeHost {
  fn with_settings(pairs: &[(&str, &str)]) -> Self {
    Self {
      settings: Mutex::new(
        pairs
          .iter()
          .map(|(k, v)| (k.to_string(), Some(v.to_string())))
          .collect(),
      ),
      ..Self::default()
    }
  }

  fn setting(&self, key: &str) -> Option<String> {
    self.settings.lock().unwrap().get(key).cloned().flatten()
  }

  /// The calls that change something, without the reads.
  fn writes(&self) -> Vec<String> {
    self
      .calls()
      .into_iter()
      .filter(|c| !c.starts_with("check") && !c.starts_with("read"))
      .collect()
  }
}

impl onboarding::OnboardingHost for FakeHost {
  fn remote_ssh_enabled(&self) -> bool {
    self.remote_ssh
  }

  fn pick_repo(&self) {
    self.record("pick_repo".into());
  }
}

impl settings_data::SettingsHost for FakeHost {
  fn settings(&self, keys: &[String]) -> Result<settings_data::Values, String> {
    self.record("read settings".into());
    Ok(keys.iter().map(|k| (k.clone(), self.setting(k))).collect())
  }

  fn set_settings(&self, values: &[(String, String)]) -> Result<(), String> {
    let pairs: Vec<_> = values.iter().map(|(k, v)| format!("{k}={v}")).collect();
    self.record(format!("set {}", pairs.join(" ")));
    let mut settings = self.settings.lock().unwrap();
    for (k, v) in values {
      settings.insert(k.clone(), Some(v.clone()));
    }
    Ok(())
  }

  fn setup_script(&self) -> Result<crate::core::app_setup_script::AppSetupScriptStatus, String> {
    self.record("read setup".into());
    Ok(crate::core::app_setup_script::AppSetupScriptStatus {
      script: self.setup_script.clone(),
      always_run: false,
      last_run_at: None,
      last_status: None,
      running: false,
    })
  }

  fn save_setup_script(&self, script: &str, always_run: bool) -> Result<(), String> {
    self.record(format!("save setup {script:?} {always_run}"));
    Ok(())
  }

  fn run_setup_script(&self) {
    self.record("run setup".into());
  }

  fn repo_config(&self, repo_path: &str) -> Result<crate::repo_config::RepoConfig, String> {
    self.record(format!("read config {repo_path}"));
    Ok(self.config.clone())
  }

  fn repo_settings(
    &self,
    repo_path: &str,
    keys: &[String],
  ) -> Result<settings_data::Values, String> {
    self.record(format!("read repo {repo_path}"));
    Ok(
      keys
        .iter()
        .map(|k| (k.clone(), self.setting(&format!("{repo_path}:{k}"))))
        .collect(),
    )
  }

  fn set_repo_settings(&self, repo_path: &str, values: &[(String, String)]) -> Result<(), String> {
    let pairs: Vec<_> = values.iter().map(|(k, v)| format!("{k}={v}")).collect();
    self.record(format!("set repo {repo_path} {}", pairs.join(" ")));
    Ok(())
  }

  fn skill_catalog(
    &self,
    _: Option<&str>,
  ) -> Result<Vec<crate::core::skills::SkillCatalogSkillView>, String> {
    self.record("read skills".into());
    Ok(self.skills.clone())
  }

  fn installed_skills(
    &self,
    _: Option<&str>,
  ) -> Result<Vec<crate::core::skills::InstalledSkill>, String> {
    Ok(
      self
        .skills
        .iter()
        .filter_map(|s| s.installed.clone())
        .collect(),
    )
  }

  fn install_skill(
    &self,
    skill_id: &str,
    scope: crate::core::skills::SkillInstallScope,
    repo_path: Option<&str>,
  ) -> Result<(), String> {
    self.record(format!("install {skill_id} {scope:?} {repo_path:?}"));
    Err("checksum mismatch".into())
  }

  fn uninstall_skill(&self, skill_id: &str, _: Option<&str>) -> Result<(), String> {
    self.record(format!("uninstall {skill_id}"));
    Ok(())
  }

  fn set_skill_scope(
    &self,
    skill_id: &str,
    scope: crate::core::skills::SkillInstallScope,
    _: Option<&str>,
  ) -> Result<(), String> {
    self.record(format!("scope {skill_id} {scope:?}"));
    Ok(())
  }

  fn notify(&self, event: crate::gpui_shell::native::NativeEvent) {
    let json = serde_json::to_value(&event).expect("serialize");
    let mut line = format!("notify {}", json["kind"].as_str().unwrap_or_default());
    if let Some(title) = json["title"].as_str() {
      line += &format!(" {title}");
    }
    self.record(line);
  }
}

impl workspace_data::WorkspaceHost for FakeHost {
  fn load(&self, route: &workspace_data::Route) -> workspace_data::Loaded {
    self.record(format!("read workspace {}", route.revision));
    self.loaded.clone()
  }

  fn pr_cache(&self, _: &str, branch: &str) -> workspace_data::PrCache {
    self.record(format!("read pr {branch}"));
    self.pr.clone()
  }

  fn push(&self, _: &str, workspace_id: Option<i64>) -> workspace_data::Outcome {
    self.record(format!("push {workspace_id:?}"));
    workspace_data::Outcome::ok("Pushed to remote", "Changes pushed successfully".into())
  }

  fn sync(&self, _: &str, workspace_id: Option<i64>) -> workspace_data::Outcome {
    self.record(format!("sync {workspace_id:?}"));
    workspace_data::Outcome::failed("Sync failed", "offline".into())
  }

  fn rebase_home(&self, _: &str, branch: &str) -> workspace_data::Outcome {
    self.record(format!("rebase {branch}"));
    workspace_data::Outcome::ok("Rebase complete", "Branch rebased onto target".into())
  }

  fn notify(&self, event: crate::gpui_shell::native::NativeEvent) {
    settings_data::SettingsHost::notify(self, event);
  }
}

impl merge::MergeHost for FakeHost {
  fn merge_preview(&self, _: &str, workspace_id: i64) -> merge::Preview {
    self.record(format!("read merge {workspace_id}"));
    self.merge.clone()
  }

  fn merge(
    &self,
    _: &str,
    workspace_id: i64,
    message: &str,
    strategy: merge::Strategy,
  ) -> Result<(), String> {
    self.record(format!("merge {workspace_id} {strategy:?} {message:?}"));
    self.merge_error.clone().map_or(Ok(()), Err)
  }
}

impl checklist::ToolsHost for FakeHost {
  fn check_prerequisites(&self) -> Vec<crate::core::PrerequisiteStatus> {
    self.record("check".into());
    self
      .installed
      .iter()
      .map(|(binary, installed)| crate::core::PrerequisiteStatus {
        binary: binary.to_string(),
        installed: *installed,
      })
      .collect()
  }

  fn open_url(&self, url: &str) {
    self.record(format!("open {url}"));
  }
}

impl Harness {
  fn new(cx: &mut TestAppContext, snapshot: ScreenSnapshot) -> Self {
    Self::with_host(cx, snapshot, Arc::new(FakeHost::default()))
  }

  fn with_host(cx: &mut TestAppContext, snapshot: ScreenSnapshot, host: Arc<FakeHost>) -> Self {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink: ActionSink = {
      let sent = sent.clone();
      Rc::new(move |action, _| sent.borrow_mut().push(action))
    };
    cx.update(crate::gpui_shell::theme::init);
    let popups: crate::gpui_shell::popup::PopupSink = Rc::new(|_, _| {});
    let (screen, cx) = cx.add_window_view(|_, _| Screen::new(snapshot, sink, popups, None, host));
    let cx = cx.clone();
    Self { cx, screen, sent }
  }

  fn drawn(&mut self, selector: &'static str) -> bool {
    self.cx.run_until_parked();
    self.cx.debug_bounds(selector).is_some()
  }

  fn click(&mut self, selector: &'static str) {
    self.cx.run_until_parked();
    let bounds = self
      .cx
      .debug_bounds(selector)
      .unwrap_or_else(|| panic!("{selector} is not drawn"));
    self.cx.simulate_click(bounds.center(), Modifiers::none());
  }

  fn sent(&self) -> Vec<ScreenAction> {
    self.sent.borrow().clone()
  }

  fn update(&mut self, f: impl FnOnce(&mut ScreenSnapshot)) {
    let mut snapshot = self.screen.read_with(&self.cx, |s, _| s.snapshot().clone());
    f(&mut snapshot);
    self
      .screen
      .update(&mut self.cx, |s, cx| s.set_snapshot(snapshot, cx));
  }
}

fn onboarding() -> ScreenSnapshot {
  serde_json::from_str(r#"{"kind": "onboarding", "dark": false}"#).expect("parse")
}

#[test]
fn builds_the_checklist_from_the_check() {
  let list = checklist::from_statuses("Before you start", None, true);
  assert!(list.validating);
  assert_eq!(list.groups.len(), 3);
  // Nothing is reported missing before the first check returns.
  assert!(!list.groups[1].error);
  assert!(list
    .groups
    .iter()
    .flat_map(|g| &g.tools)
    .all(|t| t.installed.is_none()));

  let statuses = [("git", false), ("claude", false), ("gh", true)].map(|(binary, installed)| {
    crate::core::PrerequisiteStatus {
      binary: binary.into(),
      installed,
    }
  });
  let list = checklist::from_statuses("Before you start", Some(&statuses), false);
  let [required, agents, optional] = &list.groups[..] else {
    panic!("three groups");
  };
  assert_eq!(required.label, "Required");
  assert!(required.tools[0].required);
  assert_eq!(required.tools[0].installed, Some(false));
  assert!(agents.error, "no agent CLI is installed");
  assert_eq!(agents.tools.len(), 4);
  assert_eq!(optional.tools[0].installed, Some(true));
  assert_eq!(
    optional.tools[0].note.as_deref(),
    Some("Needed for pull requests and issues")
  );
  // The binaries are the ones the backend checks, in its order.
  let binaries: Vec<_> = list
    .groups
    .iter()
    .flat_map(|g| &g.tools)
    .map(|t| t.binary.as_str())
    .collect();
  assert_eq!(binaries, crate::core::PREREQUISITE_BINARIES);
}

#[test]
fn serializes_actions_as_react_reads_them() {
  let json = serde_json::to_value(ScreenAction::OpenUrl {
    url: "https://cli.github.com/".into(),
  })
  .expect("serialize");
  assert_eq!(
    json,
    serde_json::json!({"kind": "openUrl", "url": "https://cli.github.com/"})
  );
  assert_eq!(
    serde_json::to_value(ScreenAction::RecheckPrerequisites).expect("serialize"),
    serde_json::json!({"kind": "recheckPrerequisites"})
  );
}

#[gpui::test]
fn onboarding_runs_its_own_actions(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    installed: vec![("git", false), ("gh", true)],
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  // The check ran as the screen opened.
  assert!(h.drawn("install-git"));
  assert!(!h.drawn("install-gh"));
  h.click("onboarding-open-repo");
  h.click("prerequisites-recheck");
  h.click("install-git");
  // Only the remote dialog is still React's.
  h.click("onboarding-open-remote");
  assert_eq!(
    host.calls(),
    [
      "check",
      "pick_repo",
      "check",
      "open https://git-scm.com/downloads"
    ]
  );
  assert_eq!(h.sent(), vec![ScreenAction::OpenRemote]);
}

#[gpui::test]
fn onboarding_labels_the_remote_button_by_preview(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    remote_ssh: true,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  assert!(h.drawn("onboarding-open-remote"));
  let label = h.screen.read_with(&h.cx, |screen, cx| {
    screen
      .onboarding
      .as_ref()
      .map(|view| view.read(cx).remote_label.clone())
  });
  assert_eq!(label.as_deref(), Some("Open via SSH"));
}

fn settings_route(tab: &str) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "settings",
    "dark": false,
    "tab": tab,
    "repoPath": "/repo",
    "currentBranch": "main",
  }))
  .expect("parse")
}

fn settings() -> ScreenSnapshot {
  settings_route("repository")
}

fn field(field: &str, value: serde_json::Value) -> ScreenAction {
  ScreenAction::Field {
    field: field.into(),
    value,
  }
}

/// The settings screen as last drawn.
fn drawn_settings(h: &mut Harness) -> settings::Settings {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |screen, cx| {
    screen
      .settings
      .as_ref()
      .expect("settings drawn")
      .read(cx)
      .screen()
  })
}

#[gpui::test]
fn settings_saves_the_repository_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[
    ("/repo:ignore_generated_treq_paths", "true"),
    ("/repo:auto_review_trigger", "on-push"),
  ]));
  let mut h = Harness::with_host(cx, settings(), host.clone());
  let screen = drawn_settings(&mut h);
  let repo = screen.repository.expect("repository tab");
  assert!(!repo.loading);
  assert_eq!(repo.current_branch.as_deref(), Some("main"));
  assert_eq!(repo.fields.branch_name_pattern, "treq/{name}");
  assert!(repo.fields.ignore_generated_agent_files);
  // The trigger's old name reads as the new one.
  assert_eq!(repo.fields.auto_review_trigger, "on-pull");
  assert_eq!(repo.model_options[0].label, "Use Application Default");

  h.click("settings-tab-application");
  h.click("settings-auto-push");
  h.click("settings-ignore-generated");
  h.click("settings-save");
  h.click("settings-close");
  // React still owns the tab and closing the page.
  assert_eq!(
    h.sent(),
    vec![
      field("settings.tab", "application".into()),
      press("settings.close"),
    ]
  );
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "set repo /repo branch_name_pattern=treq/{name} included_copy_files= default_model= default_agent= auto_push=true ignore_generated_treq_paths=false review_prompt= review_agent= auto_review_trigger=on-pull",
      "notify repoSettingsChanged",
      "notify toast Settings saved",
    ]
  );
}

#[gpui::test]
fn settings_takes_typed_values(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  // Controls report edits from inside the screen's update.
  h.click("settings-branch-pattern");
  h.cx.simulate_input("x");
  let fields = drawn_settings(&mut h).repository.expect("drawn").fields;
  assert!(fields.branch_name_pattern.contains('x'), "{fields:?}");
}

#[gpui::test]
fn settings_disables_what_the_yaml_file_sets(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    config: crate::repo_config::RepoConfig {
      branch_name_pattern: Some("team/{name}".into()),
      review_agent: Some("codex".into()),
      ..Default::default()
    },
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, settings(), host.clone());
  let repo = drawn_settings(&mut h).repository.expect("repository tab");
  assert_eq!(repo.managed, ["branchNamePattern", "reviewAgent"]);
  assert_eq!(
    repo.yaml_card,
    Some(settings::YamlCard {
      error: None,
      loading: false
    })
  );
  h.click("settings-yaml-reload");
  h.cx.run_until_parked();
  let reads = host
    .calls()
    .iter()
    .filter(|c| *c == "read config /repo")
    .count();
  assert_eq!(reads, 2);
}

#[gpui::test]
fn settings_saves_the_application_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[
    ("terminal_font_size", "13"),
    ("ui_zoom", "120"),
  ]));
  let mut h = Harness::with_host(cx, settings_route("application"), host.clone());
  let app = drawn_settings(&mut h).application.expect("application tab");
  // A size older builds allowed shows as the nearest preset.
  assert_eq!(app.font_size, "12");
  assert_eq!(app.zoom, 120.);
  assert!(app.notify_agent_finished);
  assert_eq!(app.conflict_marker_style, "git");
  assert_eq!(app.checklist.title, "Command-line tools");

  let view = h
    .screen
    .read_with(&h.cx, |s, _| s.settings.clone().expect("drawn"));
  let send = |h: &mut Harness, action: ScreenAction| {
    view.update(&mut h.cx, |view, cx| view.handle(action, cx));
  };
  send(&mut h, field("app.theme", "dark".into()));
  send(&mut h, field("app.fontSize", "16".into()));
  send(&mut h, field("app.defaultAgent", "codex".into()));
  h.click("settings-save");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "set theme=dark",
      "notify settingsChanged",
      "set default_model= default_agent=codex conflict_marker_style=git notify_agent_finished=true terminal_font_size=16 ui_zoom=120",
      "notify settingsChanged",
      "notify toast Settings Saved",
    ]
  );
}

#[gpui::test]
fn settings_runs_only_the_saved_setup_script(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    setup_script: "brew install jj".into(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, settings_route("application"), host.clone());
  let setup = drawn_settings(&mut h)
    .application
    .expect("drawn")
    .setup_script;
  assert!(setup.can_run);
  assert_eq!(setup.status, "Never run");
  // The Run row is below the fold of the test window.
  let view = h
    .screen
    .read_with(&h.cx, |s, _| s.settings.clone().expect("drawn"));
  view.update(&mut h.cx, |view, cx| {
    view.handle(press("app.setupScript.run"), cx)
  });
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["run setup"]);

  view.update(&mut h.cx, |view, cx| {
    view.handle(field("app.setupScript", "brew install gh".into()), cx)
  });
  let setup = drawn_settings(&mut h)
    .application
    .expect("drawn")
    .setup_script;
  assert!(!setup.can_run);
  assert_eq!(setup.status, "Save settings to run the edited script.");
}

#[gpui::test]
fn settings_switches_previews_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "feature_preview.skillsInstallation",
    "false",
  )]));
  let mut h = Harness::with_host(cx, settings_route("preview"), host.clone());
  let screen = drawn_settings(&mut h);
  assert!(!screen.tabs.contains(&settings::Tab::Skills));
  h.click("settings-preview-skillsInstallation");
  h.click("settings-preview-docs-logs");
  h.cx.run_until_parked();
  assert!(drawn_settings(&mut h).tabs.contains(&settings::Tab::Skills));
  assert_eq!(
    host.writes(),
    [
      "set feature_preview.skillsInstallation=true",
      "notify settingsChanged",
      "open http://localhost:3001/docs/concepts/logs",
    ]
  );
}

#[gpui::test]
fn settings_scrolls_its_content(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1100.), gpui::px(400.)));
  h.cx.run_until_parked();
  let before = h.cx.debug_bounds("settings-auto-push").expect("drawn");
  let scroll = h.cx.debug_bounds("settings-scroll").expect("drawn");
  h.cx.simulate_event(gpui::ScrollWheelEvent {
    position: scroll.center(),
    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-200.))),
    ..Default::default()
  });
  h.cx.run_until_parked();
  let after = h.cx.debug_bounds("settings-auto-push").expect("drawn");
  assert!(after.origin.y < before.origin.y, "{before:?} -> {after:?}");
}

#[gpui::test]
fn settings_fields_are_as_tall_as_react_ones(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.cx.run_until_parked();
  // `ui/input.tsx` and the `<select>`s are `h-10`.
  for selector in ["settings-branch-pattern", "settings-repo-default-model"] {
    let bounds = h.cx.debug_bounds(selector).expect("drawn");
    assert_eq!(bounds.size.height, gpui::px(40.), "{selector}");
  }
}

const WORKSPACE_JSON: &str = r#"{
  "header": {
    "branch": "feature/login",
    "home": false,
    "linear": {"key": "ENG-12", "url": "https://linear.app/t/ENG-12", "title": "Login"},
    "target": {"branch": "main", "loading": false, "disabled": false},
    "homeTarget": null,
    "stack": null,
    "push": null,
    "viewPr": {"number": 42, "state": "OPEN", "draft": false},
    "createPr": null,
    "statusLoading": false,
    "sync": {"ahead": 2, "behind": 0, "disabled": false, "spinning": false},
    "checks": {"status": "passed", "treq": "3/3", "github": {"label": "GH 4/4", "state": "success"}},
    "mergeQueue": null,
    "merge": {"passing": true, "disabled": false, "tooltip": null},
    "details": true,
    "more": {"disabled": false},
    "title": "Add login"
  },
  "tabRow": {
    "tab": "overview",
    "commits": {"kind": "count", "count": 3, "tone": "muted"},
    "changes": {"count": 5, "tone": "uncommitted"},
    "logs": false,
    "busy": null,
    "loc": {"insertions": 12, "deletions": 4}
  }
}"#;

fn route_json(workspace_id: Option<i64>, home_branch: &str) -> serde_json::Value {
  serde_json::json!({
    "kind": "workspace",
    "dark": false,
    "repoPath": "/repo",
    "workspaceId": workspace_id,
    "homeBranch": home_branch,
    "tab": "overview",
    "tabRow": true,
    "retargeting": false,
    "refreshing": false,
    "creatingPr": false,
    "runningChecks": [],
    "mergeQueue": null,
    "revision": 0
  })
}

fn workspace_route() -> ScreenSnapshot {
  serde_json::from_value(route_json(Some(7), "main")).expect("workspace route")
}

fn route_of(snapshot: &ScreenSnapshot) -> &workspace_data::Route {
  match &snapshot.view {
    View::Workspace(route) => route,
    _ => unreachable!(),
  }
}

fn workspace_row(branch: &str) -> serde_json::Value {
  serde_json::json!({
    "id": 7,
    "repo_path": "/repo",
    "workspace_name": "login",
    "workspace_path": "/repo/.treq/workspaces/login",
    "branch_name": branch,
    "created_at": "2026-10-19T00:00:00Z",
    "refreshed_at": null,
    "metadata": r#"{"linear_issue_key":"ENG-12","linear_issue_url":"https://linear.app/t/ENG-12","linear_issue_title":"Login"}"#,
    "target_branch": "main",
    "title": "Add login",
    "description": null,
    "moved_files": null,
    "not_on_remote": false,
    "sparse_patterns": null
  })
}

fn commit(id: &str, insertions: u32, deletions: u32) -> serde_json::Value {
  serde_json::json!({
    "commit_id": id,
    "short_id": id,
    "change_id": id,
    "description": "work",
    "author_name": "Zn",
    "timestamp": "2026-10-19T00:00:00Z",
    "parent_ids": [],
    "is_working_copy": false,
    "bookmarks": [],
    "is_immutable": false,
    "insertions": insertions,
    "deletions": deletions
  })
}

/// A feature workspace two commits ahead of its remote, with an open pull
/// request, as the header in `WORKSPACE_JSON` shows.
fn feature_loaded() -> workspace_data::Loaded {
  let row = workspace_row("feature/login");
  let ahead = serde_json::json!({"hash": "a", "timestamp": "t", "message": "m"});
  let status = serde_json::json!({
    "current": row,
    "has_conflicts": false,
    "has_changes": true,
    "commits_ahead": 3,
    "conflicted_files": [],
    "remote_sync": {"type": "Ahead", "data": {"count": 2}},
    "target": null,
    "children": [],
    "dag_nodes": [],
    "conflicted_workspace_ids": [],
    "commits_ahead_of_target": [ahead, ahead, ahead]
  });
  let commits = serde_json::json!({
    "commits": [commit("a", 10, 4), commit("b", 2, 0)],
    "target_branch": "main",
    "workspace_branch": "feature/login"
  });
  workspace_data::Loaded {
    workspace: Some(serde_json::from_value(row).expect("workspace")),
    status: Some(serde_json::from_value(status).expect("status")),
    remote: Some(crate::github::GitRemoteInfo {
      owner: "tznc".into(),
      repo: "treq".into(),
      full_name: "tznc/treq".into(),
    }),
    commits: Some(serde_json::from_value(commits).expect("commits")),
    review_count: 5,
    uncommitted_count: 2,
    checks: Some(workspace_data::ChecksSummary {
      workflows: vec!["a.yml".into(), "b.yml".into(), "c.yml".into()],
      latest: ["a.yml", "b.yml", "c.yml"]
        .map(|f| (f.to_string(), "passed".to_string()))
        .to_vec(),
    }),
    linear_enabled: true,
    ..Default::default()
  }
}

fn open_pr() -> workspace_data::PrCache {
  workspace_data::PrCache {
    pr: Some(Some(crate::github::PrInfo {
      number: 42,
      title: "Add login".into(),
      state: "OPEN".into(),
      url: "https://github.com/tznc/treq/pull/42".into(),
      head_ref_name: "feature/login".into(),
      base_ref_name: "main".into(),
      merge_state_status: None,
      is_draft: false,
    })),
    ci: Some(crate::github::PrCiStatus {
      state: "success".into(),
      total: 4,
      passed: 4,
      failed: 0,
      pending: 0,
      checks: vec![],
    }),
  }
}

fn feature_host() -> FakeHost {
  FakeHost {
    loaded: feature_loaded(),
    pr: open_pr(),
    ..FakeHost::default()
  }
}

/// The workspace screen in a window as wide as the app's default.
fn workspace_harness(
  cx: &mut TestAppContext,
  host: FakeHost,
  snapshot: ScreenSnapshot,
) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let h = Harness::with_host(cx, snapshot, host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  (h, host)
}

fn press(id: &str) -> ScreenAction {
  ScreenAction::Press { id: id.into() }
}

#[test]
fn workspace_header_is_what_react_drew() {
  let expected: workspace::Workspace = serde_json::from_str(WORKSPACE_JSON).expect("parse");
  let (header, tab_row) = workspace_data::workspace_view(
    route_of(&workspace_route()),
    &feature_loaded(),
    &open_pr(),
    workspace_data::Pending::None,
  );
  assert_eq!(header, expected.header);
  assert_eq!(tab_row, expected.tab_row);
}

#[test]
fn workspace_header_offers_the_home_rebase() {
  let route: ScreenSnapshot = serde_json::from_value(route_json(None, "feat/home")).expect("route");
  let loaded = workspace_data::Loaded {
    home_ahead: 2,
    ..Default::default()
  };
  let (header, _) = workspace_data::workspace_view(
    route_of(&route),
    &loaded,
    &workspace_data::PrCache::default(),
    workspace_data::Pending::None,
  );
  assert!(header.home);
  assert_eq!(header.branch, "feat/home");
  let target = header.home_target.expect("home target");
  assert_eq!(
    target.rebase.expect("rebase").tooltip,
    "Rebase feat/home onto main (2 new commits)"
  );
  assert_eq!(
    header.stack.expect("stack").tooltip,
    "Create stacked workspace from feat/home"
  );
}

#[gpui::test]
fn workspace_header_sends_what_it_does_not_do_itself(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  for selector in [
    "workspace-target",
    "workspace-view-pr",
    "workspace-view-pr-web",
    "workspace-checks",
    "workspace-merge",
    "workspace-details",
    "workspace-more",
  ] {
    h.click(selector);
  }
  h.click("workspace-linear");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![
      press("workspace.target"),
      press("workspace.viewPr"),
      press("workspace.checks"),
      press("workspace.merge"),
      press("workspace.details"),
      press("workspace.more"),
    ]
  );
  assert_eq!(
    host.writes(),
    vec![
      "open https://github.com/tznc/treq/pull/42",
      "open https://linear.app/t/ENG-12",
    ]
  );
}

#[gpui::test]
fn workspace_header_syncs_itself(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.click("workspace-sync");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert_eq!(
    host.writes(),
    vec![
      "sync Some(7)",
      "notify toast Sync failed",
      "notify workspaceChanged",
    ]
  );
  let reads = host
    .calls()
    .iter()
    .filter(|c| c.starts_with("read workspace"))
    .count();
  assert_eq!(reads, 2, "it reads the workspace again after syncing");
}

#[gpui::test]
fn workspace_header_reads_again_when_react_refreshes(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("workspace-sync"));
  h.update(|s| s.dark = true);
  h.update(|s| match &mut s.view {
    View::Workspace(route) => route.revision = 3,
    _ => unreachable!(),
  });
  h.cx.run_until_parked();
  let reads: Vec<_> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read workspace"))
    .collect();
  assert_eq!(reads, vec!["read workspace 0", "read workspace 3"]);
  assert!(host.calls().contains(&"read pr feature/login".to_string()));
}

#[gpui::test]
fn workspace_tabs_switch_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("commits-tab-count"));
  assert!(h.drawn("review-change-count"));
  assert!(h.drawn("loc-diff-marker"));
  assert!(!h.drawn("workspace-tab-logs"));
  h.click("workspace-tab-changes");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![field("workspace.tab", "changes".into())]);
}

#[gpui::test]
fn workspace_leaves_the_tab_row_to_react_without_one(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("workspace-tab-row"));
  h.update(|s| match &mut s.view {
    View::Workspace(route) => route.tab_row = false,
    _ => unreachable!(),
  });
  assert!(!h.drawn("workspace-tab-row"));
  assert!(h.drawn("workspace-header"));
  assert!(h.drawn("workspace-frame"));
}

#[gpui::test]
fn workspace_frame_fills_the_space_below(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  h.cx.run_until_parked();
  let header = h.cx.debug_bounds("workspace-header").expect("header");
  let row = h.cx.debug_bounds("workspace-tab-row").expect("tab row");
  let frame = h.cx.debug_bounds("workspace-frame").expect("frame");
  assert_eq!(row.top(), header.bottom());
  assert_eq!(frame.top(), row.bottom());
  assert!(frame.size.height > gpui::px(100.));
}

#[gpui::test]
fn home_workspace_opens_the_branch_switcher(cx: &mut TestAppContext) {
  let route = serde_json::from_value(route_json(None, "main")).expect("route");
  let (mut h, _) = workspace_harness(cx, FakeHost::default(), route);
  h.click("workspace-branch");
  h.click("workspace-stack");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![press("workspace.branchSwitcher"), press("workspace.stack")]
  );
}

fn skill(
  id: &str,
  name: &str,
  installed: Option<&str>,
) -> crate::core::skills::SkillCatalogSkillView {
  serde_json::from_value(serde_json::json!({
    "id": id,
    "name": name,
    "description": format!("{name} helps"),
    "source": "anthropics/skills",
    "category": "docs",
    "url": format!("https://github.com/anthropics/skills/{id}"),
    "installed": installed.map(|scope| serde_json::json!({
      "id": id,
      "name": name,
      "checksum": "abc",
      "scope": scope,
    })),
  }))
  .expect("skill")
}

fn skills_harness(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(FakeHost {
    skills: vec![
      skill("pdf", "PDF", Some("application")),
      skill("docx", "Word", None),
    ],
    ..FakeHost::default()
  });
  let h = Harness::with_host(cx, settings_route("skills"), host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  (h, host)
}

fn settings_view(h: &mut Harness) -> Entity<settings::SettingsView> {
  h.cx.run_until_parked();
  h.screen
    .read_with(&h.cx, |screen, _| screen.settings.clone())
    .expect("settings drawn")
}

fn settings_act(h: &mut Harness, action: ScreenAction) {
  let view = settings_view(h);
  view.update(&mut h.cx, |view, cx| view.handle(action, cx));
  h.cx.run_until_parked();
}

#[gpui::test]
fn skills_tab_lists_and_filters_the_registry(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  let skills = drawn_settings(&mut h).skills.expect("skills tab");
  assert_eq!(skills.installed, 1);
  let names: Vec<_> = skills.rows.iter().map(|r| r.name.as_str()).collect();
  assert_eq!(names, ["PDF", "Word"]);
  assert_eq!(skills.rows[0].installed.as_deref(), Some("application"));
  assert!(h.drawn("skill-uninstall-pdf"));
  assert!(h.drawn("skill-install-docx"));
  assert_eq!(
    host.calls().iter().filter(|c| *c == "read skills").count(),
    1
  );

  settings_act(&mut h, field("skills.query", "WORD".into()));
  let names: Vec<_> = drawn_settings(&mut h)
    .skills
    .expect("skills tab")
    .rows
    .into_iter()
    .map(|r| r.name)
    .collect();
  assert_eq!(names, ["Word"]);

  settings_act(&mut h, field("skills.query", "".into()));
  settings_act(&mut h, field("skills.scope", "application".into()));
  let rows = drawn_settings(&mut h).skills.expect("skills tab").rows;
  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].id, "pdf");
}

#[gpui::test]
fn skills_install_asks_where_then_toasts_a_failure(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  h.click("skill-install-docx");
  assert!(h.drawn("skill-install-dialog"));
  assert_eq!(
    drawn_settings(&mut h)
      .skills
      .expect("skills tab")
      .dialog
      .map(|d| d.name),
    Some("Word".into())
  );
  h.click("skill-dialog-repository");
  h.cx.run_until_parked();
  assert!(!h.drawn("skill-install-dialog"));
  assert_eq!(
    host.writes(),
    [
      "install docx Repository Some(\"/repo\")",
      "notify toast Skill library",
    ]
  );
  let reads = host.calls().iter().filter(|c| *c == "read skills").count();
  assert_eq!(reads, 2, "it reads the registry again");
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn skills_uninstall_and_move_between_levels(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  h.click("skill-uninstall-pdf");
  h.cx.run_until_parked();
  settings_act(&mut h, field("skills.level:pdf", "repository".into()));
  h.click("skill-source-pdf");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "uninstall pdf",
      "scope pdf Repository",
      "open https://github.com/anthropics/skills/pdf",
    ]
  );
}

#[gpui::test]
fn skills_dialog_cancels(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  h.click("skill-install-docx");
  h.click("skill-dialog-cancel");
  assert!(!h.drawn("skill-install-dialog"));
  assert!(host.writes().is_empty());
}

fn merge_route() -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "mergePreview",
    "dark": false,
    "repoPath": "/repo",
    "workspaceId": 7,
  }))
  .expect("parse")
}

fn merge_workspace(metadata: Option<&str>) -> crate::local_db::Workspace {
  serde_json::from_value(serde_json::json!({
    "id": 7,
    "repo_path": "/repo",
    "workspace_name": "feature",
    "workspace_path": "feature",
    "branch_name": "feature",
    "created_at": "2026-10-19T00:00:00Z",
    "refreshed_at": null,
    "metadata": metadata,
    "target_branch": "develop",
    "title": "Feature",
    "description": null,
    "moved_files": null,
    "not_on_remote": false,
    "sparse_patterns": null,
  }))
  .expect("workspace")
}

fn merge_preview(commits: usize) -> merge::Preview {
  let commit = |i: usize| {
    serde_json::json!({
      "commit_id": format!("c{i}"),
      "short_id": format!("c{i}"),
      "change_id": format!("k{i}"),
      "description": format!("Change {i}"),
      "author_name": "Zn",
      "timestamp": "2026-10-19T00:00:00Z",
      "parent_ids": [],
      "is_working_copy": false,
      "bookmarks": [],
      "is_immutable": false,
      "insertions": 3,
      "deletions": 1,
    })
  };
  merge::Preview {
    workspace: Some(merge_workspace(Some(r#"{"description": "Adds a thing"}"#))),
    commits: Some(
      serde_json::from_value(serde_json::json!({
        "commits": (0..commits).map(commit).collect::<Vec<_>>(),
        "total_count": commits,
      }))
      .expect("commits"),
    ),
    diff: Some(
      serde_json::from_value(serde_json::json!({
        "committed_files": [{
          "path": "src/a.rs",
          "status": "M",
          "previous_path": null,
          "changed_line_count": 2,
          "diff_deferred": false,
        }],
        "hunks_by_file": [{
          "path": "src/a.rs",
          "hunks": [{
            "id": "h0",
            "header": "@@ -1 +1 @@",
            "lines": ["-old", "+new"],
            "patch": "",
          }],
        }],
        "too_large_to_render": false,
      }))
      .expect("diff"),
    ),
    error: None,
  }
}

fn merge_harness(cx: &mut TestAppContext, host: FakeHost) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let mut h = Harness::with_host(cx, merge_route(), host.clone());
  assert!(h.drawn("merge-confirm"));
  (h, host)
}

fn merge_view<T>(h: &mut Harness, f: impl FnOnce(&merge::MergeView) -> T) -> T {
  h.screen.read_with(&h.cx, |screen, cx| {
    f(screen.merge.as_ref().expect("merge view").read(cx))
  })
}

#[test]
fn merge_messages_follow_the_strategy() {
  let ws = merge_workspace(Some(r#"{"description": "Adds a thing"}"#));
  assert_eq!(
    merge::default_message(&ws, merge::Strategy::Merge),
    "Merge feature into develop\n\nAdds a thing"
  );
  assert_eq!(
    merge::default_message(&ws, merge::Strategy::Squash),
    "Adds a thing"
  );
  assert_eq!(merge::default_message(&ws, merge::Strategy::Rebase), "");
  let bare = merge_workspace(Some("not json"));
  assert_eq!(
    merge::default_message(&bare, merge::Strategy::Merge),
    "Merge feature into develop"
  );
}

#[gpui::test]
fn merge_preview_merges_then_hands_back(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(2),
      ..FakeHost::default()
    },
  );
  assert!(h.drawn("merge-file-src/a.rs"));
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "merge 7 Merge \"Merge feature into develop\\n\\nAdds a thing\"",
      "notify toast Workspace merged",
    ]
  );
  assert_eq!(
    h.sent(),
    vec![ScreenAction::Press {
      id: "mergePreview.merged".into()
    }]
  );
}

#[gpui::test]
fn merge_preview_picks_a_strategy(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(2),
      merge_error: Some("conflicts".into()),
      ..FakeHost::default()
    },
  );
  h.click("merge-strategy");
  assert!(h.drawn("merge-strategy-menu"));
  h.click("merge-strategy-rebase");
  assert!(!h.drawn("merge-strategy-menu"));
  assert_eq!(
    merge_view(&mut h, |v| (v.strategy, v.message())),
    (merge::Strategy::Rebase, String::new())
  );
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["merge 7 Rebase \"\"", "notify toast Merge failed"]
  );
  assert!(h.sent().is_empty());
  assert!(!merge_view(&mut h, |v| v.merging));
}

#[gpui::test]
fn merge_preview_keeps_what_was_typed(cx: &mut TestAppContext) {
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(1),
      ..FakeHost::default()
    },
  );
  h.screen.update(&mut h.cx, |screen, cx| {
    let view = screen.merge.clone().expect("merge view");
    view.update(cx, |view, _| view.draft = Some("Ship it".into()));
  });
  h.click("merge-strategy");
  h.click("merge-strategy-squash");
  assert_eq!(merge_view(&mut h, |v| v.message()), "Ship it");
}

#[gpui::test]
fn merge_preview_collapses_files_and_goes_back(cx: &mut TestAppContext) {
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(1),
      ..FakeHost::default()
    },
  );
  assert!(h.drawn("merge-hunk-0-0"));
  h.click("merge-file-src/a.rs");
  assert!(!h.drawn("merge-hunk-0-0"));
  h.click("merge-back");
  assert_eq!(
    h.sent(),
    vec![ScreenAction::Press {
      id: "mergePreview.cancel".into()
    }]
  );
}

#[gpui::test]
fn merge_preview_without_commits_cannot_merge(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge::Preview {
        error: Some("no such revision".into()),
        ..merge::Preview::default()
      },
      ..FakeHost::default()
    },
  );
  h.click("merge-confirm");
  h.click("merge-strategy");
  assert!(!h.drawn("merge-strategy-menu"));
  assert_eq!(host.writes(), ["notify toast Failed to load merge preview"]);
}
