use crate::result_ext::ResultExt;
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static BINARY_PATHS_CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();

fn select_supported_git(
  paths: Vec<String>,
  mut version: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
  paths.into_iter().find(|path| {
    let Some(output) = version(path) else {
      return false;
    };
    let Some(number) = output
      .strip_prefix("git version ")
      .and_then(|s| s.split_whitespace().next())
    else {
      return false;
    };
    let mut parts = number.split('.');
    let major = parts.next().and_then(|s| s.parse::<u32>().ok());
    let minor = parts.next().and_then(|s| s.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if (major, minor) >= (2, 41))
  })
}

/// jj-lib fetch needs Git >= 2.41. GUI launches may find Apple's older Git
/// before Homebrew, and the persisted binary cache may also point at that Git.
pub fn resolve_remote_git() -> Result<String, String> {
  // The lookup spawns `which -a git` plus `git --version` per candidate, and
  // every fetch and push asks. A failure is not cached so installing Git
  // takes effect without a restart.
  static RESOLVED: OnceLock<String> = OnceLock::new();
  if let Some(path) = RESOLVED.get() {
    return Ok(path.clone());
  }
  let path = find_remote_git()?;
  Ok(RESOLVED.get_or_init(|| path).clone())
}

fn find_remote_git() -> Result<String, String> {
  let mut paths = get_binary_path("git").into_iter().collect::<Vec<_>>();
  let finder = if cfg!(windows) { "where" } else { "which" };
  let mut command = Command::new(finder);
  if !cfg!(windows) {
    command.arg("-a");
  }
  if let Ok(output) = command.arg("git").env("PATH", get_extended_path()).output() {
    if output.status.success() {
      for path in String::from_utf8_lossy(&output.stdout).lines() {
        let path = path.trim().to_string();
        if !path.is_empty() && !paths.contains(&path) {
          paths.push(path);
        }
      }
    }
  }
  select_supported_git(paths, |path| {
    let output = Command::new(path).arg("--version").output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
  }).ok_or_else(|| "Push and sync require Git 2.41.0 or newer. Install or upgrade Git (on macOS: brew install git), then restart Treq.".to_string())
}

/// Use the configured shell's PATH on Unix, plus the running executable directory.
pub fn get_extended_path() -> String {
  // Once the login shell's PATH is known the result no longer depends on the
  // environment, so build it once.
  #[cfg(unix)]
  {
    static EXTENDED: OnceLock<String> = OnceLock::new();
    if let Some(path) = EXTENDED.get() {
      return path.clone();
    }
    let (path, from_shell) = build_extended_path();
    if from_shell {
      return EXTENDED.get_or_init(|| path).clone();
    }
    path
  }
  #[cfg(not(unix))]
  build_extended_path().0
}

/// The extended PATH, and whether it came from the login shell (stable) or
/// fell back to the inherited environment.
fn build_extended_path() -> (String, bool) {
  let current_path = env::var("PATH").unwrap_or_default();

  if cfg!(windows) {
    // Preserve the inherited Windows PATH and its native separator.
    let mut all_paths: Vec<String> = current_path
      .split(';')
      .filter(|p| !p.is_empty())
      .map(String::from)
      .collect();
    if let Ok(exe_path) = std::env::current_exe() {
      if let Some(exe_dir) = exe_path.parent() {
        let exe_dir_str = exe_dir.to_string_lossy().to_string();
        if !exe_dir_str.is_empty() && !all_paths.contains(&exe_dir_str) {
          all_paths.push(exe_dir_str);
        }
      }
    }
    return (all_paths.join(";"), false);
  }

  #[cfg(unix)]
  let (current_path, from_shell) = {
    static SHELL_PATH: OnceLock<Option<String>> = OnceLock::new();
    let shell_path = SHELL_PATH
      .get_or_init(|| {
        let shell = env::var_os("SHELL").filter(|shell| !shell.is_empty())?;
        let mut command = login_shell_command(&shell);
        shell_path_from_command(&mut command, std::time::Duration::from_secs(5))
      })
      .clone();
    let from_shell = shell_path.is_some();
    (shell_path.unwrap_or(current_path), from_shell)
  };
  #[cfg(not(unix))]
  let from_shell = false;

  let mut all_paths: Vec<String> = current_path
    .split(':')
    .filter(|p| !p.is_empty())
    .map(String::from)
    .collect();

  // Add the running treq binary directory so `treq` is available in PTY sessions.
  if let Ok(exe_path) = std::env::current_exe() {
    if let Some(exe_dir) = exe_path.parent() {
      let exe_dir_str = exe_dir.to_string_lossy().to_string();
      if !exe_dir_str.is_empty() && !all_paths.contains(&exe_dir_str) {
        all_paths.push(exe_dir_str);
      }
    }
  }

  (all_paths.join(":"), from_shell)
}

#[cfg(unix)]
fn login_shell_command(shell: &OsStr) -> Command {
  let mut command = Command::new(shell);
  // Interactive startup loads tools configured in .zshrc/.bashrc; login
  // startup also loads profile files, matching a new terminal window.
  command.args(["-ilc", "printf '\\000%s\\000' \"$PATH\""]);
  command
}

/// Delimit PATH with NULs so startup messages cannot be mistaken for it.
/// Bound both shell startup and stdout collection (including inherited pipes).
#[cfg(unix)]
fn shell_path_from_command(command: &mut Command, timeout: std::time::Duration) -> Option<String> {
  use std::io::Read;
  use std::process::Stdio;
  use std::time::{Duration, Instant};

  let mut child = command
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .ok()?;
  let stdout = child.stdout.take()?;
  let (sender, receiver) = std::sync::mpsc::channel();
  std::thread::spawn(move || {
    let mut bytes = Vec::new();
    let result = stdout
      .take(1024 * 1024)
      .read_to_end(&mut bytes)
      .map(|_| bytes);
    sender.send(result).ignore_closed();
  });
  let start = Instant::now();
  loop {
    match child.try_wait() {
      Ok(Some(status)) => {
        if !status.success() {
          return None;
        }
        break;
      }
      Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(10)),
      _ => {
        child.kill().best_effort();
        child.wait().best_effort();
        return None;
      }
    }
  }
  let bytes = receiver
    .recv_timeout(timeout.saturating_sub(start.elapsed()))
    .ok()?
    .ok()?;
  let path = bytes.split(|byte| *byte == 0).nth(1)?;
  let path = String::from_utf8(path.to_vec()).ok()?;
  (!path.is_empty()).then_some(path)
}

/// Get the directory containing the running treq executable
pub fn get_exe_dir() -> Option<String> {
  let exe_path = std::env::current_exe().ok()?;
  let exe_dir = exe_path.parent()?;
  let dir_str = exe_dir.to_string_lossy().to_string();
  if dir_str.is_empty() {
    None
  } else {
    Some(dir_str)
  }
}

/// Detect binary path on the extended PATH, as `which` (Unix) or `where`
/// (Windows) would, without starting a process.
pub fn detect_binary(name: &str) -> Option<String> {
  detect_binary_in(name, get_extended_path())
}

/// Finds `name` on `search_path` (a PATH-style list) and nowhere else, with
/// the same lookup as [`detect_binary`].
pub fn detect_binary_in(name: &str, search_path: impl AsRef<OsStr>) -> Option<String> {
  // The lookup `which` and `where` do, without a process: the first-run
  // checklist asks for six tools at once, and a process each costs tens of
  // milliseconds on Windows.
  executable_in(name, search_path.as_ref())
}

/// The first file called `name` in a directory of `search_path`, with one of
/// the `pathext` extensions (`.EXE;.CMD`) added unless `name` has one already.
/// Windows runs a file by its extension, so `git` finds `git.exe`.
#[cfg_attr(unix, expect(dead_code, reason = "the Unix lookup checks permissions"))]
fn executable_with_extensions(
  name: &str,
  search_path: &OsStr,
  pathext: &str,
  is_file: impl Fn(&Path) -> bool,
) -> Option<String> {
  if name.is_empty() {
    return None;
  }
  let extensions: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
  let has_extension = extensions.iter().any(|ext| {
    name.len() > ext.len()
      && name.as_bytes()[name.len() - ext.len()..].eq_ignore_ascii_case(ext.as_bytes())
  });
  let candidates = |base: &Path| -> Vec<PathBuf> {
    let name = base.as_os_str().to_string_lossy().into_owned();
    let mut candidates = Vec::new();
    if has_extension {
      candidates.push(PathBuf::from(&name));
    }
    candidates.extend(
      extensions
        .iter()
        .map(|ext| PathBuf::from(format!("{name}{ext}"))),
    );
    candidates
  };
  let found = if name.contains(['/', '\\']) {
    candidates(Path::new(name)).into_iter().find(|c| is_file(c))
  } else {
    env::split_paths(search_path)
      .filter(|dir| !dir.as_os_str().is_empty())
      .find_map(|dir| {
        candidates(&dir.join(name))
          .into_iter()
          .find(|candidate| is_file(candidate))
      })
  };
  found.map(|path| path.to_string_lossy().into_owned())
}

#[cfg(not(unix))]
fn executable_in(name: &str, search_path: &OsStr) -> Option<String> {
  let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
  executable_with_extensions(name, search_path, &pathext, Path::is_file)
}

/// The first executable file called `name` in a directory of `search_path`.
#[cfg(unix)]
fn executable_in(name: &str, search_path: &OsStr) -> Option<String> {
  use std::os::unix::fs::PermissionsExt;
  let is_executable = |path: &Path| {
    path
      .metadata()
      .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
  };
  if name.is_empty() {
    return None;
  }
  if name.contains('/') {
    return is_executable(Path::new(name)).then(|| name.to_string());
  }
  env::split_paths(search_path)
    .filter(|dir| !dir.as_os_str().is_empty())
    .map(|dir| dir.join(name))
    .find(|candidate| is_executable(candidate))
    .map(|path| path.to_string_lossy().into_owned())
}

/// [`detect_binary`] for binaries that are not in the startup cache, such as
/// `gh`. A hit is kept for the process; a miss for a minute, so installing the
/// tool takes effect soon without a `which` spawn on every poll.
pub fn detect_binary_cached(name: &str) -> Option<String> {
  const MISS_TTL: Duration = Duration::from_secs(60);
  type Lookups = HashMap<String, (Option<String>, Instant)>;
  static CACHE: OnceLock<Mutex<Lookups>> = OnceLock::new();
  let cache = CACHE.get_or_init(Mutex::default);
  if let Some((found, at)) = cache.lock().ok()?.get(name) {
    if found.is_some() || at.elapsed() < MISS_TTL {
      return found.clone();
    }
  }
  let found = detect_binary(name);
  cache
    .lock()
    .ok()?
    .insert(name.to_string(), (found.clone(), Instant::now()));
  found
}

/// Initialize binary paths cache with detected paths
pub fn init_binary_paths_cache(paths: HashMap<String, String>) {
  BINARY_PATHS_CACHE.set(paths).best_effort();
}

/// Get cached binary path for a given binary name
pub fn get_binary_path(name: &str) -> Option<String> {
  BINARY_PATHS_CACHE.get()?.get(name).cloned()
}

/// Detect installed editor applications using mdfind
pub fn detect_editor_app(app_name: &str) -> bool {
  let app_file = format!("/{}.app", app_name);
  installed_applications().is_some_and(|apps| apps.lines().any(|line| line.ends_with(&app_file)))
}

/// Every application Spotlight knows, asked once for all the editors. Only
/// macOS has `mdfind`; elsewhere there is no one to ask.
fn installed_applications() -> Option<&'static str> {
  static APPLICATIONS: OnceLock<Option<String>> = OnceLock::new();
  APPLICATIONS
    .get_or_init(|| {
      if !cfg!(target_os = "macos") {
        return None;
      }
      let output = Command::new("mdfind")
        .arg("kMDItemKind == 'Application'")
        .output()
        .ok()
        .filter(|output| output.status.success())?;
      Some(String::from_utf8_lossy(&output.stdout).into_owned())
    })
    .as_deref()
}

static EDITOR_APPS_CACHE: OnceLock<HashMap<String, bool>> = OnceLock::new();

/// Initialize editor apps cache
pub fn init_editor_apps_cache(apps: HashMap<String, bool>) {
  EDITOR_APPS_CACHE.set(apps).best_effort();
}

/// Get a cached editor-apps snapshot if initialized.
pub fn get_editor_apps_cache() -> Option<HashMap<String, bool>> {
  EDITOR_APPS_CACHE.get().cloned()
}

#[cfg(test)]
mod tests {
  #[test]
  fn finds_a_windows_tool_by_its_extension_without_a_process() {
    let dir = tempfile::TempDir::new().unwrap();
    let (first, second) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(second.join("git.exe"), "").unwrap();
    std::fs::write(second.join("npm.cmd"), "").unwrap();
    std::fs::write(first.join("git.cmd"), "").unwrap();
    let path = env::join_paths([&first, &second]).unwrap();
    let find =
      |name: &str| executable_with_extensions(name, &path, ".com;.exe;.cmd", Path::is_file);
    // The earlier directory wins, and the extension is added in PATHEXT order.
    assert_eq!(
      find("git"),
      Some(first.join("git.cmd").to_string_lossy().into_owned())
    );
    assert_eq!(
      find("npm"),
      Some(second.join("npm.cmd").to_string_lossy().into_owned())
    );
    assert_eq!(
      find("git.exe"),
      Some(second.join("git.exe").to_string_lossy().into_owned())
    );
    assert_eq!(find("node"), None);
    assert_eq!(find(""), None);
  }

  use super::*;

  #[cfg(unix)]
  #[test]
  fn finds_the_first_executable_on_the_search_path() {
    use std::os::unix::fs::PermissionsExt;
    let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let tool = |dir: &tempfile::TempDir, mode| {
      let path = dir.path().join("tool");
      std::fs::write(&path, "#!/bin/sh\n").unwrap();
      std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
      path
    };
    tool(&first, 0o644);
    let wanted = tool(&second, 0o755);
    let search = env::join_paths([first.path(), second.path()]).unwrap();
    assert_eq!(
      detect_binary_in("tool", &search),
      Some(wanted.to_string_lossy().into_owned()),
      "a file that is not executable does not shadow the one behind it"
    );
    assert_eq!(detect_binary_in("missing", &search), None);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn loads_login_and_interactive_shell_startup() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
      temp.path().join(".zprofile"),
      "export PATH='/custom/login/bin:/usr/bin:/bin'\n",
    )
    .unwrap();
    std::fs::write(
      temp.path().join(".zshrc"),
      "export PATH=\"/custom/interactive/bin:$PATH\"\nprintf 'startup message\\n'\n",
    )
    .unwrap();
    let mut shell = login_shell_command(OsStr::new("/bin/zsh"));
    shell.env("ZDOTDIR", temp.path());
    let path = shell_path_from_command(&mut shell, std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(
      path,
      "/custom/interactive/bin:/custom/login/bin:/usr/bin:/bin"
    );
  }

  #[cfg(unix)]
  #[test]
  fn discovers_and_executes_cli_from_shell_path() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("custom npm prefix/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let cli = bin.join("devcontainer");
    std::fs::write(&cli, "#!/bin/sh\nprintf 'custom-cli'").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let expected = format!("{}:/usr/bin:/bin", bin.display());
    let mut shell = Command::new("/bin/sh");
    shell.args([
      "-c",
      r#"printf 'startup noise\n\000%s\000' "$1"; printf 'trailing noise'"#,
      "sh",
      &expected,
    ]);
    let path = shell_path_from_command(&mut shell, std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(path, expected);
    assert_eq!(
      detect_binary_in("devcontainer", &path),
      Some(cli.to_string_lossy().into_owned())
    );
    let output = Command::new("devcontainer")
      .env("PATH", path)
      .output()
      .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"custom-cli");
  }

  #[cfg(unix)]
  #[test]
  fn rejects_failed_shell_path_lookup() {
    let mut shell = Command::new("/bin/sh");
    shell.args(["-c", "exit 1"]);
    assert_eq!(
      shell_path_from_command(&mut shell, std::time::Duration::from_secs(2)),
      None
    );
  }

  #[cfg(unix)]
  #[test]
  fn bounds_shell_path_lookup_time() {
    let mut shell = Command::new("/bin/sh");
    shell.args(["-c", "exec /bin/sleep 10"]);
    let start = std::time::Instant::now();
    assert_eq!(
      shell_path_from_command(&mut shell, std::time::Duration::from_millis(50)),
      None
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
  }

  #[test]
  fn selects_supported_git_after_old_cached_binary() {
    let paths = vec!["apple-git".to_string(), "brew-git".to_string()];
    let selected = select_supported_git(paths, |path| {
      Some(if path == "apple-git" {
        "git version 2.39.5 (Apple Git-154)".to_string()
      } else {
        "git version 2.47.1".to_string()
      })
    });
    assert_eq!(selected.as_deref(), Some("brew-git"));
  }

  #[test]
  fn rejects_old_or_unreadable_git_versions() {
    for version in ["git version 2.40.9", "", "not git"] {
      assert_eq!(
        select_supported_git(vec!["git".to_string()], |_| Some(version.to_string())),
        None
      );
    }
  }

  #[test]
  fn accepts_minimum_and_platform_git_versions() {
    for version in [
      "git version 2.41.0",
      "git version 2.46.0.windows.1",
      "git version 3.0.0",
    ] {
      assert_eq!(
        select_supported_git(vec!["git".to_string()], |_| Some(version.to_string())),
        Some("git".to_string())
      );
    }
  }
}
