#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  reason = "test code"
)]
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

use treq_lib::core::MergeCommit;

const TARGET_BRANCH_COMMITS: usize = 2;
const WORKSPACE_COMMITS: usize = 5;

fn create_workspace(repo: &TestRepo) -> treq_lib::local_db::Workspace {
  treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/bench-merge-workspace",
    Some("merge workspace benchmark".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("failed to create workspace")
}

fn target_commit_content(index: usize) -> String {
  format!("target line 1 commit {index}\ntarget line 2 commit {index}\n")
}

fn workspace_commit_content(index: usize) -> String {
  format!("workspace line 1 commit {index}\nworkspace line 2 commit {index}\n")
}

fn add_target_branch_commits(repo: &TestRepo) {
  for index in 1..=TARGET_BRANCH_COMMITS {
    repo
      .commit_file(
        "target-history.txt",
        &target_commit_content(index),
        &format!("Benchmark target commit {index}"),
      )
      .unwrap_or_else(|e| panic!("failed to create target commit {index}: {e}"));
  }
}

fn add_workspace_commits(repo: &TestRepo, workspace: &treq_lib::local_db::Workspace) {
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);

  for index in 1..=WORKSPACE_COMMITS {
    std::fs::write(
      workspace_path.join("workspace-history.txt"),
      workspace_commit_content(index),
    )
    .unwrap_or_else(|e| panic!("failed to write workspace commit {index}: {e}"));

    treq_lib::core::commit_workspace(
      &repo.repo_path,
      workspace.id,
      &format!("Benchmark workspace commit {index}"),
    )
    .unwrap_or_else(|e| panic!("failed to create workspace commit {index}: {e}"));
  }
}

fn setup_merge_workspace_benchmark() -> (TestRepo, i64) {
  let repo = TestRepo::new().expect("failed to init test repo");
  let workspace = create_workspace(&repo);

  add_target_branch_commits(&repo);
  add_workspace_commits(&repo, &workspace);

  (repo, workspace.id)
}

fn bench_merge_strategy(
  c: &mut Criterion,
  group_name: &str,
  bench_name: &str,
  message: &str,
  strategy: MergeCommit,
) {
  let mut group = c.benchmark_group(group_name);
  group.sample_size(10);

  group.bench_function(bench_name, |b| {
    b.iter_batched_ref(
      setup_merge_workspace_benchmark,
      |(repo, workspace_id)| {
        treq_lib::core::merge_workspace(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box(*workspace_id),
          std::hint::black_box(message),
          std::hint::black_box(strategy),
        )
        .expect("merge_workspace failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

fn bench_merge_workspace_merge(c: &mut Criterion) {
  bench_merge_strategy(
    c,
    "merge_workspace_merge",
    "2_target_commits_5_workspace_commits",
    "Benchmark merge commit strategy",
    MergeCommit::Merge,
  );
}

fn bench_merge_workspace_rebase_and_merge(c: &mut Criterion) {
  bench_merge_strategy(
    c,
    "merge_workspace_rebase_and_merge",
    "2_target_commits_5_workspace_commits",
    "Benchmark rebase and merge strategy",
    MergeCommit::RebaseAndMerge,
  );
}

fn bench_merge_workspace_squash_and_merge(c: &mut Criterion) {
  bench_merge_strategy(
    c,
    "merge_workspace_squash_and_merge",
    "2_target_commits_5_workspace_commits",
    "Benchmark squash and merge strategy",
    MergeCommit::SquashAndMerge,
  );
}

criterion_group!(
  benches,
  bench_merge_workspace_merge,
  bench_merge_workspace_rebase_and_merge,
  bench_merge_workspace_squash_and_merge
);
criterion_main!(benches);
