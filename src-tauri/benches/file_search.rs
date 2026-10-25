#![expect(clippy::expect_used, reason = "test code")]

use criterion::{criterion_group, criterion_main, Criterion};
use tempfile::TempDir;
use treq_lib::file_indexer::FileIndex;
use treq_lib::local_db::{search_workspace_files, sync_workspace_files, CachedWorkspaceFile};

const FILE_COUNTS: [usize; 3] = [20_000, 50_000, 100_000];
const FILES_PER_DIR: usize = 25;
const DIRS_PER_MODULE: usize = 20;
const STEMS: [&str; 10] = [
  "main", "manager", "utils", "file", "index", "helpers", "config", "service", "model", "types",
];

fn bench_file(relative_path: String) -> CachedWorkspaceFile {
  CachedWorkspaceFile {
    id: 0,
    workspace_id: None,
    file_path: format!("/work/repo/{relative_path}"),
    parent_path: relative_path
      .rsplit_once('/')
      .map(|(parent, _)| parent.to_string()),
    relative_path,
    is_directory: false,
    cached_at: "2026-08-26T00:00:00Z".to_string(),
    mtime: None,
  }
}

fn bench_paths(count: usize) -> Vec<String> {
  (0..count)
    .map(|index| {
      let directory = index / FILES_PER_DIR;
      let module = directory / DIRS_PER_MODULE;
      let sub = directory % DIRS_PER_MODULE;
      let stem = STEMS[index % STEMS.len()];
      let extension = ["rs", "ts", "tsx", "md"][(index / 10) % 4];
      format!("src/module_{module}/sub_{sub}/{stem}_{index}.{extension}")
    })
    .collect()
}

fn bench_file_search(c: &mut Criterion) {
  for count in FILE_COUNTS {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    let files: Vec<_> = bench_paths(count).into_iter().map(bench_file).collect();
    sync_workspace_files(repo_path, None, files).expect("seed workspace files");
    let index = FileIndex::new(bench_paths(count));

    let mut group = c.benchmark_group(format!("file_search_{count}"));
    group.sample_size(20);

    for (name, query) in [
      ("empty", ""),
      ("short_ma", "ma"),
      ("fuzzy_srcmodfile", "srcmodfile"),
      ("filename_file_123_rs", "file_123.rs"),
    ] {
      group.bench_function(format!("sqlite_{name}"), |b| {
        b.iter(|| {
          search_workspace_files(repo_path, None, std::hint::black_box(query), 50)
            .expect("search_workspace_files failed")
        });
      });
      group.bench_function(format!("memory_{name}"), |b| {
        b.iter(|| index.search(std::hint::black_box(query), 50));
      });
    }

    group.bench_function("index_build_memory", |b| {
      b.iter_batched(
        || bench_paths(count),
        FileIndex::new,
        criterion::BatchSize::LargeInput,
      );
    });
    group.bench_function("index_sync_sqlite", |b| {
      b.iter_batched(
        || {
          bench_paths(count)
            .into_iter()
            .map(bench_file)
            .collect::<Vec<_>>()
        },
        |files| sync_workspace_files(repo_path, None, files).expect("sync"),
        criterion::BatchSize::LargeInput,
      );
    });
    group.finish();
  }
}

criterion_group!(benches, bench_file_search);
criterion_main!(benches);
