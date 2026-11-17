#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  reason = "test code"
)]
//! The first read of a window that opens on a repository (`Read::ALL`), with
//! 5 and 25 workspaces that each hold an uncommitted file: what the sidebar
//! waits for. `listed` is the database part (the rows can draw from it),
//! `serial` is every read one after the other (what the window waited for
//! before it drew anything). `init_*` is the repository's setup, which a
//! repository opened from the start screen or the File menu waits for.

use criterion::{criterion_group, criterion_main, Criterion};
use std::fs;
use treq_lib::core;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

fn setup(count: usize) -> TestRepo {
  let repo = TestRepo::new().expect("failed to init test repo");
  for index in 1..=count {
    let workspace = core::create_workspace(
      &repo.repo_path,
      &format!("feat/open-{index}"),
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
      .join(format!("open-{index}.txt"));
    fs::write(path, "agent edit\n").expect("failed to write workspace file");
  }
  repo
}

fn listed(repo: &str) -> Vec<i64> {
  let workspaces = core::list_workspaces(repo).expect("workspaces");
  std::hint::black_box(treq_lib::local_db::list_supporting_repo_paths(repo).ok());
  workspaces.iter().map(|w| w.id).collect()
}

fn changes(repo: &str, ids: &[i64]) -> Vec<Option<bool>> {
  ids
    .iter()
    .map(|id| core::workspace_has_changes(repo, *id).ok())
    .collect()
}

fn detail(repo: &str) {
  std::hint::black_box((
    core::repo::get_repo_current_branch(repo).ok(),
    core::repo::get_repo_default_branch(repo).ok(),
    core::github_cli::remote(repo).ok(),
  ));
}

fn bench_window_open(c: &mut Criterion) {
  let mut group = c.benchmark_group("window_open");
  group.sample_size(10);
  for count in [5, 25] {
    let repo = setup(count);
    let path = repo.repo_path.as_str();
    // Opening a repository from the start screen or the File menu: the
    // folder is set up first, then the window opens. The rebase pass used to
    // run in that step; now it runs behind the window.
    group.bench_function(format!("init_with_rebase_{count}"), |b| {
      b.iter(|| core::init(path).expect("init"))
    });
    group.bench_function(format!("init_deferring_rebase_{count}"), |b| {
      b.iter(|| core::init_deferring_rebase(path).expect("init"))
    });
    group.bench_function(format!("listed_{count}"), |b| b.iter(|| listed(path)));
    group.bench_function(format!("serial_{count}"), |b| {
      b.iter(|| {
        let ids = listed(path);
        std::hint::black_box(core::list_workspace_statuses(path).ok());
        std::hint::black_box(changes(path, &ids));
        detail(path);
      })
    });
  }
  group.finish();
}

criterion_group!(benches, bench_window_open);
criterion_main!(benches);
