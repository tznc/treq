//! The mobile app in a desktop window, for quick UI iteration:
//! `cargo run --example mobile --features mobile-ui`.

#[expect(
  clippy::exit,
  reason = "`generate_context!` expands to a `process::exit` on a bad config"
)]
fn main() {
  treq_lib::mobile::run(tauri::generate_context!());
}
