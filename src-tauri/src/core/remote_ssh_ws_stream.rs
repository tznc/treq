//! WebSocket byte stream for managed SSH through the `remote-ssh-relay`
//! Edge Function.
//!
//! Fly Sprites have no raw TCP ingress, so the client cannot dial sshd on a
//! managed VM. Instead it opens a WebSocket to the relay, which checks the
//! caller's Supabase session and forwards raw bytes to sshd through the
//! Sprites TCP proxy. [`WsByteStream`] turns that WebSocket back into a plain
//! byte stream so `russh::client::connect_stream` can run the same SSH
//! handshake, host-key pinning, and certificate auth it runs over TCP.
//!
//! Relay connections are short-lived by design: Supabase caps how long an
//! Edge Function worker lives (minutes, not hours), and the cap includes an
//! open WebSocket. A drop looks like any other dead connection to the pool,
//! which reconnects on the next command; PTY sessions run inside tmux on the
//! VM and reattach.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{Bytes, Error as WsError, Message};
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

/// Adapts a WebSocket carrying binary frames to `AsyncRead + AsyncWrite`.
/// Each write becomes one binary frame; reads return frame payloads in
/// order. A close frame or end of stream reads as EOF.
pub struct WsByteStream<S> {
  inner: WebSocketStream<S>,
  /// Unread remainder of the last binary frame.
  pending: Bytes,
  eof: bool,
}

impl<S> WsByteStream<S> {
  pub fn new(inner: WebSocketStream<S>) -> Self {
    Self {
      inner,
      pending: Bytes::new(),
      eof: false,
    }
  }
}

fn to_io_error(error: WsError) -> io::Error {
  match error {
    WsError::Io(error) => error,
    WsError::ConnectionClosed | WsError::AlreadyClosed => {
      io::Error::new(io::ErrorKind::BrokenPipe, "relay websocket closed")
    }
    other => io::Error::other(other.to_string()),
  }
}

impl<S> AsyncRead for WsByteStream<S>
where
  S: AsyncRead + AsyncWrite + Unpin,
{
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    loop {
      if !self.pending.is_empty() {
        let n = self.pending.len().min(buf.remaining());
        let chunk = self.pending.split_to(n);
        buf.put_slice(&chunk);
        return Poll::Ready(Ok(()));
      }
      if self.eof {
        return Poll::Ready(Ok(()));
      }
      match Pin::new(&mut self.inner).poll_next(cx) {
        Poll::Pending => return Poll::Pending,
        Poll::Ready(None) | Poll::Ready(Some(Ok(Message::Close(_)))) => {
          self.eof = true;
        }
        Poll::Ready(Some(Ok(Message::Binary(data)))) => self.pending = data,
        // Pings are answered by tungstenite itself; nothing else carries
        // SSH bytes.
        Poll::Ready(Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)))) => {}
        Poll::Ready(Some(Ok(Message::Text(_)))) => {
          return Poll::Ready(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "relay sent a text frame on a binary SSH stream",
          )));
        }
        Poll::Ready(Some(Err(WsError::ConnectionClosed))) => self.eof = true,
        Poll::Ready(Some(Err(error))) => return Poll::Ready(Err(to_io_error(error))),
      }
    }
  }
}

impl<S> AsyncWrite for WsByteStream<S>
where
  S: AsyncRead + AsyncWrite + Unpin,
{
  fn poll_write(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &[u8],
  ) -> Poll<io::Result<usize>> {
    // `poll_ready` waits while tungstenite's write buffer is full, which is
    // what gives the SSH side backpressure against a slow relay.
    match Pin::new(&mut self.inner).poll_ready(cx) {
      Poll::Pending => return Poll::Pending,
      Poll::Ready(Err(error)) => return Poll::Ready(Err(to_io_error(error))),
      Poll::Ready(Ok(())) => {}
    }
    Pin::new(&mut self.inner)
      .start_send(Message::Binary(Bytes::copy_from_slice(buf)))
      .map_err(to_io_error)?;
    Poll::Ready(Ok(buf.len()))
  }

  fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    Pin::new(&mut self.inner)
      .poll_flush(cx)
      .map_err(to_io_error)
  }

  fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    match Pin::new(&mut self.inner).poll_close(cx) {
      Poll::Ready(Err(WsError::ConnectionClosed | WsError::AlreadyClosed)) => Poll::Ready(Ok(())),
      other => other.map_err(to_io_error),
    }
  }
}

#[expect(clippy::expect_used, reason = "ring supports the default TLS versions")]
fn tls_connector() -> Connector {
  // Built explicitly with ring so this does not depend on which rustls
  // crypto provider happens to be installed process-wide.
  let roots = rustls::RootCertStore {
    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
  };
  let config =
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
      .with_safe_default_protocol_versions()
      .expect("ring supports the default TLS versions")
      .with_root_certificates(roots)
      .with_no_client_auth();
  Connector::Rustls(Arc::new(config))
}

/// Opens the relay WebSocket for `url`, authenticating with the user's
/// Supabase session token in the `Authorization` header so it never appears
/// in a URL or log line. Errors never include the token.
pub async fn connect_relay(
  url: &str,
  access_token: &str,
) -> Result<WsByteStream<MaybeTlsStream<tokio::net::TcpStream>>, String> {
  let mut request = url
    .into_client_request()
    .map_err(|error| format!("invalid relay url: {error}"))?;
  let mut authorization = HeaderValue::from_str(&format!("Bearer {access_token}"))
    .map_err(|_| "relay access token is not a valid header value".to_string())?;
  authorization.set_sensitive(true);
  request.headers_mut().insert("authorization", authorization);

  let connector = if url.starts_with("wss:") {
    Some(tls_connector())
  } else {
    Some(Connector::Plain)
  };
  let (socket, _response) =
    tokio_tungstenite::connect_async_tls_with_config(request, None, true, connector)
      .await
      .map_err(|error| match error {
        WsError::Http(response) => {
          format!("relay refused the connection: HTTP {}", response.status())
        }
        other => format!("relay connection failed: {other}"),
      })?;
  Ok(WsByteStream::new(socket))
}

#[cfg(test)]
mod tests {
  use super::*;
  use futures::{SinkExt, StreamExt};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  async fn pair() -> (
    WsByteStream<MaybeTlsStream<tokio::net::TcpStream>>,
    WebSocketStream<tokio::net::TcpStream>,
    tokio::sync::oneshot::Receiver<Option<String>>,
  ) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (auth_tx, auth_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
      let (tcp, _) = listener.accept().await.unwrap();
      let mut auth_tx = Some(auth_tx);
      #[expect(
        clippy::result_large_err,
        reason = "the callback signature is fixed by tungstenite's handshake callback"
      )]
      let callback = |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                      response| {
        let auth = request
          .headers()
          .get("authorization")
          .and_then(|value| value.to_str().ok())
          .map(str::to_string);
        let _ = auth_tx.take().unwrap().send(auth);
        Ok(response)
      };
      tokio_tungstenite::accept_hdr_async(tcp, callback)
        .await
        .unwrap()
    });
    let client = connect_relay(&format!("ws://{addr}/relay?endpoint_id=e"), "jwt-123")
      .await
      .unwrap();
    (client, server.await.unwrap(), auth_rx)
  }

  #[tokio::test]
  async fn sends_token_in_authorization_header() {
    let (_client, _server, auth) = pair().await;
    assert_eq!(auth.await.unwrap().as_deref(), Some("Bearer jwt-123"));
  }

  #[tokio::test]
  async fn round_trips_bytes_across_frame_boundaries() {
    let (mut client, mut server, _) = pair().await;
    client.write_all(b"SSH-2.0-test\r\n").await.unwrap();
    client.flush().await.unwrap();
    let frame = server.next().await.unwrap().unwrap();
    assert_eq!(frame.into_data().as_ref(), b"SSH-2.0-test\r\n");

    // Two frames read through a buffer smaller than either.
    server
      .send(Message::Binary(Bytes::from_static(b"hello")))
      .await
      .unwrap();
    server
      .send(Message::Binary(Bytes::from_static(b"world")))
      .await
      .unwrap();
    let mut out = [0u8; 10];
    let mut read = 0;
    while read < out.len() {
      let mut small = [0u8; 3];
      let n = client.read(&mut small).await.unwrap();
      assert!(n > 0);
      out[read..read + n].copy_from_slice(&small[..n]);
      read += n;
    }
    assert_eq!(&out, b"helloworld");
  }

  #[tokio::test]
  async fn server_close_reads_as_eof() {
    let (mut client, mut server, _) = pair().await;
    server.close(None).await.unwrap();
    let mut buf = [0u8; 4];
    assert_eq!(client.read(&mut buf).await.unwrap(), 0);
  }

  #[tokio::test]
  async fn text_frames_are_rejected() {
    let (mut client, mut server, _) = pair().await;
    server.send(Message::Text("nope".into())).await.unwrap();
    let mut buf = [0u8; 4];
    let error = client.read(&mut buf).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
  }

  #[tokio::test]
  async fn http_refusal_does_not_leak_the_token() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
      let (mut tcp, _) = listener.accept().await.unwrap();
      let mut buf = [0u8; 2048];
      let _ = tcp.read(&mut buf).await;
      let _ = tcp
        .write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n")
        .await;
    });
    let Err(error) = connect_relay(&format!("ws://{addr}/relay"), "secret-jwt").await else {
      panic!("expected a refusal")
    };
    assert!(error.contains("401"), "{error}");
    assert!(!error.contains("secret-jwt"), "{error}");
  }
}
