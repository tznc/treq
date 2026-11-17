#![expect(
  clippy::unwrap_used,
  clippy::expect_used,
  clippy::let_underscore_must_use,
  reason = "test code"
)]
//! What the Code tab reads from disk: a directory listing for the file
//! browser's tree, and the overview (the root's entries with their jj
//! status, and the README).

use criterion::{criterion_group, criterion_main, Criterion};
use std::fs;
use tempfile::TempDir;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

/// A directory of 5,000 files, as a large generated or vendored folder.
fn bench_list_directory(c: &mut Criterion) {
  let temp = TempDir::new().expect("tempdir");
  for index in 0..5000 {
    fs::write(temp.path().join(format!("file_{index}.rs")), "x\n").expect("write file");
  }
  let path = temp.path().to_str().expect("utf8 path");
  let mut group = c.benchmark_group("code_files");
  group.sample_size(20);
  group.bench_function("list_directory_5000", |b| {
    b.iter(|| treq_lib::core::list_directory(std::hint::black_box(path)).expect("list"))
  });
  group.finish();
}

/// The Code tab's overview of a repository of 2,000 tracked root files.
fn bench_overview(c: &mut Criterion) {
  let repo = TestRepo::new().expect("failed to init test repo");
  for index in 0..2000 {
    repo
      .create_file(&format!("file_{index}.rs"), "pub const VALUE: usize = 1;\n")
      .expect("failed to write tracked file");
  }
  repo
    .commit_file("README.md", "# Bench\n", "track 2,000 files")
    .expect("failed to commit tracked files");
  init_bench_app_db(&repo);
  let mut group = c.benchmark_group("code_files");
  group.sample_size(10);
  group.bench_function("overview_2000", |b| {
    b.iter(|| treq_lib::core::workspace_overview(&repo.repo_path, None).expect("overview"))
  });
  group.finish();
}

fn init_bench_app_db(repo: &TestRepo) {
  let app_dir = repo.temp_dir.path().join("app-data");
  fs::create_dir_all(&app_dir).expect("failed to create benchmark app dir");
  std::env::set_var("TREQ_APP_DATA_DIR", app_dir.to_string_lossy().to_string());
  let db =
    treq_lib::db::Database::new(app_dir.join("treq.db")).expect("failed to open benchmark app db");
  db.init().expect("failed to initialize benchmark app db");
}

criterion_group!(benches, bench_list_directory, bench_overview);
criterion_main!(benches);
