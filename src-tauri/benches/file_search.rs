#![allow(clippy::expect_used, clippy::string_slice, reason = "test code")]

use criterion::{criterion_group, criterion_main, Criterion};
use tempfile::TempDir;
use treq_lib::local_db::{search_workspace_files, sync_workspace_files, CachedWorkspaceFile};

const FILE_COUNT: usize = 50_000;
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

fn bench_files() -> Vec<CachedWorkspaceFile> {
  (0..FILE_COUNT)
    .map(|index| {
      let directory = index / FILES_PER_DIR;
      let module = directory / DIRS_PER_MODULE;
      let sub = directory % DIRS_PER_MODULE;
      let stem = STEMS[index % STEMS.len()];
      let extension = ["rs", "ts", "tsx", "md"][(index / 10) % 4];
      bench_file(format!(
        "src/module_{module}/sub_{sub}/{stem}_{index}.{extension}"
      ))
    })
    .collect()
}

fn bench_file_search(c: &mut Criterion) {
  let temp = TempDir::new().expect("tempdir");
  let repo_path = temp.path().to_str().expect("utf8 path");
  sync_workspace_files(repo_path, None, bench_files()).expect("seed workspace files");

  let mut group = c.benchmark_group("file_search");
  group.sample_size(20);

  for (name, query) in [
    ("short_ma", "ma"),
    ("fuzzy_srcmodfile", "srcmodfile"),
    ("filename_file_123_rs", "file_123.rs"),
  ] {
    group.bench_function(name, |b| {
      b.iter(|| {
        search_workspace_files(repo_path, None, std::hint::black_box(query), 50)
          .expect("search_workspace_files failed")
      });
    });
  }

  group.finish();
}

criterion_group!(benches, bench_file_search);
criterion_main!(benches);
