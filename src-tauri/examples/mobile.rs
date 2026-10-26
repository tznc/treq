//! The mobile app in a desktop window, for quick UI iteration:
//! `cargo run --example mobile --features mobile-ui`.

fn main() {
  treq_lib::mobile::run(tauri::generate_context!());
}
