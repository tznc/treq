//! Sends one [`Request`] to a treq instance's IPC endpoint and reads its
//! [`Response`]: the same one-shot TCP exchange as `treq send`.

use super::protocol::{Request, Response};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);

/// `Ok` with the response's data, `Err` with its reason or the IO error.
pub fn call(endpoint: &str, request: &Request, io_timeout: Duration) -> Result<Value, String> {
  let addr = endpoint
    .parse()
    .map_err(|e| format!("invalid endpoint '{endpoint}': {e}"))?;
  let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
    .map_err(|e| format!("treq instance at {endpoint} is not reachable: {e}"))?;
  stream
    .set_read_timeout(Some(io_timeout))
    .and_then(|()| stream.set_write_timeout(Some(io_timeout)))
    .map_err(|e| format!("failed to set ipc timeout: {e}"))?;
  let payload = serde_json::to_vec(request).map_err(|e| format!("failed to encode request: {e}"))?;
  stream
    .write_all(&payload)
    .and_then(|()| stream.shutdown(Shutdown::Write))
    .map_err(|e| format!("failed to send request to {endpoint}: {e}"))?;
  let mut answer = String::new();
  stream
    .read_to_string(&mut answer)
    .map_err(|e| format!("no answer from {endpoint}: {e}"))?;
  let response: Response = serde_json::from_str(answer.trim())
    .map_err(|e| format!("invalid answer from {endpoint}: {e}"))?;
  match response.status.as_str() {
    "ok" => Ok(response.data),
    _ => Err(response.reason.unwrap_or_else(|| "request failed".into())),
  }
}

#[cfg(test)]
mod tests {
  use super::super::protocol::{Op, KIND};
  use super::*;
  use std::net::TcpListener;

  fn fake_instance(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let handle = std::thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let mut payload = String::new();
      stream.read_to_string(&mut payload).unwrap();
      stream.write_all(answer.as_bytes()).unwrap();
      payload
    });
    (endpoint, handle)
  }

  fn request() -> Request {
    Request {
      kind: KIND.into(),
      request_id: "r-1".into(),
      repo: "/src/app".into(),
      op: Op::List {
        repo: None,
        local_only: true,
      },
    }
  }

  #[test]
  fn sends_the_request_and_returns_the_data() {
    let (endpoint, server) = fake_instance(r#"{"status":"ok","data":{"agents":[]}}"#);
    let data = call(&endpoint, &request(), Duration::from_secs(2)).unwrap();
    assert_eq!(data["agents"], serde_json::json!([]));
    let sent: Value = serde_json::from_str(&server.join().unwrap()).unwrap();
    assert_eq!(sent["kind"], "agent_msg");
    assert_eq!(sent["op"], "list");
    assert_eq!(sent["local_only"], true);
  }

  #[test]
  fn an_error_response_is_its_reason() {
    let (endpoint, _server) = fake_instance(r#"{"status":"error","reason":"no agent called x"}"#);
    assert_eq!(
      call(&endpoint, &request(), Duration::from_secs(2)),
      Err("no agent called x".into())
    );
  }

  #[test]
  fn an_instance_that_is_not_there_is_an_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    drop(listener);
    assert!(call(&endpoint, &request(), Duration::from_secs(1)).is_err());
  }
}
