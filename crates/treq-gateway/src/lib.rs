//! treq-gateway: a WebSocket server on loopback that speaks Treq's gateway
//! proxy protocol (`supabase/functions/_shared/remote/gateway-proxy.ts`), so
//! the Treq SSH relay reaches sshd the same way on every provider (docs/design/sandbox-provider-adapter.md, D4 and §8).
//!
//! Protocol, per connection:
//! 1. The upgrade request carries `x-treq-gateway-token` (see [`token`]).
//! 2. The first text frame is `{"host": "...", "port": N}`.
//! 3. The gateway replies `{"status":"connected"}` once the TCP connection to
//!    an allowed target is open, or `{"status":"error","message":"..."}`.
//! 4. Binary frames then carry raw TCP bytes both ways.

pub mod token;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;

pub const TOKEN_HEADER: &str = "x-treq-gateway-token";
pub const DEFAULT_LISTEN_PORT: u16 = 8022;
pub const DEFAULT_SSHD_PORT: u16 = 2222;

#[derive(Clone, Debug)]
pub struct Config {
  pub placement_id: String,
  pub secret: Vec<u8>,
  /// Targets a client may ask for, as (host, port).
  pub allow: Vec<(String, u16)>,
  pub max_ttl_secs: u64,
}

impl Config {
  /// The default allowlist: the managed sshd on loopback.
  pub fn new(placement_id: String, secret: Vec<u8>) -> Self {
    Self {
      placement_id,
      secret,
      allow: vec![
        ("127.0.0.1".into(), DEFAULT_SSHD_PORT),
        ("localhost".into(), DEFAULT_SSHD_PORT),
      ],
      max_ttl_secs: token::MAX_TTL_SECS,
    }
  }

  fn allows(&self, host: &str, port: u16) -> bool {
    self.allow.iter().any(|(h, p)| h == host && *p == port)
  }
}

#[derive(Deserialize)]
struct ConnectRequest {
  host: String,
  port: u16,
}

fn now_secs() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0)
}

fn error_reply(message: &str) -> Message {
  Message::Text(
    serde_json::json!({ "status": "error", "message": message })
      .to_string()
      .into(),
  )
}

/// Accepts connections forever.
pub async fn serve(listener: TcpListener, config: Arc<Config>) -> std::io::Result<()> {
  loop {
    let (stream, _) = listener.accept().await?;
    let config = config.clone();
    tokio::spawn(async move {
      let _ = handle(stream, &config).await;
    });
  }
}

async fn handle(stream: TcpStream, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
  // tungstenite's handshake callback fixes this error type.
  #[allow(clippy::result_large_err)]
  let check = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
    let token = req
      .headers()
      .get(TOKEN_HEADER)
      .and_then(|v| v.to_str().ok())
      .unwrap_or("");
    match token::verify(
      &config.secret,
      &config.placement_id,
      token,
      now_secs(),
      config.max_ttl_secs,
    ) {
      Ok(()) => Ok(resp),
      Err(_) => {
        let mut deny = ErrorResponse::new(Some("invalid gateway token".into()));
        *deny.status_mut() = StatusCode::UNAUTHORIZED;
        Err(deny)
      }
    }
  };
  let mut ws = tokio_tungstenite::accept_hdr_async(stream, check).await?;

  let first = match ws.next().await {
    Some(Ok(Message::Text(text))) => text,
    _ => {
      ws.send(error_reply("expected a connect request")).await?;
      return Ok(());
    }
  };
  let req: ConnectRequest = match serde_json::from_str(first.as_str()) {
    Ok(req) => req,
    Err(_) => {
      ws.send(error_reply("unreadable connect request")).await?;
      return Ok(());
    }
  };
  if !config.allows(&req.host, req.port) {
    ws.send(error_reply("target not allowed")).await?;
    ws.close(None).await.ok();
    return Ok(());
  }
  let tcp = match TcpStream::connect((req.host.as_str(), req.port)).await {
    Ok(tcp) => tcp,
    Err(_) => {
      ws.send(error_reply("could not reach target")).await?;
      ws.close(None).await.ok();
      return Ok(());
    }
  };
  ws.send(Message::Text(r#"{"status":"connected"}"#.into()))
    .await?;

  let (mut tcp_read, mut tcp_write) = tcp.into_split();
  let (mut ws_tx, mut ws_rx) = ws.split();

  let up = async {
    while let Some(msg) = ws_rx.next().await {
      match msg {
        Ok(Message::Binary(bytes)) => {
          if tcp_write.write_all(&bytes).await.is_err() {
            break;
          }
        }
        Ok(Message::Text(text)) => {
          if tcp_write.write_all(text.as_bytes()).await.is_err() {
            break;
          }
        }
        Ok(Message::Close(_)) | Err(_) => break,
        Ok(_) => {}
      }
    }
    let _ = tcp_write.shutdown().await;
  };
  let down = async {
    let mut buf = vec![0u8; 32 * 1024];
    loop {
      match tcp_read.read(&mut buf).await {
        Ok(0) | Err(_) => break,
        Ok(n) => {
          if ws_tx
            .send(Message::Binary(buf[..n].to_vec().into()))
            .await
            .is_err()
          {
            break;
          }
        }
      }
    }
    let _ = ws_tx.close().await;
  };
  tokio::select! {
      _ = up => {}
      _ = down => {}
  }
  Ok(())
}
