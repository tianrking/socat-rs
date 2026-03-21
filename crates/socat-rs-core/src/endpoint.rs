use std::env;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use native_tls::{Identity, TlsAcceptor, TlsConnector};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncRead, AsyncWrite, stdin, stdout};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::sleep;
use tokio_native_tls::{TlsAcceptor as TokioTlsAcceptor, TlsConnector as TokioTlsConnector};

use crate::error::SocoreError;
use crate::spec::{EndpointOptions, EndpointSpec, ProxyAuth, RetryBackoff};

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> IoStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

#[cfg(test)]
pub async fn open(spec: EndpointSpec) -> Result<Box<dyn IoStream>, SocoreError> {
    open_with_options(spec, &EndpointOptions::default()).await
}

pub async fn open_with_options(
    spec: EndpointSpec,
    options: &EndpointOptions,
) -> Result<Box<dyn IoStream>, SocoreError> {
    match spec {
        EndpointSpec::Stdio => Ok(Box::new(StdioEndpoint::new())),
        EndpointSpec::TcpConnect(addr) => {
            with_connect_policy(options, || {
                let addr = addr.clone();
                async move { Ok(Box::new(TcpStream::connect(addr).await?) as Box<dyn IoStream>) }
            })
            .await
        }
        EndpointSpec::TcpListen(addr) => {
            let listener = TcpListener::bind(addr).await?;
            let (stream, _) = listener.accept().await?;
            Ok(Box::new(stream))
        }
        EndpointSpec::UdpConnect(addr) => {
            with_connect_policy(options, || {
                let addr = addr.clone();
                async move {
                    let socket = UdpSocket::bind("0.0.0.0:0").await?;
                    socket.connect(addr).await?;
                    Ok(Box::new(UdpStream::new(socket, Vec::new())) as Box<dyn IoStream>)
                }
            })
            .await
        }
        EndpointSpec::UdpListen(addr) => {
            let socket = UdpSocket::bind(addr).await?;
            let mut first = vec![0_u8; 65_535];
            let (n, peer) = socket.recv_from(&mut first).await?;
            first.truncate(n);
            socket.connect(peer).await?;
            Ok(Box::new(UdpStream::new(socket, first)))
        }
        EndpointSpec::TlsConnect(addr) => {
            with_connect_policy(options, || {
                let addr = addr.clone();
                let options = options.clone();
                async move { open_tls_connect(addr, &options).await }
            })
            .await
        }
        EndpointSpec::TlsListen(addr) => open_tls_listen(addr).await,
        EndpointSpec::Socks4Connect { proxy, target } => {
            with_connect_policy(options, || {
                let proxy = proxy.clone();
                let target = target.clone();
                async move { open_socks4_connect(proxy, target, false).await }
            })
            .await
        }
        EndpointSpec::Socks4aConnect { proxy, target } => {
            with_connect_policy(options, || {
                let proxy = proxy.clone();
                let target = target.clone();
                async move { open_socks4_connect(proxy, target, true).await }
            })
            .await
        }
        EndpointSpec::Socks5Connect {
            proxy,
            target,
            auth,
        } => {
            with_connect_policy(options, || {
                let proxy = proxy.clone();
                let target = target.clone();
                let auth = auth.clone();
                async move { open_socks5_connect(proxy, target, auth).await }
            })
            .await
        }
        EndpointSpec::HttpProxyConnect {
            proxy,
            target,
            auth,
        } => {
            with_connect_policy(options, || {
                let proxy = proxy.clone();
                let target = target.clone();
                let auth = auth.clone();
                async move { open_http_proxy_connect(proxy, target, auth).await }
            })
            .await
        }
        EndpointSpec::Exec(cmd) => open_process_stream(cmd, ProcessKind::Exec).await,
        EndpointSpec::System(cmd) => open_process_stream(cmd, ProcessKind::System).await,
        EndpointSpec::Shell(cmd) => open_process_stream(cmd, ProcessKind::Shell).await,
        EndpointSpec::UnixConnect(path) => {
            with_connect_policy(options, || {
                let path = path.clone();
                async move { open_unix_connect(path).await }
            })
            .await
        }
        EndpointSpec::UnixListen(path) => open_unix_listen(path).await,
        EndpointSpec::File(path) => {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .await?;
            Ok(Box::new(file))
        }
        EndpointSpec::NamedPipe(path) => {
            with_connect_policy(options, || {
                let path = path.clone();
                async move { open_named_pipe(path).await }
            })
            .await
        }
        EndpointSpec::Unsupported(name) => Err(SocoreError::UnsupportedEndpoint(name)),
    }
}

async fn with_connect_policy<T, F, Fut>(
    options: &EndpointOptions,
    mut operation: F,
) -> Result<T, SocoreError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, SocoreError>>,
{
    let retry = options.retry.unwrap_or(0);
    let attempts = retry.saturating_add(1);
    let base_delay_ms = options.retry_delay_ms.unwrap_or(200);
    let backoff = options.retry_backoff.unwrap_or(RetryBackoff::Constant);
    let max_delay_ms = options
        .retry_max_delay_ms
        .unwrap_or_else(|| base_delay_ms.saturating_mul(32));

    for attempt in 1..=attempts {
        let result = if let Some(timeout_ms) = options.connect_timeout_ms {
            let timeout_duration = Duration::from_millis(timeout_ms);
            match tokio::time::timeout(timeout_duration, operation()).await {
                Ok(v) => v,
                Err(_) => Err(SocoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("connect timeout after {timeout_ms}ms"),
                ))),
            }
        } else {
            operation().await
        };

        match result {
            Ok(v) => return Ok(v),
            Err(_) if attempt < attempts => {
                let delay_ms = retry_delay_for_attempt(
                    base_delay_ms,
                    backoff,
                    max_delay_ms,
                    attempt.saturating_sub(1),
                );
                sleep(Duration::from_millis(delay_ms)).await
            }
            Err(err) => return Err(err),
        }
    }

    Err(SocoreError::UnsupportedEndpoint(
        "connect retry policy reached impossible state".to_string(),
    ))
}

async fn open_socks4_connect(
    proxy: String,
    target: String,
    use_4a: bool,
) -> Result<Box<dyn IoStream>, SocoreError> {
    let mut stream = TcpStream::connect(proxy).await?;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (host, port) = split_host_port(&target)?;
    let mut req = Vec::new();
    req.push(0x04);
    req.push(0x01);
    req.extend_from_slice(&port.to_be_bytes());
    if use_4a {
        req.extend_from_slice(&[0, 0, 0, 1]);
        req.push(0x00);
        req.extend_from_slice(host.as_bytes());
        req.push(0x00);
    } else {
        let ip = host.parse::<Ipv4Addr>().map_err(|_| {
            SocoreError::InvalidAddress(
                "SOCKS4 requires IPv4 target; use SOCKS4A for domain targets".to_string(),
            )
        })?;
        req.extend_from_slice(&ip.octets());
        req.push(0x00);
    }

    stream.write_all(&req).await?;
    let mut resp = [0_u8; 8];
    stream.read_exact(&mut resp).await?;
    if resp[1] != 0x5a {
        return Err(SocoreError::UnsupportedEndpoint(format!(
            "socks4 connect failed with reply code {}",
            resp[1]
        )));
    }
    Ok(Box::new(stream))
}

async fn open_socks5_connect(
    proxy: String,
    target: String,
    auth: Option<ProxyAuth>,
) -> Result<Box<dyn IoStream>, SocoreError> {
    let mut stream = TcpStream::connect(proxy).await?;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    if auth.is_some() {
        stream.write_all(&[0x05, 0x02, 0x00, 0x02]).await?;
    } else {
        stream.write_all(&[0x05, 0x01, 0x00]).await?;
    }
    let mut method = [0_u8; 2];
    stream.read_exact(&mut method).await?;
    if method[0] != 0x05 {
        return Err(SocoreError::UnsupportedEndpoint(
            "invalid socks5 response".to_string(),
        ));
    }
    match method[1] {
        0x00 => {}
        0x02 => {
            let creds = auth.ok_or_else(|| {
                SocoreError::UnsupportedEndpoint(
                    "socks5 proxy requires username/password auth".to_string(),
                )
            })?;
            let uname = creds.username.as_bytes();
            let pass = creds.password.as_bytes();
            if uname.len() > 255 || pass.len() > 255 {
                return Err(SocoreError::InvalidAddress(
                    "socks5 username/password too long".to_string(),
                ));
            }
            let mut auth_req = Vec::with_capacity(3 + uname.len() + pass.len());
            auth_req.push(0x01);
            auth_req.push(uname.len() as u8);
            auth_req.extend_from_slice(uname);
            auth_req.push(pass.len() as u8);
            auth_req.extend_from_slice(pass);
            stream.write_all(&auth_req).await?;
            let mut auth_resp = [0_u8; 2];
            stream.read_exact(&mut auth_resp).await?;
            if auth_resp[1] != 0x00 {
                return Err(SocoreError::UnsupportedEndpoint(
                    "socks5 username/password auth failed".to_string(),
                ));
            }
        }
        m => {
            return Err(SocoreError::UnsupportedEndpoint(format!(
                "socks5 unsupported auth method selected: {m}"
            )));
        }
    }

    let (host, port) = split_host_port(&target)?;
    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(SocoreError::InvalidAddress(
            "socks5 target host too long".to_string(),
        ));
    }
    let mut req = Vec::with_capacity(6 + host_bytes.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
    req.extend_from_slice(host_bytes);
    req.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&req).await?;

    let mut head = [0_u8; 4];
    stream.read_exact(&mut head).await?;
    if head[1] != 0x00 {
        return Err(SocoreError::UnsupportedEndpoint(format!(
            "socks5 connect failed with reply code {}",
            head[1]
        )));
    }
    let atyp = head[3];
    match atyp {
        0x01 => {
            let mut rest = [0_u8; 6];
            stream.read_exact(&mut rest).await?;
        }
        0x03 => {
            let mut len = [0_u8; 1];
            stream.read_exact(&mut len).await?;
            let mut rest = vec![0_u8; usize::from(len[0]) + 2];
            stream.read_exact(&mut rest).await?;
        }
        0x04 => {
            let mut rest = [0_u8; 18];
            stream.read_exact(&mut rest).await?;
        }
        _ => {
            return Err(SocoreError::UnsupportedEndpoint(
                "socks5 returned unsupported address type".to_string(),
            ));
        }
    }

    Ok(Box::new(stream))
}

async fn open_http_proxy_connect(
    proxy: String,
    target: String,
    auth: Option<ProxyAuth>,
) -> Result<Box<dyn IoStream>, SocoreError> {
    let mut stream = TcpStream::connect(proxy).await?;
    use base64::Engine;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut req =
        format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Connection: Keep-Alive\r\n");
    if let Some(auth) = auth {
        let raw = format!("{}:{}", auth.username, auth.password);
        let token = base64::engine::general_purpose::STANDARD.encode(raw);
        req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).await?;

    let mut response = Vec::new();
    let mut buf = [0_u8; 1024];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        response.extend_from_slice(&buf[..n]);
        if response.windows(4).any(|w| w == b"\\r\\n\\r\\n") {
            break;
        }
        if response.len() > 16 * 1024 {
            return Err(SocoreError::UnsupportedEndpoint(
                "http proxy response header too large".to_string(),
            ));
        }
    }
    let resp_text = String::from_utf8_lossy(&response);
    if !(resp_text.starts_with("HTTP/1.1 200") || resp_text.starts_with("HTTP/1.0 200")) {
        return Err(SocoreError::UnsupportedEndpoint(format!(
            "http proxy connect failed: {}",
            resp_text.lines().next().unwrap_or("<empty response>")
        )));
    }
    Ok(Box::new(stream))
}

async fn open_tls_connect(
    addr: String,
    options: &EndpointOptions,
) -> Result<Box<dyn IoStream>, SocoreError> {
    let stream = TcpStream::connect(&addr).await?;
    let host = options
        .tls_sni
        .clone()
        .unwrap_or_else(|| extract_host(&addr));

    let mut builder = TlsConnector::builder();
    if matches!(options.tls_verify, Some(false)) {
        builder.danger_accept_invalid_certs(true);
    }
    let connector = builder.build()?;
    let connector = TokioTlsConnector::from(connector);
    let tls = connector.connect(&host, stream).await?;
    Ok(Box::new(tls))
}

fn retry_delay_for_attempt(
    base_delay_ms: u64,
    backoff: RetryBackoff,
    max_delay_ms: u64,
    attempt_index: u32,
) -> u64 {
    let delay = match backoff {
        RetryBackoff::Constant => base_delay_ms,
        RetryBackoff::Exponential => {
            let factor = 1_u64.checked_shl(attempt_index.min(20)).unwrap_or(u64::MAX);
            base_delay_ms.saturating_mul(factor)
        }
    };
    delay.min(max_delay_ms)
}

async fn open_tls_listen(addr: String) -> Result<Box<dyn IoStream>, SocoreError> {
    let pkcs12_path = env::var("SOCAT_RS_TLS_PKCS12").map_err(|_| {
        SocoreError::UnsupportedEndpoint(
            "tls-listen requires env SOCAT_RS_TLS_PKCS12=<path-to-identity.p12>".to_string(),
        )
    })?;
    let password = env::var("SOCAT_RS_TLS_PASSWORD").unwrap_or_default();

    let der = std::fs::read(pkcs12_path)?;
    let identity = Identity::from_pkcs12(&der, &password)?;
    let acceptor = TlsAcceptor::new(identity)?;
    let acceptor = TokioTlsAcceptor::from(acceptor);

    let listener = TcpListener::bind(addr).await?;
    let (stream, _) = listener.accept().await?;
    let tls = acceptor.accept(stream).await?;
    Ok(Box::new(tls))
}

fn extract_host(addr: &str) -> String {
    if let Some(stripped) = addr.strip_prefix('[')
        && let Some((host, _)) = stripped.split_once("]:")
    {
        return host.to_string();
    }
    addr.rsplit_once(':')
        .map_or_else(|| addr.to_string(), |(host, _)| host.to_string())
}

fn split_host_port(target: &str) -> Result<(String, u16), SocoreError> {
    if let Some(stripped) = target.strip_prefix('[')
        && let Some((host, rest)) = stripped.split_once("]:")
    {
        let port = rest
            .parse::<u16>()
            .map_err(|_| SocoreError::InvalidAddress(format!("invalid target port in {target}")))?;
        return Ok((host.to_string(), port));
    }
    if let Some((host, port_s)) = target.rsplit_once(':') {
        let port = port_s
            .parse::<u16>()
            .map_err(|_| SocoreError::InvalidAddress(format!("invalid target port in {target}")))?;
        return Ok((host.to_string(), port));
    }
    Err(SocoreError::InvalidAddress(format!(
        "invalid target, expected host:port, got {target}"
    )))
}

#[derive(Debug, Clone, Copy)]
enum ProcessKind {
    Exec,
    System,
    Shell,
}

async fn open_process_stream(
    cmd: String,
    kind: ProcessKind,
) -> Result<Box<dyn IoStream>, SocoreError> {
    let mut command = match kind {
        ProcessKind::Exec => {
            let mut parts = cmd.split_whitespace();
            let program = parts
                .next()
                .ok_or_else(|| SocoreError::InvalidAddress("empty EXEC command".to_string()))?;
            let mut command = Command::new(program);
            for arg in parts {
                command.arg(arg);
            }
            command
        }
        ProcessKind::System | ProcessKind::Shell => shell_command(&cmd),
    };

    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());

    let mut child = command.spawn()?;
    let child_stdin = child.stdin.take().ok_or_else(|| {
        SocoreError::UnsupportedEndpoint("failed to capture child stdin".to_string())
    })?;
    let child_stdout = child.stdout.take().ok_or_else(|| {
        SocoreError::UnsupportedEndpoint("failed to capture child stdout".to_string())
    })?;

    Ok(Box::new(ProcessStream {
        stdin: child_stdin,
        stdout: child_stdout,
        child,
    }))
}

fn shell_command(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

#[cfg(unix)]
async fn open_unix_connect(path: PathBuf) -> Result<Box<dyn IoStream>, SocoreError> {
    use tokio::net::UnixStream;
    Ok(Box::new(UnixStream::connect(path).await?))
}

#[cfg(not(unix))]
async fn open_unix_connect(path: PathBuf) -> Result<Box<dyn IoStream>, SocoreError> {
    let _ = path;
    Err(SocoreError::UnsupportedEndpoint(
        "unix domain socket is unavailable on this platform".to_string(),
    ))
}

#[cfg(unix)]
async fn open_unix_listen(path: PathBuf) -> Result<Box<dyn IoStream>, SocoreError> {
    use tokio::net::UnixListener;
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(path)?;
    let (stream, _) = listener.accept().await?;
    Ok(Box::new(stream))
}

#[cfg(not(unix))]
async fn open_unix_listen(path: PathBuf) -> Result<Box<dyn IoStream>, SocoreError> {
    let _ = path;
    Err(SocoreError::UnsupportedEndpoint(
        "unix domain socket is unavailable on this platform".to_string(),
    ))
}

#[cfg(windows)]
async fn open_named_pipe(path: String) -> Result<Box<dyn IoStream>, SocoreError> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let client = ClientOptions::new().open(path)?;
    Ok(Box::new(client))
}

#[cfg(not(windows))]
async fn open_named_pipe(path: String) -> Result<Box<dyn IoStream>, SocoreError> {
    let _ = path;
    Err(SocoreError::UnsupportedEndpoint(
        "named pipe is only available on Windows".to_string(),
    ))
}

struct StdioEndpoint {
    input: tokio::io::Stdin,
    output: tokio::io::Stdout,
}

impl StdioEndpoint {
    fn new() -> Self {
        Self {
            input: stdin(),
            output: stdout(),
        }
    }
}

impl tokio::io::AsyncRead for StdioEndpoint {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.input).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for StdioEndpoint {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.output).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.output).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.output).poll_shutdown(cx)
    }
}

struct UdpStream {
    socket: UdpSocket,
    prebuffer: Vec<u8>,
    prebuffer_pos: usize,
}

impl UdpStream {
    fn new(socket: UdpSocket, prebuffer: Vec<u8>) -> Self {
        Self {
            socket,
            prebuffer,
            prebuffer_pos: 0,
        }
    }
}

impl tokio::io::AsyncRead for UdpStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.prebuffer_pos < self.prebuffer.len() {
            let remaining = self.prebuffer.len() - self.prebuffer_pos;
            let to_copy = remaining.min(buf.remaining());
            let end = self.prebuffer_pos + to_copy;
            buf.put_slice(&self.prebuffer[self.prebuffer_pos..end]);
            self.prebuffer_pos = end;
            return std::task::Poll::Ready(Ok(()));
        }
        std::pin::Pin::new(&mut self.socket).poll_recv(cx, buf)
    }
}

impl tokio::io::AsyncWrite for UdpStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.socket).poll_send(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

struct ProcessStream {
    stdin: ChildStdin,
    stdout: ChildStdout,
    child: Child,
}

impl Drop for ProcessStream {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl tokio::io::AsyncRead for ProcessStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stdout).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for ProcessStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.stdin).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stdin).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stdin).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::spec::{EndpointOptions, EndpointSpec, ProxyAuth, RetryBackoff};

    #[tokio::test]
    async fn udp_connect_roundtrip() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind udp server");
        let server_addr = server.local_addr().expect("server addr");

        tokio::spawn(async move {
            let mut buf = [0_u8; 1024];
            let (n, peer) = server.recv_from(&mut buf).await.expect("recv");
            let payload = &buf[..n];
            server.send_to(payload, peer).await.expect("send");
        });

        let mut stream = super::open(EndpointSpec::UdpConnect(server_addr.to_string()))
            .await
            .expect("open udp connect");
        stream.write_all(b"ping").await.expect("write");
        let mut out = [0_u8; 4];
        stream.read_exact(&mut out).await.expect("read");
        assert_eq!(&out, b"ping");
    }

    #[tokio::test]
    async fn socks5_connect_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let proxy_addr = listener.local_addr().expect("proxy addr");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = [0_u8; 3];
            socket.read_exact(&mut buf).await.expect("read greeting");
            socket
                .write_all(&[0x05, 0x00])
                .await
                .expect("write method response");

            let mut head = [0_u8; 5];
            socket
                .read_exact(&mut head)
                .await
                .expect("read connect head");
            let domain_len = usize::from(head[4]);
            let mut rest = vec![0_u8; domain_len + 2];
            socket
                .read_exact(&mut rest)
                .await
                .expect("read connect rest");

            socket
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90])
                .await
                .expect("write connect ok");
            socket.write_all(b"pong").await.expect("write payload");
        });

        let mut stream = super::open(EndpointSpec::Socks5Connect {
            proxy: proxy_addr.to_string(),
            target: "example.com:443".to_string(),
            auth: None,
        })
        .await
        .expect("open socks5");
        let mut data = [0_u8; 4];
        stream.read_exact(&mut data).await.expect("read payload");
        assert_eq!(&data, b"pong");
    }

    #[tokio::test]
    async fn http_proxy_connect_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let proxy_addr = listener.local_addr().expect("proxy addr");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut req = Vec::new();
            let mut buf = [0_u8; 1024];
            loop {
                let n = socket.read(&mut buf).await.expect("read request");
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                if req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .expect("write response");
        });

        let _stream = super::open(EndpointSpec::HttpProxyConnect {
            proxy: proxy_addr.to_string(),
            target: "example.com:443".to_string(),
            auth: None,
        })
        .await
        .expect("open http proxy");
    }

    #[tokio::test]
    async fn socks4_connect_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let proxy_addr = listener.local_addr().expect("proxy addr");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut req = [0_u8; 9];
            socket.read_exact(&mut req).await.expect("read request");
            socket
                .write_all(&[0x00, 0x5a, 0x01, 0xbb, 127, 0, 0, 1])
                .await
                .expect("write response");
        });

        let _stream = super::open(EndpointSpec::Socks4Connect {
            proxy: proxy_addr.to_string(),
            target: "1.2.3.4:443".to_string(),
        })
        .await
        .expect("open socks4");
    }

    #[tokio::test]
    async fn socks4a_connect_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let proxy_addr = listener.local_addr().expect("proxy addr");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut req = Vec::new();
            let mut buf = [0_u8; 256];
            loop {
                let n = socket.read(&mut buf).await.expect("read request");
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                if req.ends_with(b"\x00") {
                    break;
                }
                if req.len() > 256 {
                    break;
                }
            }
            socket
                .write_all(&[0x00, 0x5a, 0x01, 0xbb, 127, 0, 0, 1])
                .await
                .expect("write response");
        });

        let _stream = super::open(EndpointSpec::Socks4aConnect {
            proxy: proxy_addr.to_string(),
            target: "example.com:443".to_string(),
        })
        .await
        .expect("open socks4a");
    }

    #[tokio::test]
    async fn socks5_auth_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let proxy_addr = listener.local_addr().expect("proxy addr");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut greet = [0_u8; 4];
            socket.read_exact(&mut greet).await.expect("read greeting");
            socket
                .write_all(&[0x05, 0x02])
                .await
                .expect("select auth method");

            let mut auth_head = [0_u8; 2];
            socket
                .read_exact(&mut auth_head)
                .await
                .expect("read auth head");
            let ulen = usize::from(auth_head[1]);
            let mut uname = vec![0_u8; ulen];
            socket.read_exact(&mut uname).await.expect("read username");
            let mut plen = [0_u8; 1];
            socket.read_exact(&mut plen).await.expect("read plen");
            let mut pass = vec![0_u8; usize::from(plen[0])];
            socket.read_exact(&mut pass).await.expect("read password");
            socket.write_all(&[0x01, 0x00]).await.expect("auth ok");

            let mut head = [0_u8; 5];
            socket
                .read_exact(&mut head)
                .await
                .expect("read connect head");
            let domain_len = usize::from(head[4]);
            let mut rest = vec![0_u8; domain_len + 2];
            socket
                .read_exact(&mut rest)
                .await
                .expect("read connect rest");
            socket
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90])
                .await
                .expect("write connect ok");
        });

        let _stream = super::open(EndpointSpec::Socks5Connect {
            proxy: proxy_addr.to_string(),
            target: "example.com:443".to_string(),
            auth: Some(ProxyAuth {
                username: "u".to_string(),
                password: "p".to_string(),
            }),
        })
        .await
        .expect("open socks5 auth");
    }

    #[tokio::test]
    async fn http_proxy_basic_auth_header() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let proxy_addr = listener.local_addr().expect("proxy addr");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut req = Vec::new();
            let mut buf = [0_u8; 1024];
            loop {
                let n = socket.read(&mut buf).await.expect("read request");
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                if req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8(req).expect("utf8");
            assert!(text.contains("Proxy-Authorization: Basic dTpw"));
            socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .expect("write response");
        });

        let _stream = super::open(EndpointSpec::HttpProxyConnect {
            proxy: proxy_addr.to_string(),
            target: "example.com:443".to_string(),
            auth: Some(ProxyAuth {
                username: "u".to_string(),
                password: "p".to_string(),
            }),
        })
        .await
        .expect("open http proxy auth");
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn named_pipe_not_supported_on_non_windows() {
        let err = match super::open(EndpointSpec::NamedPipe(r"\\.\pipe\socat-rs".to_string())).await
        {
            Ok(_) => panic!("named pipe should be unsupported"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("only available on Windows"));
    }

    #[tokio::test]
    async fn connect_policy_retries_then_succeeds() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let options = EndpointOptions {
            connect_timeout_ms: None,
            retry: Some(2),
            retry_delay_ms: Some(1),
            retry_backoff: Some(RetryBackoff::Constant),
            retry_max_delay_ms: None,
            tls_verify: None,
            tls_sni: None,
        };

        let value = super::with_connect_policy(&options, {
            let attempts = attempts.clone();
            move || {
                let attempts = attempts.clone();
                async move {
                    let current = attempts.fetch_add(1, Ordering::SeqCst);
                    if current < 2 {
                        Err(crate::error::SocoreError::UnsupportedEndpoint(
                            "not yet".to_string(),
                        ))
                    } else {
                        Ok(42_u8)
                    }
                }
            }
        })
        .await
        .expect("retry success");

        assert_eq!(value, 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn connect_policy_timeout() {
        let options = EndpointOptions {
            connect_timeout_ms: Some(10),
            retry: Some(0),
            retry_delay_ms: Some(1),
            retry_backoff: Some(RetryBackoff::Constant),
            retry_max_delay_ms: None,
            tls_verify: None,
            tls_sni: None,
        };

        let err = super::with_connect_policy(&options, || async {
            tokio::time::sleep(Duration::from_millis(80)).await;
            Ok::<_, crate::error::SocoreError>(())
        })
        .await
        .expect_err("should timeout");

        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn retry_backoff_exponential_caps_at_max() {
        let d1 = super::retry_delay_for_attempt(100, RetryBackoff::Exponential, 10_000, 0);
        let d2 = super::retry_delay_for_attempt(100, RetryBackoff::Exponential, 10_000, 3);
        let d3 = super::retry_delay_for_attempt(100, RetryBackoff::Exponential, 700, 10);
        assert_eq!(d1, 100);
        assert_eq!(d2, 800);
        assert_eq!(d3, 700);
    }
}
