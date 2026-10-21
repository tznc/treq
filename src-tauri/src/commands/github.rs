#[cfg(test)]
mod tests {
  use std::fs;
  use std::io::Write;
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::sync::Arc;
  use std::time::{Duration, Instant};
  use tempfile::TempDir;

  #[cfg(unix)]
  fn write_fake_gh(dir: &TempDir, script_body: &str) -> String {
    let path = dir.path().join("gh");
    let mut f = fs::File::create(&path).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    write!(f, "{script_body}").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.to_str().unwrap().to_string()
  }

  /// `gh_create_pr` must schedule the `gh` subprocess on the blocking pool
  /// and return a Future the caller can await — never run the network call
  /// synchronously on the invoking thread.
  #[test]
  #[cfg(unix)]
  fn gh_create_pr_runs_on_blocking_pool_not_caller_thread() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "sleep 0.2\necho 'https://github.com/owner/repo/pull/11'",
    );

    let started = Arc::new(AtomicBool::new(false));
    let started_flag = Arc::clone(&started);

    let join = std::thread::spawn(move || {
      tauri::async_runtime::block_on(async move {
        tauri::async_runtime::spawn_blocking(move || {
          started_flag.store(true, Ordering::SeqCst);
          crate::github::gh_create_pr_impl(
            &gh_path,
            "owner/repo",
            "T",
            "B",
            "main",
            "feat",
            false,
            "/usr/bin:/bin",
          )
        })
        .await
        .expect("join create_pr task")
      })
    });

    // The invoking path returns a join handle immediately; work happens on
    // the blocking pool. We must observe the subprocess start without the
    // caller thread being stuck inside `Command::output`.
    let deadline = Instant::now() + Duration::from_secs(2);
    while !started.load(Ordering::SeqCst) {
      assert!(
        Instant::now() < deadline,
        "gh_create_pr blocking task never started"
      );
      std::thread::sleep(Duration::from_millis(10));
    }

    let number = join.join().expect("worker thread").unwrap();
    assert_eq!(number, 11);
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_pr_command_returns_pr_number_via_spawn_blocking() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr create --repo owner/repo --title T --body-file - --base main --head feat" || exit 9
echo 'https://github.com/owner/repo/pull/42'"#,
    );

    // Exercise the same spawn_blocking join path the Tauri command uses,
    // without depending on the process-wide gh binary cache.
    let number = tauri::async_runtime::block_on(async move {
      tauri::async_runtime::spawn_blocking(move || {
        crate::github::gh_create_pr_impl(
          &gh_path,
          "owner/repo",
          "T",
          "B",
          "main",
          "feat",
          false,
          "/usr/bin:/bin",
        )
      })
      .await
      .expect("join")
    })
    .unwrap();

    assert_eq!(number, 42);
  }
}
