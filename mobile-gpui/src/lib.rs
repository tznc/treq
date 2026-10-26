//! The Treq mobile app crate. The app itself lives in `treq_lib::mobile`;
//! this crate holds the mobile entry point and the Tauri config the mobile
//! build reads.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  treq_lib::mobile::run(tauri::generate_context!());
}
