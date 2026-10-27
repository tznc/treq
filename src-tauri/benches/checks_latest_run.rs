#![expect(clippy::unwrap_used, reason = "test code")]
//! What the Checks popover reads per workflow: the newest run's id and
//! status, from a run with 10 or 50 jobs of 20 recorded steps each.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;
use treq_lib::{core::latest_workflow_run_sync, local_db};

fn setup(jobs: i64) -> (TempDir, String) {
  let dir = TempDir::new().unwrap();
  let repo = dir.path().to_string_lossy().to_string();
  local_db::init_local_db(&repo).unwrap();
  let steps: Vec<String> = (0..20)
    .map(|i| format!(r#"{{"name":"step {i}","status":"success","output":"{}"}}"#, "x".repeat(200)))
    .collect();
  let steps = format!("[{}]", steps.join(","));
  let run = local_db::create_workflow_run(&repo, 1, "ci.yaml").unwrap();
  for job in 0..jobs {
    local_db::add_workflow_job_result(&repo, run, &format!("job{job}"), job, "success", "t0", "t1", &steps, None)
      .unwrap();
  }
  local_db::finish_workflow_run(&repo, run, "success").unwrap();
  (dir, repo)
}

fn bench(c: &mut Criterion) {
  let mut group = c.benchmark_group("checks_latest_run");
  for jobs in [10, 50] {
    let (_dir, repo) = setup(jobs);
    group.bench_function(format!("latest/{jobs}_jobs"), |b| {
      b.iter(|| black_box(latest_workflow_run_sync(&repo, 1, "ci.yaml").unwrap()))
    });
  }
  group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
