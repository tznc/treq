//! What the sidebar and GitHub panel pay to read the PR poller's cache, with
//! 10 and 100 branches whose PRs carry a 2 KiB description each.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use std::sync::Arc;
use treq_lib::{
  github::PrInfo,
  pr_status::{BranchListFn, PrFetchFn, PrStatusManager},
};

const REPO: &str = "/tmp/bench-repo";

fn manager(count: usize) -> PrStatusManager {
  let fetch: PrFetchFn = Arc::new(|_, branch| {
    Ok(Some(PrInfo {
      number: 1,
      title: format!("PR for {branch}"),
      state: "OPEN".into(),
      url: "https://github.com/o/r/pull/1".into(),
      head_ref_name: branch.into(),
      base_ref_name: "main".into(),
      merge_state_status: Some("CLEAN".into()),
      is_draft: false,
      body: "x".repeat(2048),
    }))
  });
  let branches: Vec<String> = (0..count).map(|i| format!("feat/{i}")).collect();
  let list: BranchListFn = Arc::new(move |_| Ok(branches.clone()));
  let mgr = PrStatusManager::new(fetch, list);
  mgr.refresh_repo_now(REPO);
  mgr
}

fn bench(c: &mut Criterion) {
  let mut group = c.benchmark_group("pr_cache");
  for count in [10, 100] {
    let mgr = manager(count);
    group.bench_function(format!("list_cached/{count}"), |b| {
      b.iter(|| black_box(mgr.list_cached(REPO)).len())
    });
    // The sidebar compares each poll's read with what it holds.
    let held = mgr.list_cached(REPO);
    group.bench_function(format!("read_and_compare/{count}"), |b| {
      b.iter(|| black_box(mgr.list_cached(REPO) == held))
    });
  }
  group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
