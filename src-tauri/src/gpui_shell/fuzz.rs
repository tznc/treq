//! Runs GPUI property tests: each proptest case on a fresh test app, as
//! `#[gpui::property_test]` would (its macro no longer expands with
//! proptest-macro 0.6, which wants `proptest_path` as a path).
//!
//! Cases are few by default so the lib tests stay fast. For longer runs:
//! `PROPTEST_CASES=2000 SEED=7 cargo test --features gpui-shell --lib fuzz`.
//! `SEED` picks both the generated cases and GPUI's scheduler order; a
//! failure prints the shrunk input.

use std::fmt::Debug;

use gpui::{
  proptest::{
    strategy::Strategy,
    test_runner::{Config, TestRunner},
  },
  TestAppContext, TestDispatcher,
};

fn config(default_cases: u32) -> Config {
  let cases = std::env::var("PROPTEST_CASES")
    .ok()
    .and_then(|c| c.parse().ok())
    .unwrap_or(default_cases);
  gpui::apply_seed_to_proptest_config(Config {
    cases,
    failure_persistence: None,
    max_shrink_iters: 256,
    ..Config::default()
  })
}

/// Checks `property` on `default_cases` values of `strategy` (or
/// `PROPTEST_CASES`), each on its own test app.
pub fn check<S>(
  name: &'static str,
  default_cases: u32,
  strategy: S,
  property: impl Fn(&mut TestAppContext, S::Value),
) where
  S: Strategy,
  S::Value: Debug + Clone,
{
  let mut runner = TestRunner::new(config(default_cases));
  let result = runner.run(&(gpui::seed_strategy(), strategy), |(seed, value)| {
    // As `gpui::run_test_once`; proptest catches the panics.
    let dispatcher = TestDispatcher::new(seed);
    let scheduler = dispatcher.scheduler().clone();
    let mut cx = TestAppContext::build(dispatcher.clone(), Some(name));
    property(&mut cx, value);
    dispatcher.run_until_parked();
    cx.executor().forbid_parking();
    cx.quit();
    dispatcher.run_until_parked();
    scheduler.end_test();
    Ok(())
  });
  if let Err(error) = result {
    panic!("{name}: {error}");
  }
}
