#![allow(
  clippy::unwrap_used,
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

fn write_workspace_file(
  repo: &TestRepo,
  ws: &treq_lib::local_db::Workspace,
  path: &str,
  content: &str,
) {
  let dir = repo.workspaces_dir().join(&ws.workspace_path);
  let full = dir.join(path);
  fs::create_dir_all(full.parent().expect("workspace file should have a parent"))
    .expect("failed to create parent directories");
  fs::write(full, content).expect("failed to write workspace file");
}

fn setup_workspace_diff() -> (TestRepo, i64) {
  let repo = TestRepo::new().expect("failed to init test repo");
  repo
    .commit_file("target.txt", "target content\n", "target baseline")
    .expect("failed to create target baseline");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat-bench-diff",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("failed to create workspace");

  write_workspace_file(&repo, &workspace, "f1.txt", "first committed file\n");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add f1")
    .expect("failed to commit first diff file");

  write_workspace_file(
    &repo,
    &workspace,
    "nested/f2.txt",
    "second committed file\n",
  );
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add f2")
    .expect("failed to commit second diff file");

  write_workspace_file(&repo, &workspace, "uncommitted.txt", "working copy only\n");

  init_bench_app_db(&repo);
  (repo, workspace.id)
}

fn fifty_added_lines() -> String {
  (1..=50)
    .map(|line| format!("added line {line}"))
    .collect::<Vec<_>>()
    .join("\n")
    + "\n"
}

fn setup_workspace_diff_ten_files_fifty_added_lines() -> (TestRepo, i64) {
  let repo = TestRepo::new().expect("failed to init test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat-bench-diff-10-files",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("failed to create workspace");

  let content = fifty_added_lines();
  for index in 1..=10 {
    write_workspace_file(&repo, &workspace, &format!("added-{index}.txt"), &content);
  }

  init_bench_app_db(&repo);
  (repo, workspace.id)
}

/// A repo of 2,000 tracked files with one edit in the workspace: the reload
/// walks the whole working copy to snapshot it.
fn setup_workspace_diff_2000_files() -> (TestRepo, i64) {
  let repo = TestRepo::new().expect("failed to init test repo");
  for index in 0..2000 {
    repo
      .create_file(
        &format!("src/module_{}/file_{index}.rs", index % 40),
        &format!("pub const VALUE: usize = {index};\n"),
      )
      .expect("failed to write tracked file");
  }
  repo
    .commit_file("README.md", "bench\n", "track 2,000 files")
    .expect("failed to commit tracked files");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat-bench-diff-2000-files",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("failed to create workspace");
  write_workspace_file(&repo, &workspace, "src/module_0/file_0.rs", "edited\n");

  init_bench_app_db(&repo);
  (repo, workspace.id)
}

fn init_bench_app_db(repo: &TestRepo) {
  let app_dir: PathBuf = repo.temp_dir.path().join("app-data");
  fs::create_dir_all(&app_dir).expect("failed to create benchmark app dir");
  std::env::set_var("TREQ_APP_DATA_DIR", app_dir.to_string_lossy().to_string());

  let db =
    treq_lib::db::Database::new(app_dir.join("treq.db")).expect("failed to open benchmark app db");
  db.init().expect("failed to initialize benchmark app db");
  db.set_setting("conflict_marker_style", "git")
    .expect("failed to set benchmark conflict style");
}

fn bench_workspace_diff(c: &mut Criterion) {
  let mut group = c.benchmark_group("workspace_diff");
  group.sample_size(20);

  group.bench_function("committed_changes", |b| {
    // By reference, so deleting the temp repo stays out of the timing.
    b.iter_batched_ref(
      setup_workspace_diff,
      |(repo, workspace_id)| {
        treq_lib::core::workspace_diff(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box(*workspace_id),
        )
        .expect("workspace_diff failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.bench_function("10_files_50_added_lines_each", |b| {
    // By reference, so deleting the temp repo stays out of the timing.
    b.iter_batched_ref(
      setup_workspace_diff_ten_files_fifty_added_lines,
      |(repo, workspace_id)| {
        treq_lib::core::workspace_diff(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box(*workspace_id),
        )
        .expect("workspace_diff failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.bench_function("repo_2000_files_one_edit", |b| {
    // By reference, so deleting the temp repo stays out of the timing.
    b.iter_batched_ref(
      setup_workspace_diff_2000_files,
      |(repo, workspace_id)| {
        treq_lib::core::workspace_diff(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box(*workspace_id),
        )
        .expect("workspace_diff failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(benches, bench_workspace_diff);
criterion_main!(benches);
