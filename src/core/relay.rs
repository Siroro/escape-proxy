//! TCP byte relay between the SOCKS5 client and the destination.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

/// Wraps a stream and counts every byte read through it. Bytes are counted on
/// the read side so each byte is accounted exactly once while the session is
/// still running.
pub struct CountingStream<T> {
    inner: T,
    counter: Arc<AtomicU64>,
}

impl<T> CountingStream<T> {
    pub fn new(inner: T, counter: Arc<AtomicU64>) -> Self {
        Self { inner, counter }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for CountingStream<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let filled_before = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let n = buf.filled().len() - filled_before;
                if n > 0 {
                    self.counter.fetch_add(n as u64, Ordering::Relaxed);
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for CountingStream<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Copies bytes in both directions until EOF or error, returning
/// `(client-to-remote bytes, remote-to-client bytes)`. Half-closes are
/// propagated by `copy_bidirectional`: a client FIN shuts down only the
/// upstream write side, and vice versa, so half-duplex protocols keep working.
pub async fn relay(
    client: &mut TcpStream,
    remote: &mut TcpStream,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
) -> io::Result<(u64, u64)> {
    let mut client = CountingStream::new(client, tx_bytes);
    let mut remote = CountingStream::new(remote, rx_bytes);
    tokio::io::copy_bidirectional(&mut client, &mut remote).await
}
