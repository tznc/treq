#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  reason = "test code"
)]
//! A stack of workspaces, each on the branch below it, over 200 tracked
//! files. Every workspace has a commit and an uncommitted edit, as agents
//! at work leave them. Covers what a stack costs: the top workspace's
//! Commits read (which lists each lower workspace's working copy), the
//! sidebar statuses, a commit at the bottom (which rebases the workspace
//! above it), and restacking everything after the default branch moves.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::fs;
use std::path::PathBuf;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;
use treq_lib::local_db::Workspace;

fn edit(repo: &TestRepo, workspace: &Workspace, name: &str, content: &str) {
  let path = repo.workspaces_dir().join(&workspace.workspace_path);
  fs::write(path.join(name), content).expect("failed to write workspace file");
}

/// Builds the stack bottom first; returns it in that order.
fn setup_stack(levels: usize) -> (TestRepo, Vec<Workspace>) {
  let repo = TestRepo::new().expect("failed to init test repo");
  for index in 0..200 {
    repo
      .create_file(
        &format!("src/module_{}/file_{index}.rs", index % 10),
        &format!("pub const VALUE: usize = {index};\n"),
      )
      .expect("failed to write tracked file");
  }
  repo
    .commit_file("README.md", "bench\n", "track 200 files")
    .expect("failed to commit tracked files");

  let mut stack: Vec<Workspace> = Vec::with_capacity(levels);
  for level in 1..=levels {
    let target = stack.last().map(|w| w.branch_name.clone());
    let workspace = treq_lib::core::create_workspace(
      &repo.repo_path,
      &format!("feat/stack-{level}"),
      None,
      None,
      target.as_deref(),
      None,
      None,
    )
    .expect("failed to create stacked workspace");
    edit(
      &repo,
      &workspace,
      &format!("level-{level}.txt"),
      "committed\n",
    );
    treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, &format!("level {level}"))
      .expect("failed to commit stacked workspace");
    stack.push(workspace);
  }
  for (level, workspace) in stack.iter().enumerate() {
    edit(
      &repo,
      workspace,
      &format!("src/module_{level}/file_{level}.rs"),
      "uncommitted\n",
    );
  }

  let app_dir: PathBuf = repo.temp_dir.path().join("app-data");
  fs::create_dir_all(&app_dir).expect("failed to create benchmark app dir");
  std::env::set_var("TREQ_APP_DATA_DIR", app_dir.to_string_lossy().to_string());
  let db =
    treq_lib::db::Database::new(app_dir.join("treq.db")).expect("failed to open benchmark app db");
  db.init().expect("failed to initialize benchmark app db");

  (repo, stack)
}

fn bench_reads(c: &mut Criterion) {
  let mut group = c.benchmark_group("stacked");
  group.sample_size(10);
  for levels in [3, 5] {
    let (repo, stack) = setup_stack(levels);
    let top = stack.last().expect("stack").id;
    let read = || {
      treq_lib::core::list_commits(&repo.repo_path, Some(top), true, None, None)
        .expect("list_commits failed")
    };
    let log = read();
    assert_eq!(
      log
        .commits
        .iter()
        .filter(|c| c.workspace_label.is_some())
        .count(),
      levels - 1,
      "each lower workspace's working copy is listed"
    );
    group.bench_function(format!("commits_read_top_of_{levels}"), |b| {
      b.iter(|| std::hint::black_box(read()))
    });
    group.bench_function(format!("statuses_{levels}"), |b| {
      b.iter(|| {
        std::hint::black_box(
          treq_lib::core::list_workspace_statuses(&repo.repo_path).expect("statuses"),
        )
      })
    });
  }
  group.finish();
}

fn bench_rewrites(c: &mut Criterion) {
  let mut group = c.benchmark_group("stacked");
  group.sample_size(10);

  // Committing at the bottom rebases the workspace stacked on it.
  group.bench_function("commit_bottom_of_5", |b| {
    b.iter_batched_ref(
      || setup_stack(5),
      |(repo, stack)| {
        treq_lib::core::commit_workspace(&repo.repo_path, stack[0].id, "bottom")
          .expect("commit failed");
      },
      BatchSize::PerIteration,
    );
  });

  // The default branch moved: sync every stack (`check_and_rebase_all`).
  group.bench_function("rebase_all_after_main_moves_5", |b| {
    b.iter_batched_ref(
      || {
        let (repo, stack) = setup_stack(5);
        repo
          .commit_file("main-update.txt", "advance\n", "advance main")
          .expect("failed to advance main");
        (repo, stack)
      },
      |(repo, _)| {
        treq_lib::core::check_and_rebase_workspaces(&repo.repo_path, None, None, None, "diff")
          .expect("rebase failed");
      },
      BatchSize::PerIteration,
    );
  });

  // Restack from the top: every workspace onto the one below it.
  group.bench_function("restack_from_top_of_5", |b| {
    b.iter_batched_ref(
      || {
        let (repo, stack) = setup_stack(5);
        repo
          .commit_file("main-update.txt", "advance\n", "advance main")
          .expect("failed to advance main");
        (repo, stack)
      },
      |(repo, stack)| {
        let default_branch = repo.default_branch().to_string();
        treq_lib::core::check_and_rebase_workspaces(
          &repo.repo_path,
          Some(stack[4].id),
          Some(default_branch),
          Some(true),
          "diff",
        )
        .expect("restack failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(benches, bench_reads, bench_rewrites);
criterion_main!(benches);
