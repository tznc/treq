//! What the PR poller pays every 30 s per repo to tell whether a poll found
//! anything new: a digest of every branch's PR and CI rollup.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use treq_lib::{
  github::{PrCheckEntry, PrCiStatus, PrInfo},
  pr_status::{statuses_digest, RepoCiCache, RepoPrCache},
};

fn caches(count: usize) -> (RepoPrCache, RepoCiCache) {
  let mut prs = RepoPrCache::new();
  let mut ci = RepoCiCache::new();
  for i in 0..count {
    let branch = format!("feat/{i}");
    prs.insert(
      branch.clone(),
      Some(PrInfo {
        number: i as u64,
        title: format!("PR {i}"),
        state: "OPEN".into(),
        url: format!("https://github.com/o/r/pull/{i}"),
        head_ref_name: branch.clone(),
        base_ref_name: "main".into(),
        merge_state_status: Some("CLEAN".into()),
        is_draft: false,
        body: "x".repeat(2048),
      }),
    );
    let checks = (0..8)
      .map(|c| PrCheckEntry {
        name: format!("check {c}"),
        bucket: "pass".into(),
        link: format!("https://github.com/o/r/actions/runs/{c}"),
        duration_secs: Some(60),
      })
      .collect();
    ci.insert(
      branch,
      Some(PrCiStatus {
        state: "success".into(),
        total: 8,
        passed: 8,
        failed: 0,
        pending: 0,
        checks,
      }),
    );
  }
  (prs, ci)
}

fn bench(c: &mut Criterion) {
  let mut group = c.benchmark_group("pr_status_digest");
  for count in [10, 100] {
    let (prs, ci) = caches(count);
    group.bench_function(format!("digest/{count}"), |b| {
      b.iter(|| black_box(statuses_digest(&prs, &ci)))
    });
  }
  group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
