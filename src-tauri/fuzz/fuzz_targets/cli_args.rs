#![no_main]

// `args.rs` names `crate::result_ext` and `super::handle_cli_command`; the
// stubs here stand in for them so the parser builds without the app.
#[allow(dead_code)]
mod result_ext {
  pub trait ResultExt<E> {
    fn best_effort(self);
  }
  impl<T, E> ResultExt<E> for Result<T, E> {
    fn best_effort(self) {}
  }
}

#[path = "../../src/cli/args.rs"]
#[allow(dead_code)]
mod args;

fn handle_cli_command(_: &args::SubcommandMatches) -> Option<i32> {
  None
}

#[path = "../../src/notify_dispatch.rs"]
#[allow(dead_code)]
mod notify_dispatch;

use libfuzzer_sys::fuzz_target;

// Input: argv tokens separated by NUL bytes. The same bytes also go to the
// notify request JSON parser and the message normalizer `treq notify` uses.
fuzz_target!(|data: &[u8]| {
  let argv = std::iter::once(std::ffi::OsString::from("treq")).chain(
    data
      .split(|b| *b == 0)
      .map(|arg| std::ffi::OsString::from(String::from_utf8_lossy(arg).into_owned())),
  );
  let _ = args::parse(argv);
  if let Ok(text) = std::str::from_utf8(data) {
    let _ = serde_json::from_str::<notify_dispatch::NotifyDispatchRequest>(text);
    if let Some(line) = notify_dispatch::normalize_message(text) {
      assert!(line.chars().count() <= notify_dispatch::MAX_MESSAGE_CHARS);
    }
  }
});
