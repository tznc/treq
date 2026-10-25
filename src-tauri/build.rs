fn main() {
  #[cfg(target_os = "macos")]
  println!("cargo:rustc-link-arg-bin=treq=-Wl,-rpath,@executable_path/../Frameworks");

  // GPUI embeds its own Windows app manifest (gpui-pre's default
  // `windows-manifest` feature, which gpui-kit turns on); a second one from
  // tauri-build fails the link with a duplicate MANIFEST resource. GPUI's
  // manifest already declares Common-Controls v6, the only thing Tauri's adds.
  #[cfg(feature = "gpui-shell")]
  if let Err(error) = tauri_build::try_build(
    tauri_build::Attributes::new()
      .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
  ) {
    panic!("tauri-build failed: {error:#}");
  }
  #[cfg(all(feature = "gui", not(feature = "gpui-shell")))]
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
