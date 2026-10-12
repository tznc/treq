use url::Url;

/// Restricts the in-app browser to localhost/127.0.0.1 (any port) and local
/// static HTML files — the feature's explicit scope is previewing a dev server
/// or a static file, not general internet browsing.
pub fn is_allowed_browser_url(candidate: &str) -> bool {
  let Ok(url) = Url::parse(candidate) else {
    return false;
  };

  match url.scheme() {
    "file" => true,
    "http" => matches!(url.host_str(), Some("localhost") | Some("127.0.0.1")),
    _ => false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn allows_localhost_with_any_port() {
    assert!(is_allowed_browser_url("http://localhost:3000"));
    assert!(is_allowed_browser_url("http://localhost:5173/app?x=1"));
    assert!(is_allowed_browser_url("http://localhost/"));
  }

  #[test]
  fn allows_127_0_0_1_with_any_port() {
    assert!(is_allowed_browser_url("http://127.0.0.1:8080/"));
    assert!(is_allowed_browser_url("http://127.0.0.1/"));
  }

  #[test]
  fn allows_file_urls() {
    assert!(is_allowed_browser_url("file:///tmp/preview.html"));
    assert!(is_allowed_browser_url(
      "file:///home/user/project/index.html"
    ));
  }

  #[test]
  fn rejects_https_and_other_hosts() {
    assert!(!is_allowed_browser_url("https://example.com"));
    assert!(!is_allowed_browser_url("http://example.com"));
    assert!(!is_allowed_browser_url("http://192.168.1.5:3000"));
  }

  #[test]
  fn rejects_other_schemes() {
    assert!(!is_allowed_browser_url("ftp://localhost/file"));
    assert!(!is_allowed_browser_url("javascript:alert(1)"));
    assert!(!is_allowed_browser_url("data:text/html,hi"));
  }

  #[test]
  fn rejects_malformed_urls() {
    assert!(!is_allowed_browser_url("not a url"));
    assert!(!is_allowed_browser_url(""));
  }
}
