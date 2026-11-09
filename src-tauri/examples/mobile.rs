//! The mobile app in a desktop window, for quick UI iteration:
//! `cargo run --example mobile --features mobile-ui`.

// `tauri::generate_context!` can expand to an exit, depending on the build
// environment, so this allows rather than expects it.
#[allow(clippy::exit)]
fn main() {
  treq_lib::mobile::run(tauri::generate_context!());
}
