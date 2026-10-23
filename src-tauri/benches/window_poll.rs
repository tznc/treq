#![allow(clippy::unwrap_used, reason = "test code")]
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
  for count in [5, 25] {
    let repo = setup(count);
    group.bench_function(format!("{count}_workspaces"), |b| {
      b.iter(|| {
        let workspaces = treq_lib::core::list_workspaces(&repo.repo_path).expect("workspaces");
        let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path).expect("statuses");
        std::hint::black_box((workspaces, statuses));
      })
    });
  }
  group.finish();
}

criterion_group!(benches, bench_window_poll);
criterion_main!(benches);
