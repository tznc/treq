#![allow(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::string_slice,
  reason = "test code"
)]
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::fs;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

fn bench_commit_workspace(c: &mut Criterion) {
  let mut group = c.benchmark_group("commit_workspace");
  // Each iteration requires real git+jj subprocess calls, so keep sample size low.
  group.sample_size(100);

  group.bench_function("create_commit", |b| {
    b.iter_batched_ref(
      || {
        let repo = TestRepo::new().expect("failed to init test repo");
        let workspace = treq_lib::core::create_workspace(
          &repo.repo_path,
          "feat/bench",
          Some("bench workspace".to_string()),
          None,
          None,
          None,
          None,
        )
        .expect("failed to create workspace");
        let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
        fs::write(workspace_path.join("file.txt"), "bench content")
          .expect("failed to write test file");
        (repo, workspace)
      },
      |(repo, workspace)| {
        treq_lib::core::commit_workspace(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box(workspace.id),
          std::hint::black_box("bench commit"),
        )
        .expect("commit_workspace failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(benches, bench_commit_workspace);
criterion_main!(benches);
