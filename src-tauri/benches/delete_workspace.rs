#![allow(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::string_slice,
  reason = "test code"
)]
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

fn setup_delete_workspace() -> (TestRepo, i64) {
  let repo = TestRepo::new().expect("failed to init test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/delete-bench",
    Some("delete bench workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("failed to create workspace");

  (repo, workspace.id)
}

fn bench_delete_workspace(c: &mut Criterion) {
  let mut group = c.benchmark_group("delete_workspace");
  group.sample_size(20);

  group.bench_function("default", |b| {
    b.iter_batched_ref(
      setup_delete_workspace,
      |(repo, workspace_id)| {
        treq_lib::core::delete_workspace(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box(workspace_id),
        )
        .expect("delete_workspace failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(benches, bench_delete_workspace);
criterion_main!(benches);
