// End-to-end: a real gateway on an ephemeral loopback port, a real WebSocket
// client, and a local TCP echo server standing in for sshd.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use treq_gateway::{serve, token, Config, TOKEN_HEADER};

const PLACEMENT: &str = "11111111-2222-3333-4444-555555555555";
const SECRET: &[u8] = b"integration-secret";

fn now() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs()
}

async fn echo_server() -> u16 {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  tokio::spawn(async move {
    while let Ok((mut s, _)) = listener.accept().await {
      tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        while let Ok(n) = s.read(&mut buf).await {
          if n == 0 || s.write_all(&buf[..n]).await.is_err() {
            break;
          }
        }
      });
    }
  });
  port
}

async fn gateway(allow_port: u16) -> u16 {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  let mut config = Config::new(PLACEMENT.into(), SECRET.to_vec());
  config.allow = vec![("127.0.0.1".into(), allow_port)];
  tokio::spawn(serve(listener, Arc::new(config)));
  port
}

type Ws =
  tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(
  port: u16,
  token: Option<String>,
) -> Result<Ws, tokio_tungstenite::tungstenite::Error> {
  let mut req = format!("ws://127.0.0.1:{port}/")
    .into_client_request()
    .unwrap();
  if let Some(t) = token {
    req.headers_mut().insert(TOKEN_HEADER, t.parse().unwrap());
  }
  tokio_tungstenite::connect_async(req)
    .await
    .map(|(ws, _)| ws)
}

fn valid_token() -> String {
  token::mint(SECRET, PLACEMENT, now() + 60)
}

async fn reply(ws: &mut Ws) -> serde_json::Value {
  match ws.next().await.unwrap().unwrap() {
    Message::Text(t) => serde_json::from_str(t.as_str()).unwrap(),
    other => panic!("expected text reply, got {other:?}"),
  }
}

#[tokio::test]
async fn handshake_replies_connected_for_allowed_target() {
  let echo = echo_server().await;
  let gw = gateway(echo).await;
  let mut ws = connect(gw, Some(valid_token())).await.unwrap();
  ws.send(Message::Text(
    format!(r#"{{"host":"127.0.0.1","port":{echo}}}"#).into(),
  ))
  .await
  .unwrap();
  assert_eq!(reply(&mut ws).await["status"], "connected");
}

#[tokio::test]
async fn relays_bytes_to_and_from_tcp() {
  let echo = echo_server().await;
  let gw = gateway(echo).await;
  let mut ws = connect(gw, Some(valid_token())).await.unwrap();
  ws.send(Message::Text(
    format!(r#"{{"host":"127.0.0.1","port":{echo}}}"#).into(),
  ))
  .await
  .unwrap();
  reply(&mut ws).await;
  ws.send(Message::Binary(b"SSH-2.0-test\r\n".to_vec().into()))
    .await
    .unwrap();
  match ws.next().await.unwrap().unwrap() {
    Message::Binary(b) => assert_eq!(&b[..], b"SSH-2.0-test\r\n"),
    other => panic!("expected binary, got {other:?}"),
  }
}

#[tokio::test]
async fn rejects_upgrade_without_token() {
  let gw = gateway(1).await;
  assert!(connect(gw, None).await.is_err());
}

#[tokio::test]
async fn rejects_upgrade_with_expired_token() {
  let gw = gateway(1).await;
  let expired = token::mint(SECRET, PLACEMENT, now() - 1);
  assert!(connect(gw, Some(expired)).await.is_err());
}

#[tokio::test]
async fn rejects_upgrade_with_forged_token() {
  let gw = gateway(1).await;
  let forged = token::mint(b"not-the-secret", PLACEMENT, now() + 60);
  assert!(connect(gw, Some(forged)).await.is_err());
}

#[tokio::test]
async fn refuses_target_outside_allowlist() {
  let echo = echo_server().await;
  let gw = gateway(echo).await;
  let mut ws = connect(gw, Some(valid_token())).await.unwrap();
  ws.send(Message::Text(r#"{"host":"127.0.0.1","port":22}"#.into()))
    .await
    .unwrap();
  let r = reply(&mut ws).await;
  assert_eq!(r["status"], "error");
  assert_eq!(r["message"], "target not allowed");
}

#[test]
fn default_allowlist_is_loopback_sshd_only() {
  let c = Config::new(PLACEMENT.into(), SECRET.to_vec());
  assert_eq!(
    c.allow,
    vec![
      ("127.0.0.1".to_string(), 2222),
      ("localhost".to_string(), 2222)
    ]
  );
}
