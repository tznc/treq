use gpui::TestAppContext;

use super::*;

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

fn labels(rows: &[settings::SkillRow]) -> Vec<(&str, Vec<&str>)> {
  rows
    .iter()
    .map(|r| {
      (
        r.name.as_str(),
        r.labels.iter().map(String::as_str).collect(),
      )
    })
    .collect()
}

fn custom_skill(
  name: &str,
  level: crate::core::skills::CustomSkillLevel,
) -> crate::core::skills::CustomSkill {
  // Repository copies are relative to it; global ones are full paths.
  let root = match level {
    crate::core::skills::CustomSkillLevel::Repo => "",
    crate::core::skills::CustomSkillLevel::Global => "/home/u/",
  };
  crate::core::skills::CustomSkill {
    name: name.into(),
    description: Some(format!("{name} by hand")),
    level,
    paths: vec![format!("{root}.claude/skills/{name}")],
  }
}

/// Every kind of skill: registry ones at each level, one customized here,
/// one the registry dropped, and hand-written ones in the repo and home.
fn skills_status_harness(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  use crate::core::skills::CustomSkillLevel::{Global, Repo};
  let mut pptx = skill("pptx", "PowerPoint", Some("application"));
  pptx.customized = true;
  let host = Arc::new(FakeHost {
    skills: vec![
      skill("pdf", "PDF", Some("application")),
      skill("docx", "Word", None),
      skill("xlsx", "Excel", Some("repository")),
      pptx,
    ],
    uncataloged: vec![crate::core::skills::InstalledSkill {
      id: "old/gone".into(),
      name: "gone".into(),
      checksum: "abc".into(),
      scope: crate::core::skills::SkillInstallScope::Application,
    }],
    custom: vec![
      custom_skill("PowerPoint", Repo),
      custom_skill("deploy", Repo),
      custom_skill("notes", Global),
    ],
    ..FakeHost::default()
  });
  let h = Harness::with_host(cx, settings_route("skills"), host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  (h, host)
}

#[gpui::test]
fn skills_list_labels_every_skill_by_install_status(cx: &mut TestAppContext) {
  let (mut h, _) = skills_status_harness(cx);
  let skills = drawn_settings(&mut h).skills.expect("skills tab");
  assert_eq!(
    labels(&skills.rows),
    [
      ("PDF", vec!["App-wide"]),
      ("Excel", vec!["Repository"]),
      ("PowerPoint", vec!["App-wide", "Custom · Repo"]),
      ("gone", vec!["App-wide", "Not in registry"]),
      ("deploy", vec!["Custom · Repo"]),
      ("notes", vec!["Custom · Global"]),
      ("Word", vec![]),
    ]
  );
  let chips: Vec<_> = skills
    .filters
    .iter()
    .map(|f| (f.label, f.selected))
    .collect();
  assert_eq!(
    chips,
    [
      ("Installed", false),
      ("App-wide", false),
      ("Repository", false),
      ("Custom", false),
      ("Not installed", false),
    ]
  );
  assert_eq!(
    skills.rows[4].description.as_deref(),
    Some("deploy by hand")
  );
  assert!(
    !h.drawn("skill-filter-all"),
    "no filter selected shows every skill"
  );
}

#[gpui::test]
fn skill_filter_chips_narrow_the_list(cx: &mut TestAppContext) {
  let (mut h, _) = skills_status_harness(cx);
  let shown = |h: &mut Harness| -> Vec<String> {
    drawn_settings(h)
      .skills
      .expect("skills tab")
      .rows
      .into_iter()
      .map(|r| r.name)
      .collect()
  };
  h.click("skill-filter-custom");
  assert_eq!(
    shown(&mut h),
    ["PowerPoint", "deploy", "notes"],
    "Custom covers repo and global copies"
  );
  h.click("skill-filter-repository");
  assert_eq!(shown(&mut h), ["Excel"]);
  h.click("skill-filter-not-installed");
  assert_eq!(shown(&mut h), ["Word"]);
  h.click("skill-filter-installed");
  settings_act(&mut h, field("skills.query", "NOTE".into()));
  assert_eq!(shown(&mut h), ["notes"], "the search narrows the filter");
  let chips = drawn_settings(&mut h).skills.expect("skills tab").filters;
  assert!(chips.iter().any(|f| f.id == "installed" && f.selected));

  settings_act(&mut h, field("skills.query", "".into()));
  h.click("skill-filter-installed");
  assert_eq!(
    shown(&mut h).len(),
    7,
    "clicking the selected chip clears the filter"
  );
  let chips = drawn_settings(&mut h).skills.expect("skills tab").filters;
  assert!(chips.iter().all(|f| !f.selected));
}

#[gpui::test]
fn hand_written_skill_edits_with_an_agent_and_cannot_uninstall(cx: &mut TestAppContext) {
  let (mut h, host) = skills_status_harness(cx);
  assert!(!h.drawn("skill-uninstall-custom-global/notes"));
  assert!(
    !h.drawn("skill-customize-old/gone"),
    "no catalog copy to customize from"
  );
  assert!(h.drawn("skill-uninstall-old/gone"));
  h.click("skill-filter-custom");
  h.click("skill-customize-custom-global/notes");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["customize-global notes /home/u/.claude/skills/notes /repo"],
    "a global skill is copied into the repository first"
  );
  let prompt = started_prompt(&h);
  assert!(prompt.contains(".agents/skills/notes/"), "{prompt}");
  assert!(!prompt.contains("/home/"), "no home path: {prompt}");
}

fn started_prompt(h: &Harness) -> String {
  let sent = h.sent();
  let [ScreenAction::Field { field, value }] = &sent[..] else {
    panic!("not one field: {sent:?}");
  };
  assert_eq!(field, "skills.startAgent");
  value["prompt"].as_str().expect("prompt").to_string()
}

#[gpui::test]
fn customise_a_skill_not_yet_installed_pulls_it_from_the_registry(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  h.click("skill-customize-docx");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["customize-registry docx /repo"]);
  let prompt = started_prompt(&h);
  assert!(prompt.contains(".agents/skills/Word/"), "{prompt}");
}

#[gpui::test]
fn hand_written_repo_skill_prompt_uses_repo_paths(cx: &mut TestAppContext) {
  let (mut h, host) = skills_status_harness(cx);
  h.click("skill-filter-custom");
  h.click("skill-customize-custom-repo/deploy");
  h.cx.run_until_parked();
  assert!(host.writes().is_empty(), "it is already the user's copy");
  let prompt = started_prompt(&h);
  assert!(prompt.contains(".claude/skills/deploy/"), "{prompt}");
  assert!(!prompt.contains("/repo/"), "{prompt}");
}

#[gpui::test]
fn skill_link_without_a_repository_installs_app_wide(cx: &mut TestAppContext) {
  let mut docx = skill("docx", "Word", None);
  docx.skill.checksum = Some("c0ffee".into());
  let host = Arc::new(FakeHost {
    skills: vec![docx],
    ..FakeHost::default()
  });
  let route = serde_json::from_value(serde_json::json!({
    "kind": "settings",
    "dark": false,
    "tab": "skills",
    "repoPath": null,
    "installRequest": "docx",
  }))
  .expect("parse");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  assert!(h.drawn("skill-confirm-dialog"));
  h.click("skill-confirm-install");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes()[0],
    "install docx Application None",
    "app-wide needs no repository"
  );
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

fn skills_link_harness(cx: &mut TestAppContext, request: &str) -> (Harness, Arc<FakeHost>) {
  let mut docx = skill("docx", "Word", None);
  docx.skill.license = Some("MIT".into());
  docx.skill.checksum = Some("c0ffee".into());
  docx.skill.route = Some("/skills/anthropics/docx".into());
  docx.skill.files = vec![crate::core::skills::SkillFile {
    path: "SKILL.md".into(),
    size: 2048,
    binary: false,
    github_url: None,
    raw_url: None,
  }];
  let host = Arc::new(FakeHost {
    skills: vec![skill("pdf", "PDF", Some("application")), docx],
    ..FakeHost::default()
  });
  let route = serde_json::from_value(serde_json::json!({
    "kind": "settings",
    "dark": false,
    "tab": "skills",
    "repoPath": "/repo",
    "currentBranch": "main",
    "installRequest": request,
  }))
  .expect("parse");
  let h = Harness::with_host(cx, route, host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  (h, host)
}

#[gpui::test]
fn skill_link_shows_catalog_details_before_installing(cx: &mut TestAppContext) {
  let (mut h, host) = skills_link_harness(cx, "docx");
  assert!(h.drawn("skill-confirm-dialog"));
  let confirm = drawn_settings(&mut h)
    .skills
    .expect("skills tab")
    .confirm
    .expect("confirm dialog");
  assert_eq!(confirm.title, "Install Word for all agents?");
  assert_eq!(confirm.description.as_deref(), Some("Word helps"));
  assert_eq!(confirm.checksum.as_deref(), Some("c0ffee"));
  assert_eq!(
    confirm.registry_url,
    "https://treq.dev/skills/anthropics/docx"
  );
  let details: Vec<_> = confirm
    .details
    .iter()
    .map(|(k, v)| format!("{k}: {v}"))
    .collect();
  assert_eq!(
    details,
    [
      "Catalog id: docx",
      "Source: anthropics/skills",
      "Category: docs",
      "License: MIT",
      "Files: 1 file, 2.0 KB",
    ]
  );
  assert!(confirm.can_install);

  h.click("skill-confirm-registry");
  h.click("skill-confirm-install");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "open https://treq.dev/skills/anthropics/docx",
      "install docx Application Some(\"/repo\")",
      "notify toast Skill library",
    ]
  );
  assert_eq!(h.sent(), vec![press("skills.installRequest.close")]);
}

#[gpui::test]
fn skill_link_for_unknown_skill_offers_no_install(cx: &mut TestAppContext) {
  let (mut h, host) = skills_link_harness(cx, "evil/payload");
  let confirm = drawn_settings(&mut h)
    .skills
    .expect("skills tab")
    .confirm
    .expect("confirm dialog");
  assert!(!confirm.can_install);
  assert!(confirm
    .notice
    .as_deref()
    .is_some_and(|n| n.contains("not in the Treq skill catalog")));
  assert!(!h.drawn("skill-confirm-install"));
  h.click("skill-confirm-cancel");
  h.cx.run_until_parked();
  assert!(host.writes().is_empty());
  assert_eq!(h.sent(), vec![press("skills.installRequest.close")]);
}

#[gpui::test]
fn skill_link_for_installed_skill_says_so(cx: &mut TestAppContext) {
  let (mut h, _) = skills_link_harness(cx, "pdf");
  let confirm = drawn_settings(&mut h)
    .skills
    .expect("skills tab")
    .confirm
    .expect("confirm dialog");
  assert!(!confirm.can_install);
  assert!(confirm
    .notice
    .as_deref()
    .is_some_and(|n| n.contains("already installed")));
}

#[gpui::test]
fn customize_writes_a_copy_and_asks_for_an_agent(cx: &mut TestAppContext) {
  let (mut h, host) = skills_harness(cx);
  assert!(
    h.drawn("skill-customize-docx"),
    "not-installed skills customise from the registry"
  );
  h.click("skill-customize-pdf");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["customize pdf /repo"]);
  let sent = h.sent();
  assert_eq!(sent.len(), 1);
  let ScreenAction::Field { field, value } = &sent[0] else {
    panic!("not a field: {sent:?}");
  };
  assert_eq!(field, "skills.startAgent");
  let prompt = value["prompt"].as_str().expect("prompt");
  assert!(prompt.contains(".agents/skills/PDF/"), "{prompt}");
  assert!(prompt.contains(".claude/skills/PDF/"), "{prompt}");
}

#[gpui::test]
fn customized_skill_shows_a_badge_and_edits_without_copying(cx: &mut TestAppContext) {
  let mut pdf = skill("pdf", "PDF", Some("application"));
  pdf.customized = true;
  let host = Arc::new(FakeHost {
    skills: vec![pdf],
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, settings_route("skills"), host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  let rows = drawn_settings(&mut h).skills.expect("skills tab").rows;
  assert_eq!(rows[0].labels, ["App-wide", "Custom · Repo"]);
  h.click("skill-customize-pdf");
  h.cx.run_until_parked();
  assert!(host.writes().is_empty(), "the copy is the user's");
  assert!(matches!(
    &h.sent()[..],
    [ScreenAction::Field { field, .. }] if field == "skills.startAgent"
  ));
}
