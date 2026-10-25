#![expect(
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

fn create_moved_files(repo: &TestRepo) -> Vec<String> {
  let moved_files = vec![
    "feature1.rs".to_string(),
    "feature2.rs".to_string(),
    "feature3.rs".to_string(),
    "feature4.rs".to_string(),
    "feature5.rs".to_string(),
  ];

  for file in &moved_files {
    repo
      .create_file(file, &format!("// Benchmark content for {file}"))
      .expect("failed to create moved file");
  }

  moved_files
}

fn bench_create_workspace(c: &mut Criterion) {
  let mut group = c.benchmark_group("create_workspace");
  group.sample_size(20);

  group.bench_function("default", |b| {
    b.iter_batched_ref(
      || TestRepo::new().expect("failed to init test repo"),
      |repo| {
        treq_lib::core::create_workspace(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box("feat/bench"),
          std::hint::black_box(Some("bench workspace".to_string())),
          std::hint::black_box(None),
          std::hint::black_box(None),
          std::hint::black_box(None),
          None,
        )
        .expect("create_workspace failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

fn bench_create_workspace_with_moved_files(c: &mut Criterion) {
  let mut group = c.benchmark_group("create_workspace_with_moved_files");
  group.sample_size(20);

  group.bench_function("5_files", |b| {
    b.iter_batched_ref(
      || {
        let repo = TestRepo::new().expect("failed to init test repo");
        let moved_files = create_moved_files(&repo);
        (repo, moved_files)
      },
      |repo| {
        let (repo, moved_files) = repo;
        treq_lib::core::create_workspace(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box("feat/bench"),
          std::hint::black_box(Some("bench workspace".to_string())),
          std::hint::black_box(Some(moved_files.clone())),
          std::hint::black_box(None),
          std::hint::black_box(None),
          None,
        )
        .expect("create_workspace failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(
  benches,
  bench_create_workspace,
  bench_create_workspace_with_moved_files
);
criterion_main!(benches);
