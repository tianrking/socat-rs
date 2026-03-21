use std::env;
use std::path::PathBuf;

use native_tls::{Identity, TlsAcceptor, TlsConnector};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncRead, AsyncWrite, stdin, stdout};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio_native_tls::{TlsAcceptor as TokioTlsAcceptor, TlsConnector as TokioTlsConnector};

use crate::error::SocoreError;
use crate::spec::EndpointSpec;

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> IoStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub async fn open(spec: EndpointSpec) -> Result<Box<dyn IoStream>, SocoreError> {
    match spec {
        EndpointSpec::Stdio => Ok(Box::new(StdioEndpoint::new())),
        EndpointSpec::TcpConnect(addr) => Ok(Box::new(TcpStream::connect(addr).await?)),
        EndpointSpec::TcpListen(addr) => {
            let listener = TcpListener::bind(addr).await?;
            let (stream, _) = listener.accept().await?;
            Ok(Box::new(stream))
        }
        EndpointSpec::UdpConnect(addr) => {
            let socket = UdpSocket::bind("0.0.0.0:0").await?;
            socket.connect(addr).await?;
            Ok(Box::new(UdpStream::new(socket, Vec::new())))
        }
        EndpointSpec::UdpListen(addr) => {
            let socket = UdpSocket::bind(addr).await?;
            let mut first = vec![0_u8; 65_535];
            let (n, peer) = socket.recv_from(&mut first).await?;
            first.truncate(n);
            socket.connect(peer).await?;
            Ok(Box::new(UdpStream::new(socket, first)))
        }
        EndpointSpec::TlsConnect(addr) => open_tls_connect(addr).await,
        EndpointSpec::TlsListen(addr) => open_tls_listen(addr).await,
        EndpointSpec::Exec(cmd) => open_process_stream(cmd, ProcessKind::Exec).await,
        EndpointSpec::System(cmd) => open_process_stream(cmd, ProcessKind::System).await,
        EndpointSpec::Shell(cmd) => open_process_stream(cmd, ProcessKind::Shell).await,
        EndpointSpec::UnixConnect(path) => open_unix_connect(path).await,
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
        EndpointSpec::Unsupported(name) => Err(SocoreError::UnsupportedEndpoint(name)),
    }
}

async fn open_tls_connect(addr: String) -> Result<Box<dyn IoStream>, SocoreError> {
    let stream = TcpStream::connect(&addr).await?;
    let host = extract_host(&addr);

    let connector = TlsConnector::builder().build()?;
    let connector = TokioTlsConnector::from(connector);
    let tls = connector.connect(&host, stream).await?;
    Ok(Box::new(tls))
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::spec::EndpointSpec;

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
}
