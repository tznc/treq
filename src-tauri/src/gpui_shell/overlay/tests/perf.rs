//! What the branch switcher costs per frame as the repository's bookmark
//! count grows (round 6 measurement):
//! `cargo test --features gpui-shell --lib overlay::tests::perf -- --ignored --nocapture --test-threads=1`

use std::time::Instant;

use super::*;

fn host(n: usize) -> FakeHost {
  let mut branches = vec![branch("main", true)];
  for i in 0..n {
    let name = if i % 3 == 0 {
      format!("feature/ABC-{i:04}-some-descriptive-slug@origin")
    } else {
      format!("feature/ABC-{i:04}-some-descriptive-slug")
    };
    branches.push(branch(&name, false));
  }
  FakeHost {
    branches,
    ..FakeHost::default()
  }
}

fn median_frame(h: &mut Harness, n: usize) -> std::time::Duration {
  let mut samples: Vec<_> = (0..n)
    .map(|_| {
      h.cx.run_until_parked();
      h.cx.update(|window, cx| {
        window.refresh();
        let started = Instant::now();
        window.draw(cx).clear(cx);
        started.elapsed()
      })
    })
    .collect();
  samples.sort();
  samples[n / 2]
}

#[gpui::test]
#[ignore = "benchmark"]
fn overlay_branches_perf(cx: &mut TestAppContext) {
  let count: usize = std::env::var("N")
    .ok()
    .and_then(|n| n.parse().ok())
    .unwrap_or(500);
  let mut h = Harness::new(cx, branch_json(), host(count));
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  h.cx.run_until_parked();
  let empty = median_frame(&mut h, 15);
  h.type_text("feat");
  let searched = median_frame(&mut h, 15);
  h.type_text("fxz9");
  let missed = median_frame(&mut h, 15);
  println!(
    "branches={count:>5}  frame: no search {empty:?}  \"feat\" {searched:?}  \"featfxz9\" {missed:?}  rows={}",
    h.shown().len()
  );
}
