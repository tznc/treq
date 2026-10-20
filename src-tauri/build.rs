fn main() {
  #[cfg(target_os = "macos")]
  println!("cargo:rustc-link-arg-bin=treq=-Wl,-rpath,@executable_path/../Frameworks");

  #[cfg(feature = "gui")]
  tauri_build::build();
  // tauri-build sets the `desktop`/`mobile` cfgs; the headless CLI build only
  // targets desktop platforms.
  #[cfg(not(feature = "gui"))]
  {
    println!("cargo:rustc-check-cfg=cfg(desktop)");
    println!("cargo:rustc-check-cfg=cfg(mobile)");
    println!("cargo:rustc-cfg=desktop");
  }
}
