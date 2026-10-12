/// Only http://localhost*, http://127.0.0.1* and file:// URLs are supported
/// by the in-app browser — mirrors `is_allowed_browser_url` in
/// `src-tauri/src/core/browser_url.rs`, checked client-side too for
/// instant feedback before round-tripping to Rust.
export function isAllowedBrowserUrl(candidate: string): boolean {
  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return false;
  }
  if (url.protocol === "file:") return true;
  if (url.protocol === "http:") {
    return url.hostname === "localhost" || url.hostname === "127.0.0.1";
  }
  return false;
}
