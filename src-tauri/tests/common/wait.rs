// The one polling helper tests share; the `no-sleep-in-tests-rust` rule lets
// this file sleep. `common/mod.rs` includes it, so unit tests reach it as
// `crate::test_repo::wait_until` and integration tests as `common::wait_until`.

/// Calls `done` until it returns true or `timeout` passes, and returns
/// whether it did. A test waits on the condition it needs this way rather
/// than on a fixed delay: it goes on as soon as the condition holds, and a
/// slow machine only makes it wait longer.
#[expect(
  clippy::allow_attributes,
  reason = "dead in only some of the binaries that share this file"
)]
#[allow(dead_code, reason = "each test binary uses a subset of the fixtures")]
pub fn wait_until(timeout: std::time::Duration, mut done: impl FnMut() -> bool) -> bool {
  const POLL: std::time::Duration = std::time::Duration::from_millis(5);
  let deadline = std::time::Instant::now() + timeout;
  loop {
    if done() {
      return true;
    }
    if std::time::Instant::now() >= deadline {
      return false;
    }
    std::thread::sleep(POLL);
  }
}
