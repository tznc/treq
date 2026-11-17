#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  reason = "test code"
)]
//! Workspace operations in a repo that already has several workspaces, as a
//! real one does: each operation touches the others (reconcile, discovery).
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

const SIBLINGS: usize = 5;

fn repo_with_siblings(remote: bool) -> TestRepo {
  let repo = if remote {
    TestRepo::with_remote().expect("failed to init test repo")
  } else {
    TestRepo::new().expect("failed to init test repo")
  };
  for index in 0..SIBLINGS {
    repo
      .create_workspace_simple(&format!("feat/sibling-{index}"))
      .expect("failed to create sibling workspace");
  }
  repo
}

fn bench_delete(c: &mut Criterion) {
  let mut group = c.benchmark_group("workspace_ops_delete");
  group.sample_size(10);
  group.bench_function("with_5_siblings", |b| {
    b.iter_batched_ref(
      || {
        let repo = repo_with_siblings(false);
        let ws = repo.create_workspace_simple("feat/victim").unwrap();
        (repo, ws.id)
      },
      |(repo, id)| treq_lib::core::delete_workspace(&repo.repo_path, id).unwrap(),
      BatchSize::PerIteration,
    );
  });
  group.bench_function("archive_with_5_siblings", |b| {
    b.iter_batched_ref(
      || {
        let repo = repo_with_siblings(false);
        let ws = repo.create_workspace_simple("feat/victim").unwrap();
        (repo, ws.id)
      },
      |(repo, id)| {
        treq_lib::core::archive_workspace_removing_directory_later(&repo.repo_path, *id).unwrap()
      },
      BatchSize::PerIteration,
    );
  });
  group.finish();
}

fn bench_create(c: &mut Criterion) {
  let mut group = c.benchmark_group("workspace_ops_create");
  group.sample_size(10);
  group.bench_function("with_5_siblings", |b| {
    b.iter_batched_ref(
      || repo_with_siblings(false),
      |repo| {
        repo.create_workspace_simple("feat/new").unwrap();
      },
      BatchSize::PerIteration,
    );
  });
  group.bench_function("create_new_workspace_with_title", |b| {
    b.iter_batched_ref(
      || repo_with_siblings(false),
      |repo| {
        treq_lib::core::create_new_workspace(
          &repo.repo_path,
          "feat/new",
          None,
          treq_lib::core::WorkspaceMetadata {
            title: Some("A title".into()),
            ..Default::default()
          },
          None,
        )
        .unwrap();
      },
      BatchSize::PerIteration,
    );
  });
  group.bench_function("new_workspaces_run_3_titles", |b| {
    b.iter_batched_ref(
      || repo_with_siblings(true),
      |repo| {
        let rows = treq_lib::core::new_workspaces::run(
          &repo.repo_path,
          "Add the first thing\nAdd the second thing\n  Add the third thing\n",
          None,
        )
        .unwrap();
        assert!(rows.iter().all(|r| r.workspace_id.is_some()), "{rows:?}");
      },
      BatchSize::PerIteration,
    );
  });
  group.finish();
}

fn bench_sync_fetch_push(c: &mut Criterion) {
  let mut group = c.benchmark_group("workspace_ops_remote");
  group.sample_size(10);
  group.bench_function("sync_clean_5_siblings", |b| {
    b.iter_batched_ref(
      || repo_with_siblings(false),
      |repo| treq_lib::core::sync_workspaces(&repo.repo_path).unwrap(),
      BatchSize::PerIteration,
    );
  });
  group.bench_function("fetch_5_siblings", |b| {
    b.iter_batched_ref(
      || repo_with_siblings(true),
      |repo| {
        treq_lib::jj::jj_git_fetch(&repo.repo_path).unwrap();
      },
      BatchSize::PerIteration,
    );
  });
  group.bench_function("push_workspace_5_siblings", |b| {
    b.iter_batched_ref(
      || {
        let repo = repo_with_siblings(true);
        let ws = repo
          .create_workspace_with_commit("feat/push", "p.txt", "x", None)
          .unwrap();
        (repo, ws.id)
      },
      |(repo, id)| {
        treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(*id)).unwrap();
      },
      BatchSize::PerIteration,
    );
  });
  group.finish();
}

criterion_group!(benches, bench_delete, bench_create, bench_sync_fetch_push);
criterion_main!(benches);
