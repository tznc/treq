#![allow(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::string_slice,
  reason = "test code"
)]
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::fs;
use std::path::PathBuf;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

/// A workspace of 30 commits over 500 tracked files, with an uncommitted
/// edit: what the Commits tab lists when it opens.
fn setup_commits_log() -> (TestRepo, i64) {
  let repo = TestRepo::new().expect("failed to init test repo");
  for index in 0..500 {
    repo
      .create_file(
        &format!("src/module_{}/file_{index}.rs", index % 20),
        &format!("pub const VALUE: usize = {index};\n"),
      )
      .expect("failed to write tracked file");
  }
  repo
    .commit_file("README.md", "bench\n", "track 500 files")
    .expect("failed to commit tracked files");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat-bench-log",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("failed to create workspace");
  let dir = repo.workspaces_dir().join(&workspace.workspace_path);
  for index in 0..30 {
    fs::write(
      dir.join(format!("src/module_0/file_{index}.rs")),
      format!("pub const VALUE: usize = {};\n", index + 1000),
    )
    .expect("failed to edit workspace file");
    treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, &format!("Edit {index}"))
      .expect("failed to commit workspace edit");
  }
  fs::write(dir.join("src/module_1/file_1.rs"), "uncommitted\n")
    .expect("failed to write uncommitted edit");

  let app_dir: PathBuf = repo.temp_dir.path().join("app-data");
  fs::create_dir_all(&app_dir).expect("failed to create benchmark app dir");
  std::env::set_var("TREQ_APP_DATA_DIR", app_dir.to_string_lossy().to_string());
  let db =
    treq_lib::db::Database::new(app_dir.join("treq.db")).expect("failed to open benchmark app db");
  db.init().expect("failed to initialize benchmark app db");

  (repo, workspace.id)
}

fn list(repo: &TestRepo, workspace_id: i64) {
  treq_lib::core::list_commits(
    std::hint::black_box(&repo.repo_path),
    std::hint::black_box(Some(workspace_id)),
    true,
    None,
    None,
  )
  .expect("list_commits failed");
}

fn bench_commits_log(c: &mut Criterion) {
  let mut group = c.benchmark_group("commits_log");
  group.sample_size(20);

  // The first read caches each commit's diff stat.
  group.bench_function("30_commits_cold", |b| {
    // By reference, so deleting the temp repo stays out of the timing.
    b.iter_batched_ref(
      setup_commits_log,
      |(repo, workspace_id)| list(repo, *workspace_id),
      BatchSize::PerIteration,
    );
  });

  let (repo, workspace_id) = setup_commits_log();
  list(&repo, workspace_id);
  group.bench_function("30_commits_warm", |b| {
    b.iter(|| list(&repo, workspace_id));
  });

  group.finish();
}

criterion_group!(benches, bench_commits_log);
criterion_main!(benches);
