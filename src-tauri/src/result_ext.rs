/// Names why a `Result` is dropped, so `let _ =` never hides an error silently.
///
/// `clippy::let_underscore_must_use` rejects `let _ = result;`; each method
/// here consumes the `Result` and states the reason instead.
pub trait ResultExt<E> {
  /// The other side is gone (window, app, channel receiver, PTY), so there is
  /// nobody left to tell and nothing to retry.
  fn ignore_closed(self);

  /// Cleanup or an optional write whose failure changes nothing (temp file
  /// removal, process kill on teardown, rollback after an earlier error).
  fn best_effort(self);

  /// The operation is allowed to fail without failing the caller, but the
  /// failure is worth finding in the log. `what` names the attempted action.
  fn warn_on_err(self, what: &str)
  where
    E: std::fmt::Display;
}

impl<T, E> ResultExt<E> for Result<T, E> {
  fn ignore_closed(self) {}

  fn best_effort(self) {}

  fn warn_on_err(self, what: &str)
  where
    E: std::fmt::Display,
  {
    if let Err(error) = self {
      log::warn!("{what} failed: {error}");
    }
  }
}
