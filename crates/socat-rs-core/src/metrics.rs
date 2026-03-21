use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::SocoreError;

static CONNECTIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static CONNECTIONS_FAILED_TOTAL: AtomicU64 = AtomicU64::new(0);
static BYTES_LEFT_TO_RIGHT_TOTAL: AtomicU64 = AtomicU64::new(0);
static BYTES_RIGHT_TO_LEFT_TOTAL: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct MetricsSnapshot {
    pub connections_total: u64,
    pub connections_failed_total: u64,
    pub bytes_left_to_right_total: u64,
    pub bytes_right_to_left_total: u64,
}

pub fn record_connection_start() {
    CONNECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_connection_failed() {
    CONNECTIONS_FAILED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_bytes(left_to_right: u64, right_to_left: u64) {
    BYTES_LEFT_TO_RIGHT_TOTAL.fetch_add(left_to_right, Ordering::Relaxed);
    BYTES_RIGHT_TO_LEFT_TOTAL.fetch_add(right_to_left, Ordering::Relaxed);
}

pub fn snapshot() -> MetricsSnapshot {
    MetricsSnapshot {
        connections_total: CONNECTIONS_TOTAL.load(Ordering::Relaxed),
        connections_failed_total: CONNECTIONS_FAILED_TOTAL.load(Ordering::Relaxed),
        bytes_left_to_right_total: BYTES_LEFT_TO_RIGHT_TOTAL.load(Ordering::Relaxed),
        bytes_right_to_left_total: BYTES_RIGHT_TO_LEFT_TOTAL.load(Ordering::Relaxed),
    }
}

fn render_prometheus(snapshot: MetricsSnapshot) -> String {
    format!(
        "\
# TYPE socat_rs_connections_total counter
socat_rs_connections_total {}
# TYPE socat_rs_connections_failed_total counter
socat_rs_connections_failed_total {}
# TYPE socat_rs_bytes_left_to_right_total counter
socat_rs_bytes_left_to_right_total {}
# TYPE socat_rs_bytes_right_to_left_total counter
socat_rs_bytes_right_to_left_total {}
",
        snapshot.connections_total,
        snapshot.connections_failed_total,
        snapshot.bytes_left_to_right_total,
        snapshot.bytes_right_to_left_total
    )
}

pub async fn serve_prometheus(bind: String) -> Result<(), SocoreError> {
    let listener = TcpListener::bind(bind).await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = render_prometheus(snapshot());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{MetricsSnapshot, render_prometheus};

    #[test]
    fn renders_prometheus_text() {
        let text = render_prometheus(MetricsSnapshot {
            connections_total: 2,
            connections_failed_total: 1,
            bytes_left_to_right_total: 10,
            bytes_right_to_left_total: 20,
        });
        assert!(text.contains("socat_rs_connections_total 2"));
        assert!(text.contains("socat_rs_connections_failed_total 1"));
        assert!(text.contains("socat_rs_bytes_left_to_right_total 10"));
        assert!(text.contains("socat_rs_bytes_right_to_left_total 20"));
    }
}
