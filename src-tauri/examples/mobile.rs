//! The mobile app in a desktop window, for quick UI iteration:
//! `cargo run --example mobile --features mobile-ui`.

#[expect(clippy::exit, reason = "`tauri::generate_context!` expands to an exit")]
fn main() {
  treq_lib::mobile::run(tauri::generate_context!());
}
