use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::agent_dispatch;

pub const SEND_KIND: &str = "send";
pub const MEDIA_IMAGE: &str = "image";
pub const MEDIA_TEXT: &str = "text";
pub const MEDIA_BROWSER: &str = "browser";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SendDispatchRequest {
  pub kind: String,
  pub request_id: String,
  pub repo: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub pty_session_id: Option<String>,
  pub media_type: String,
  pub path: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
}

impl SendDispatchRequest {
  pub fn new(
    request_id: impl Into<String>,
    repo: impl Into<String>,
    media_type: impl Into<String>,
    path: impl Into<String>,
  ) -> Self {
    Self {
      kind: SEND_KIND.to_string(),
      request_id: request_id.into(),
      repo: repo.into(),
      pty_session_id: None,
      media_type: media_type.into(),
      path: path.into(),
      title: None,
    }
  }
}

/// Detect whether a path should be previewed as an image or as text.
pub fn detect_media_type_from_path(path: &Path) -> &'static str {
  match path
    .extension()
    .and_then(|ext| ext.to_str())
    .map(|ext| ext.to_ascii_lowercase())
    .as_deref()
  {
    Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg") => MEDIA_IMAGE,
    _ => MEDIA_TEXT,
  }
}

/// Sniff common image magic bytes. Used for piped stdin without a filename.
pub fn detect_media_type_from_bytes(bytes: &[u8]) -> &'static str {
  if bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']) {
    return MEDIA_IMAGE;
  }
  if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
    return MEDIA_IMAGE;
  }
  if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
    return MEDIA_IMAGE;
  }
  if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
    return MEDIA_IMAGE;
  }
  if bytes.starts_with(b"BM") {
    return MEDIA_IMAGE;
  }
  let trimmed = trim_utf8_bom(bytes);
  if trimmed.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg")) {
    return MEDIA_IMAGE;
  }
  MEDIA_TEXT
}

fn trim_utf8_bom(bytes: &[u8]) -> &[u8] {
  if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
    &bytes[3..]
  } else {
    bytes
  }
}

pub fn extension_for_media_type(media_type: &str, bytes: &[u8]) -> &'static str {
  if media_type == MEDIA_IMAGE {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']) {
      return "png";
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
      return "jpg";
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
      return "gif";
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
      return "webp";
    }
    if bytes.starts_with(b"BM") {
      return "bmp";
    }
    let trimmed = trim_utf8_bom(bytes);
    if trimmed.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg")) {
      return "svg";
    }
    return "bin";
  }
  "txt"
}

/// Resolve a durable on-disk path for `treq send` payloads.
/// Piped stdin is written under `<repo>/.treq/send/`.
pub fn resolve_send_path(
  repo_path: &str,
  path_arg: Option<&str>,
  stdin: &mut dyn Read,
  is_stdin_tty: bool,
) -> Result<(PathBuf, &'static str, String), String> {
  let use_stdin = match path_arg {
    Some("-") => true,
    None => !is_stdin_tty,
    Some(_) => false,
  };

  if use_stdin {
    let mut bytes = Vec::new();
    stdin
      .read_to_end(&mut bytes)
      .map_err(|e| format!("failed to read stdin: {}", e))?;
    if bytes.is_empty() {
      return Err("stdin is empty; pass a file path or pipe content".to_string());
    }
    let media_type = detect_media_type_from_bytes(&bytes);
    let ext = extension_for_media_type(media_type, &bytes);
    let dir = send_staging_dir(repo_path)?;
    let filename = format!("send-{}.{}", uuid::Uuid::new_v4(), ext);
    let out_path = dir.join(&filename);
    std::fs::write(&out_path, &bytes)
      .map_err(|e| format!("failed to write staged send file: {}", e))?;
    return Ok((out_path, media_type, filename));
  }

  match path_arg {
    Some(path_str) => {
      let path = PathBuf::from(path_str);
      if !path.exists() {
        return Err(format!("file not found: {}", path_str));
      }
      if !path.is_file() {
        return Err(format!("not a file: {}", path_str));
      }
      let abs = std::fs::canonicalize(&path)
        .map_err(|e| format!("failed to resolve path '{}': {}", path_str, e))?;
      let media_type = detect_media_type_from_path(&abs);
      let title = abs
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path_str.to_string());
      Ok((abs, media_type, title))
    }
    None => Err(
      "pass a file path, or pipe content via stdin (`echo hi | treq send` / `treq send -`)"
        .to_string(),
    ),
  }
}

/// Resolves a `treq send --browser` argument into an allowed browser URL.
/// A `http://localhost`, `http://127.0.0.1`, or `file://` URL is validated
/// and returned as-is; anything else is treated as a filesystem path to an
/// existing HTML file, which is canonicalized and converted to a `file://`
/// URL.
pub fn resolve_browser_send_target(path_arg: &str) -> Result<String, String> {
  let trimmed = path_arg.trim();
  if trimmed.is_empty() {
    return Err("path or URL is required".to_string());
  }

  let looks_like_url = trimmed.starts_with("http://")
    || trimmed.starts_with("https://")
    || trimmed.starts_with("file://");

  let url = if looks_like_url {
    trimmed.to_string()
  } else {
    let path = Path::new(trimmed);
    if !path.exists() {
      return Err(format!("file not found: {}", trimmed));
    }
    if !path.is_file() {
      return Err(format!("not a file: {}", trimmed));
    }
    let abs = std::fs::canonicalize(path)
      .map_err(|e| format!("failed to resolve path '{}': {}", trimmed, e))?;
    url::Url::from_file_path(&abs)
      .map(|u| u.to_string())
      .map_err(|_| format!("failed to build a file:// URL for '{}'", trimmed))?
  };

  if !crate::core::is_allowed_browser_url(&url) {
    return Err(format!(
      "'{}' is not an allowed browser URL (only http://localhost, http://127.0.0.1, and file:// are supported)",
      url
    ));
  }

  Ok(url)
}

pub fn send_staging_dir(repo_path: &str) -> Result<PathBuf, String> {
  let dir = Path::new(repo_path).join(".treq").join("send");
  std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create send staging dir: {}", e))?;
  Ok(dir)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SendArtifactRecord {
  pub id: String,
  pub repo: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub pty_session_id: Option<String>,
  pub media_type: String,
  pub path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  pub received_at: i64,
}

impl SendArtifactRecord {
  pub fn from_request(request: &SendDispatchRequest) -> Self {
    Self {
      id: request.request_id.clone(),
      repo: request.repo.clone(),
      pty_session_id: request.pty_session_id.clone(),
      media_type: request.media_type.clone(),
      path: request.path.clone(),
      title: request.title.clone(),
      received_at: received_at_from_request_id(&request.request_id),
    }
  }
}

fn received_at_from_request_id(request_id: &str) -> i64 {
  request_id
    .strip_prefix("send-")
    .and_then(|rest| rest.parse::<i64>().ok())
    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
}

/// Append a send to `<repo>/.treq/send/manifest.jsonl` as a history log.
pub fn record_send_artifact(request: &SendDispatchRequest) -> Result<(), String> {
  let dir = send_staging_dir(&request.repo)?;
  let manifest = dir.join("manifest.jsonl");
  let record = SendArtifactRecord::from_request(request);
  let mut line = serde_json::to_string(&record).map_err(|e| e.to_string())?;
  line.push('\n');
  use std::io::Write;
  let mut file = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(&manifest)
    .map_err(|e| format!("failed to open send manifest: {}", e))?;
  file
    .write_all(line.as_bytes())
    .map_err(|e| format!("failed to write send manifest: {}", e))?;
  Ok(())
}

fn sanitize_review_image_name(suggested_name: &str) -> String {
  let base = Path::new(suggested_name)
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("review.png");
  let stem = Path::new(base)
    .file_stem()
    .and_then(|n| n.to_str())
    .unwrap_or("review");
  let cleaned: String = stem
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
        c
      } else {
        '-'
      }
    })
    .collect();
  let cleaned = if cleaned.is_empty() {
    "review".to_string()
  } else {
    cleaned
  };
  format!("{cleaned}.png")
}

/// Persist a PNG (base64) under `<repo>/.treq/send/` for attachment review.
pub fn write_send_review_image(
  repo_path: &str,
  suggested_name: &str,
  contents_base64: &str,
) -> Result<PathBuf, String> {
  let bytes = decode_base64(contents_base64.trim())?;
  write_send_review_image_bytes(repo_path, suggested_name, &bytes)
}

/// [`write_send_review_image`] for a PNG already in memory.
pub fn write_send_review_image_bytes(
  repo_path: &str,
  suggested_name: &str,
  bytes: &[u8],
) -> Result<PathBuf, String> {
  if bytes.is_empty() {
    return Err("image data is empty".to_string());
  }
  let dir = send_staging_dir(repo_path)?;
  let filename = sanitize_review_image_name(suggested_name);
  let path = dir.join(filename);
  std::fs::write(&path, bytes).map_err(|e| format!("failed to write review image: {e}"))?;
  Ok(path)
}

/// Directory for clipboard images pasted into a terminal.
pub fn pasted_image_dir() -> PathBuf {
  std::env::temp_dir().join("treq-pasted-images")
}

/// Pasted images older than this are deleted when another is pasted, for
/// the ones whose session ended without the app closing it.
const PASTED_IMAGE_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

fn sweep_old_files(dir: &Path, max_age: std::time::Duration) {
  let Ok(entries) = std::fs::read_dir(dir) else {
    return;
  };
  for entry in entries.flatten() {
    let old = entry
      .metadata()
      .and_then(|m| m.modified())
      .ok()
      .and_then(|modified| modified.elapsed().ok())
      .is_some_and(|age| age > max_age);
    if old {
      std::fs::remove_file(entry.path()).ok();
    }
  }
}

/// Deletes the temporary files of a terminal session that ended: images
/// pasted into it and the agent files written for it. Anything else in
/// `paths` is left alone.
pub fn remove_session_files(paths: &[String]) {
  let temp = std::env::temp_dir();
  let pasted = pasted_image_dir();
  let mut agent_files = Vec::new();
  for path in paths {
    let path_ref = Path::new(path);
    if path_ref.parent() == Some(pasted.as_path()) {
      std::fs::remove_file(path_ref).ok();
    } else if path_ref.parent() == Some(temp.as_path())
      && path_ref
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("treq-agent-"))
    {
      agent_files.push(path.clone());
    }
  }
  if let Err(error) = crate::core::agent_cli::cleanup_agent_cli_files(&agent_files) {
    log::warn!("could not remove agent files: {error}");
  }
}

/// Save a clipboard image pasted into a terminal under `dir`, in a new file
/// named for its format, so the agent CLI can read it from its path.
pub fn write_pasted_image(dir: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
  let ext = extension_for_media_type(MEDIA_IMAGE, bytes);
  if ext == "bin" || ext == "svg" {
    return Err("pasted data is not a supported image".to_string());
  }
  std::fs::create_dir_all(dir).map_err(|e| format!("failed to create pasted image dir: {e}"))?;
  sweep_old_files(dir, PASTED_IMAGE_MAX_AGE);
  let path = dir.join(format!("paste-{}.{ext}", uuid::Uuid::new_v4()));
  std::fs::write(&path, bytes).map_err(|e| format!("failed to write pasted image: {e}"))?;
  Ok(path)
}

fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
  const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut lookup = [0xffu8; 256];
  for (i, b) in TABLE.iter().enumerate() {
    lookup[*b as usize] = i as u8;
  }
  let cleaned: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
  if !cleaned.len().is_multiple_of(4) {
    return Err("invalid image data".to_string());
  }
  let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
  for chunk in cleaned.as_chunks::<4>().0 {
    let pad = chunk.iter().filter(|b| **b == b'=').count();
    let mut vals = [0u8; 4];
    for i in 0..4 {
      if chunk[i] == b'=' {
        vals[i] = 0;
      } else {
        let v = lookup[chunk[i] as usize];
        if v == 0xff {
          return Err("invalid image data".to_string());
        }
        vals[i] = v;
      }
    }
    let n = (u32::from(vals[0]) << 18)
      | (u32::from(vals[1]) << 12)
      | (u32::from(vals[2]) << 6)
      | u32::from(vals[3]);
    out.push(((n >> 16) & 0xff) as u8);
    if pad < 2 {
      out.push(((n >> 8) & 0xff) as u8);
    }
    if pad < 1 {
      out.push((n & 0xff) as u8);
    }
  }
  Ok(out)
}

pub fn pty_session_id_from_env() -> Option<String> {
  std::env::var("TREQ_PTY_SESSION_ID")
    .ok()
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
}

pub fn parse_ipc_payload(payload: &str) -> Result<IpcDispatchMessage, String> {
  let value: serde_json::Value =
    serde_json::from_str(payload.trim()).map_err(|e| format!("invalid request json: {}", e))?;
  match value.get("kind").and_then(|k| k.as_str()) {
    Some(SEND_KIND) => {
      let request: SendDispatchRequest =
        serde_json::from_value(value).map_err(|e| format!("invalid send request json: {}", e))?;
      Ok(IpcDispatchMessage::Send(request))
    }
    Some(crate::notify_dispatch::NOTIFY_KIND) => {
      let request =
        serde_json::from_value(value).map_err(|e| format!("invalid notify request json: {}", e))?;
      Ok(IpcDispatchMessage::Notify(request))
    }
    _ => {
      let request: agent_dispatch::AgentDispatchRequest =
        serde_json::from_value(value).map_err(|e| format!("invalid agent request json: {}", e))?;
      Ok(IpcDispatchMessage::Agent(request))
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcDispatchMessage {
  Agent(agent_dispatch::AgentDispatchRequest),
  Send(SendDispatchRequest),
  Notify(crate::notify_dispatch::NotifyDispatchRequest),
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Cursor;

  #[test]
  fn detects_image_extensions() {
    assert_eq!(
      detect_media_type_from_path(Path::new("/tmp/shot.PNG")),
      MEDIA_IMAGE
    );
    assert_eq!(
      detect_media_type_from_path(Path::new("notes.md")),
      MEDIA_TEXT
    );
  }

  #[test]
  fn sniffs_png_magic_bytes() {
    let png = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 1, 2];
    assert_eq!(detect_media_type_from_bytes(&png), MEDIA_IMAGE);
    assert_eq!(extension_for_media_type(MEDIA_IMAGE, &png), "png");
  }

  #[test]
  fn sniffs_svg_markup_as_image() {
    let svg = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    assert_eq!(detect_media_type_from_bytes(svg), MEDIA_IMAGE);
    assert_eq!(extension_for_media_type(MEDIA_IMAGE, svg), "svg");
  }

  #[test]
  fn treats_plain_bytes_as_text() {
    assert_eq!(detect_media_type_from_bytes(b"hello world"), MEDIA_TEXT);
    assert_eq!(extension_for_media_type(MEDIA_TEXT, b"hello world"), "txt");
  }

  #[test]
  fn stages_stdin_text_under_repo_treq_send() {
    let temp = tempfile::TempDir::new().expect("temp");
    let repo = temp.path().to_string_lossy().to_string();
    let mut stdin = Cursor::new(b"preview me".to_vec());
    let (path, media, title) =
      resolve_send_path(&repo, Some("-"), &mut stdin, true).expect("resolve");
    assert_eq!(media, MEDIA_TEXT);
    assert!(path.starts_with(temp.path().join(".treq").join("send")));
    assert!(title.ends_with(".txt"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "preview me");
  }

  #[test]
  fn resolves_existing_file_path_without_copying() {
    let temp = tempfile::TempDir::new().expect("temp");
    let file = temp.path().join("photo.jpg");
    std::fs::write(&file, b"not-a-real-jpeg").expect("write");
    let mut empty = Cursor::new(Vec::new());
    let (path, media, title) = resolve_send_path(
      temp.path().to_str().unwrap(),
      Some(file.to_str().unwrap()),
      &mut empty,
      true,
    )
    .expect("resolve");
    assert_eq!(media, MEDIA_IMAGE);
    assert_eq!(title, "photo.jpg");
    assert_eq!(path, std::fs::canonicalize(&file).unwrap());
  }

  #[test]
  fn returns_error_for_missing_file() {
    let temp = tempfile::TempDir::new().expect("temp");
    let mut empty = Cursor::new(Vec::new());
    let err = resolve_send_path(
      temp.path().to_str().unwrap(),
      Some("/no/such/file.png"),
      &mut empty,
      true,
    )
    .expect_err("missing");
    assert!(err.contains("file not found"));
  }

  #[test]
  fn writes_review_png_under_treq_send() {
    let temp = tempfile::TempDir::new().expect("temp");
    let repo = temp.path().to_string_lossy().to_string();
    let path = write_send_review_image(&repo, "shot review.png", "iVBORw0KGgo=").expect("write");
    assert_eq!(
      path,
      temp
        .path()
        .join(".treq")
        .join("send")
        .join("shot-review.png")
    );
    assert_eq!(
      std::fs::read(&path).unwrap(),
      [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
  }

  #[test]
  fn writes_review_png_bytes() {
    let temp = tempfile::TempDir::new().expect("temp");
    let repo = temp.path().to_string_lossy().to_string();
    let path = write_send_review_image_bytes(&repo, "a b.png", b"png").expect("write");
    assert_eq!(path, temp.path().join(".treq").join("send").join("a-b.png"));
    assert_eq!(std::fs::read(&path).unwrap(), b"png");
    assert!(write_send_review_image_bytes(&repo, "a.png", b"").is_err());
  }

  #[test]
  fn rejects_empty_review_image() {
    let temp = tempfile::TempDir::new().expect("temp");
    let err =
      write_send_review_image(temp.path().to_str().unwrap(), "a.png", "").expect_err("empty");
    assert!(err.contains("invalid") || err.contains("empty"));
  }

  const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0";

  #[test]
  fn writes_pasted_png_under_dir_with_png_extension() {
    let temp = tempfile::TempDir::new().expect("temp");
    let dir = temp.path().join("pasted");
    let path = write_pasted_image(&dir, PNG).expect("write");
    assert_eq!(path.parent(), Some(dir.as_path()));
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("png"));
    assert_eq!(std::fs::read(&path).unwrap(), PNG);
  }

  #[test]
  fn removes_the_pasted_images_of_an_ended_session_and_nothing_else() {
    let dir = pasted_image_dir();
    let mine = write_pasted_image(&dir, PNG).expect("mine");
    let other = write_pasted_image(&dir, PNG).expect("other");
    let outside = tempfile::NamedTempFile::new().expect("outside");
    remove_session_files(&[
      mine.to_string_lossy().to_string(),
      outside.path().to_string_lossy().to_string(),
    ]);
    assert!(!mine.exists());
    assert!(other.exists());
    assert!(outside.path().exists());
    std::fs::remove_file(other).ok();
  }

  #[test]
  fn sweeps_pasted_images_left_by_old_sessions() {
    let temp = tempfile::TempDir::new().expect("temp");
    let old = write_pasted_image(temp.path(), PNG).expect("old");
    sweep_old_files(temp.path(), std::time::Duration::ZERO);
    assert!(!old.exists());
  }

  #[test]
  fn writes_each_pasted_image_to_a_new_file() {
    let temp = tempfile::TempDir::new().expect("temp");
    let first = write_pasted_image(temp.path(), PNG).expect("first");
    let second = write_pasted_image(temp.path(), PNG).expect("second");
    assert_ne!(first, second);
  }

  #[test]
  fn names_pasted_jpeg_with_jpg_extension() {
    let temp = tempfile::TempDir::new().expect("temp");
    let path = write_pasted_image(temp.path(), &[0xff, 0xd8, 0xff, 0xe0]).expect("write");
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("jpg"));
  }

  #[test]
  fn rejects_pasted_bytes_that_are_not_an_image() {
    let temp = tempfile::TempDir::new().expect("temp");
    let err = write_pasted_image(temp.path(), b"hello").expect_err("not an image");
    assert!(err.contains("image"));
  }

  #[test]
  fn resolve_browser_send_target_accepts_a_bare_localhost_url() {
    let url = resolve_browser_send_target("http://localhost:3000/app").expect("resolve");
    assert_eq!(url, "http://localhost:3000/app");
  }

  #[test]
  fn resolve_browser_send_target_converts_an_existing_html_file_to_a_file_url() {
    let temp = tempfile::TempDir::new().expect("temp");
    let file = temp.path().join("index.html");
    std::fs::write(&file, b"<html></html>").unwrap();

    let url = resolve_browser_send_target(file.to_str().unwrap()).expect("resolve");
    assert!(url.starts_with("file://"));
    assert!(url.ends_with("index.html"));
  }

  #[test]
  fn resolve_browser_send_target_rejects_a_disallowed_url() {
    let err = resolve_browser_send_target("https://example.com").unwrap_err();
    assert!(err.contains("not an allowed browser URL"));
  }

  #[test]
  fn resolve_browser_send_target_rejects_a_missing_file() {
    let err = resolve_browser_send_target("/no/such/file.html").unwrap_err();
    assert!(err.contains("file not found"));
  }

  #[test]
  fn resolve_browser_send_target_rejects_an_empty_argument() {
    let err = resolve_browser_send_target("  ").unwrap_err();
    assert!(err.contains("required"));
  }

  #[test]
  fn parse_ipc_routes_notify_requests() {
    use crate::notify_dispatch::{NotifyDispatchRequest, NotifyEvent, NOTIFY_KIND};
    let notify = NotifyDispatchRequest {
      kind: NOTIFY_KIND.to_string(),
      request_id: "notify-1".into(),
      repo: "/repo".into(),
      workspace: Some("feat/login".into()),
      event: NotifyEvent::Message,
      message: Some("Ready for review".into()),
    };
    let json = serde_json::to_string(&notify).unwrap();
    match parse_ipc_payload(&json).unwrap() {
      IpcDispatchMessage::Notify(req) => assert_eq!(req, notify),
      other => panic!("expected notify, got {:?}", other),
    }
  }

  #[test]
  fn parse_ipc_distinguishes_send_from_agent() {
    let send = SendDispatchRequest::new("r1", "/repo", MEDIA_TEXT, "/repo/.treq/send/a.txt");
    let send_json = serde_json::to_string(&send).unwrap();
    match parse_ipc_payload(&send_json).unwrap() {
      IpcDispatchMessage::Send(req) => {
        assert_eq!(req.request_id, "r1");
        assert_eq!(req.media_type, MEDIA_TEXT);
      }
      other => panic!("expected send, got {:?}", other),
    }

    let agent = agent_dispatch::AgentDispatchRequest {
      request_id: "a1".into(),
      repo: "/repo".into(),
      branch: "main".into(),
      prompt: "hi".into(),
      mode: "plan".into(),
      agent: "claude".into(),
    };
    let agent_json = serde_json::to_string(&agent).unwrap();
    match parse_ipc_payload(&agent_json).unwrap() {
      IpcDispatchMessage::Agent(req) => assert_eq!(req.request_id, "a1"),
      other => panic!("expected agent, got {:?}", other),
    }
  }
}
