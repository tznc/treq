#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::string_slice,
  reason = "test code"
)]
//! What a repo window reads every 10 s (`Read::POLL`): the workspace list
//! and the sidebar statuses, with 5 and 25 workspaces. Each workspace has
//! an uncommitted file, as an agent at work leaves it.

use criterion::{criterion_group, criterion_main, Criterion};
use std::fs;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

fn setup(count: usize) -> TestRepo {
  let repo = TestRepo::new().expect("failed to init test repo");
  for index in 1..=count {
    let workspace = treq_lib::core::create_workspace(
      &repo.repo_path,
      &format!("feat/poll-{index}"),
      None,
      None,
      None,
      None,
      None,
    )
    .expect("failed to create workspace");
    let path = repo
      .workspaces_dir()
      .join(&workspace.workspace_path)
      .join(format!("poll-{index}.txt"));
    fs::write(path, "agent edit\n").expect("failed to write workspace file");
  }
  repo
}

fn bench_window_poll(c: &mut Criterion) {
  let mut group = c.benchmark_group("window_poll");
  group.sample_size(10);
  // Criterion's name filter: the argument that is neither a flag nor a
  // flag's value.
  let args: Vec<String> = std::env::args().skip(1).collect();
  let filter = args
    .iter()
    .enumerate()
    .rfind(|(i, a)| {
      !a.starts_with('-') && (*i == 0 || !args[i - 1].starts_with("--") || args[i - 1] == "--bench")
    })
    .map(|(_, a)| a.clone());
  for count in [5, 25, 100, 200] {
    // Setting up 200 workspaces takes a while; skip what the filter leaves out.
    let id = format!("window_poll/{count}_workspaces");
    if filter.as_deref().is_some_and(|f| !id.contains(f)) {
      continue;
    }
    let repo = setup(count);
    group.bench_function(format!("{count}_workspaces"), |b| {
      b.iter(|| {
        let workspaces = treq_lib::core::list_workspaces(&repo.repo_path).expect("workspaces");
        let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path).expect("statuses");
        std::hint::black_box((workspaces, statuses));
      })
    });
  }
  // A detail read, or a new workspace, also asks whether each has changes.
  for count in [5, 100] {
    let id = format!("window_poll/{count}_workspaces_has_changes");
    if filter.as_deref().is_some_and(|f| !id.contains(f)) {
      continue;
    }
    let repo = setup(count);
    let workspaces = treq_lib::core::list_workspaces(&repo.repo_path).expect("workspaces");
    group.bench_function(format!("{count}_workspaces_has_changes"), |b| {
      b.iter(|| {
        for workspace in &workspaces {
          std::hint::black_box(
            treq_lib::core::workspace_has_changes(&repo.repo_path, workspace.id).ok(),
          );
        }
      })
    });
    // As the window reads them: all at once.
    group.bench_function(format!("{count}_workspaces_has_changes_together"), |b| {
      b.iter(|| {
        std::hint::black_box(treq_lib::core::workspaces_have_changes(
          &repo.repo_path,
          &workspaces,
        ))
      })
    });
  }
  group.finish();
}

criterion_group!(benches, bench_window_poll);
criterion_main!(benches);
