use gpui::TestAppContext;

use super::*;

/// The preview with PR #7 for `feature` on GitHub (`o/r`), whose default
/// merge method is squash and which forbids rebase merges.
fn github_preview(state: &str, merge_state: &str, base: &str) -> merge::Preview {
  merge::Preview {
    github: Some(merge::GithubPr {
      repo_full_name: "o/r".into(),
      pr: crate::github::PrInfo {
        number: 7,
        title: "Feature".into(),
        state: state.into(),
        url: "https://github.com/o/r/pull/7".into(),
        head_ref_name: "feature".into(),
        base_ref_name: base.into(),
        merge_state_status: Some(merge_state.into()),
        is_draft: false,
        body: String::new(),
      },
      settings: Some(crate::github::GhRepoMergeSettings {
        default_method: crate::github::GhMergeMethod::Squash,
        allowed_methods: vec![
          crate::github::GhMergeMethod::Merge,
          crate::github::GhMergeMethod::Squash,
        ],
      }),
    }),
    can_push: true,
    ..merge_preview(2)
  }
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
fn merge_preview_without_a_pr_merges_locally_only(cx: &mut TestAppContext) {
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge: merge_preview(1),
      ..FakeHost::default()
    },
  );
  assert!(h.drawn("merge-landing"));
  assert!(!h.drawn("merge-location-github"));
  assert!(!h.drawn("merge-push"));
}

#[gpui::test]
fn merge_preview_lists_children_it_retargets(cx: &mut TestAppContext) {
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge: merge::Preview {
        plan: Some(merge_plan(&["child"], false)),
        ..merge_preview(1)
      },
      ..FakeHost::default()
    },
  );
  assert_eq!(
    merge_view(&mut h, |v| v.landing_lines()),
    [
      "feature lands in develop.",
      "child is retargeted onto develop (was feature)",
    ]
  );
}

#[gpui::test]
fn merge_preview_pushes_the_target_by_default(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge::Preview {
        can_push: true,
        ..merge_preview(1)
      },
      ..FakeHost::default()
    },
  );
  assert!(h.drawn("merge-push"));
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert!(host.writes()[0].ends_with(" push"), "{:?}", host.writes());
}

#[gpui::test]
fn merge_preview_skips_the_push_when_unticked(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge::Preview {
        can_push: true,
        ..merge_preview(1)
      },
      ..FakeHost::default()
    },
  );
  h.click("merge-push");
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert!(!host.writes()[0].ends_with(" push"), "{:?}", host.writes());
}

#[gpui::test]
fn merge_preview_rolls_up_a_stack(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: merge::Preview {
        plan: Some(merge_plan(&[], true)),
        rollup_root: Some("main".into()),
        ..merge_preview(1)
      },
      ..FakeHost::default()
    },
  );
  h.click("merge-rollup");
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"read merge 7 rollup".to_string()));
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert!(host.writes()[0].ends_with(" rollup"), "{:?}", host.writes());
}

#[gpui::test]
fn merge_preview_merges_the_pr_on_github_with_the_repo_default(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: github_preview("OPEN", "CLEAN", "develop"),
      merge_notes: vec!["Moved PR #8 (child) onto develop".into()],
      ..FakeHost::default()
    },
  );
  h.click("merge-location-github");
  assert_eq!(merge_view(&mut h, |v| v.strategy), merge::Strategy::Squash);
  assert!(!h.drawn("merge-push"));
  h.click("merge-strategy");
  assert!(h.drawn("merge-strategy-squash"));
  assert!(!h.drawn("merge-strategy-rebase"));
  h.click("merge-strategy-squash");
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "merge on github 7 o/r#7 Squash None",
      "notify toast Pull request merged",
      "notify toast Stacked pull requests updated",
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
fn merge_preview_hints_the_github_message_is_optional(cx: &mut TestAppContext) {
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge: github_preview("OPEN", "CLEAN", "develop"),
      ..FakeHost::default()
    },
  );
  let placeholder = |h: &mut Harness| {
    h.screen.read_with(&h.cx, |screen, cx| {
      screen
        .merge
        .as_ref()
        .expect("merge view")
        .read(cx)
        .message_placeholder()
    })
  };
  assert_eq!(placeholder(&mut h), "Enter merge commit message...");
  h.click("merge-location-github");
  assert!(h.drawn("merge-message"));
  assert_eq!(
    placeholder(&mut h),
    "Leave empty to use GitHub's default message"
  );
}

#[gpui::test]
fn merge_preview_warns_what_github_reports(cx: &mut TestAppContext) {
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge: github_preview("OPEN", "DIRTY", "develop"),
      ..FakeHost::default()
    },
  );
  assert!(!h.drawn("merge-warning-0"));
  h.click("merge-location-github");
  assert!(h.drawn("merge-warning-0"));
  assert_eq!(
    merge_view(&mut h, |v| v.warnings()),
    ["This pull request has merge conflicts on GitHub. Resolve them before merging."]
  );
}

#[gpui::test]
fn merge_preview_blocks_a_pr_with_another_base(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: github_preview("OPEN", "CLEAN", "main"),
      ..FakeHost::default()
    },
  );
  h.click("merge-location-github");
  h.click("merge-confirm");
  h.cx.run_until_parked();
  assert!(host.writes().is_empty());
}

#[gpui::test]
fn merge_preview_syncs_a_pr_already_merged(cx: &mut TestAppContext) {
  let (mut h, host) = merge_harness(
    cx,
    FakeHost {
      merge: github_preview("MERGED", "CLEAN", "develop"),
      ..FakeHost::default()
    },
  );
  assert!(!h.drawn("merge-location-github"));
  h.click("merge-sync");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["sync merged 7", "notify toast Workspace synced"]
  );
  assert_eq!(
    h.sent(),
    vec![ScreenAction::Press {
      id: "mergePreview.merged".into()
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

#[gpui::test]
fn stack_line_counts_stay_on_one_line(cx: &mut TestAppContext) {
  let host = FakeHost {
    workspaces: [
      workspace_row("feature/login"),
      serde_json::json!({"id": 8, "branch_name": "feature/logout", "target_branch": "feature/login"}),
    ]
    .into_iter()
    .map(|row| {
      let mut base = workspace_row("feature/login");
      base.as_object_mut().expect("row").extend(row.as_object().expect("row").clone());
      serde_json::from_value(base).expect("workspace")
    })
    .collect(),
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  let counts = h
    .cx
    .debug_bounds("workspace-loc-indicator")
    .expect("indicator");
  // One `text-xs` line, as React's grid of `ch` columns keeps it.
  assert!(counts.size.height <= gpui::px(16.), "{counts:?}");
}
