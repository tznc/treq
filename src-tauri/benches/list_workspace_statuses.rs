#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  reason = "test code"
)]
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::fs;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

fn write_workspace_file(repo: &TestRepo, workspace: &treq_lib::local_db::Workspace, name: &str) {
  let path = repo
    .workspaces_dir()
    .join(&workspace.workspace_path)
    .join(name);
  fs::write(path, "bench status content\n").expect("failed to write workspace file");
}

fn setup_five_flat_workspaces() -> TestRepo {
  let repo = TestRepo::new().expect("failed to init test repo");

  for index in 1..=5 {
    let workspace = treq_lib::core::create_workspace(
      &repo.repo_path,
      &format!("feat/flat-{index}"),
      Some(format!("flat workspace {index}")),
      None,
      None,
      None,
      None,
    )
    .expect("failed to create flat workspace");

    write_workspace_file(&repo, &workspace, &format!("flat-{index}.txt"));
  }

  repo
}

fn setup_five_hierarchies_three_levels() -> TestRepo {
  let repo = TestRepo::new().expect("failed to init test repo");

  for group in 1..=5 {
    let root = treq_lib::core::create_workspace(
      &repo.repo_path,
      &format!("feat/h{group}-l1"),
      Some(format!("hierarchy {group} level 1")),
      None,
      None,
      None,
      None,
    )
    .expect("failed to create hierarchy root");
    write_workspace_file(&repo, &root, &format!("h{group}-l1.txt"));

    let level2 = treq_lib::core::create_workspace(
      &repo.repo_path,
      &format!("feat/h{group}-l2"),
      None,
      None,
      Some(&root.branch_name),
      None,
      None,
    )
    .expect("failed to create hierarchy level 2");
    write_workspace_file(&repo, &level2, &format!("h{group}-l2.txt"));

    let level3 = treq_lib::core::create_workspace(
      &repo.repo_path,
      &format!("feat/h{group}-l3"),
      None,
      None,
      Some(&level2.branch_name),
      None,
      None,
    )
    .expect("failed to create hierarchy level 3");
    write_workspace_file(&repo, &level3, &format!("h{group}-l3.txt"));
  }

  repo
}

fn bench_list_workspace_statuses_flat(c: &mut Criterion) {
  let mut group = c.benchmark_group("list_workspace_statuses");
  group.sample_size(10);

  group.bench_function("5_flat_workspaces", |b| {
    b.iter_batched_ref(
      setup_five_flat_workspaces,
      |repo| {
        treq_lib::core::list_workspace_statuses(std::hint::black_box(&repo.repo_path))
          .expect("list_workspace_statuses failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

fn bench_list_workspace_statuses_hierarchy(c: &mut Criterion) {
  let mut group = c.benchmark_group("list_workspace_statuses_with_hierarchy");
  group.sample_size(10);

  group.bench_function("5_groups_3_levels", |b| {
    b.iter_batched_ref(
      setup_five_hierarchies_three_levels,
      |repo| {
        treq_lib::core::list_workspace_statuses(std::hint::black_box(&repo.repo_path))
          .expect("list_workspace_statuses failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(
  benches,
  bench_list_workspace_statuses_flat,
  bench_list_workspace_statuses_hierarchy
);
criterion_main!(benches);
