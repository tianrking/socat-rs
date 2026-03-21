use serde::Serialize;
use std::path::PathBuf;

use crate::error::SocoreError;

#[derive(Debug, Clone, Serialize)]
pub struct ProxyAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProxyType {
    Socks4,
    Socks4a,
    Socks5,
    HttpProxy,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyHop {
    pub kind: ProxyType,
    pub proxy: String,
    pub auth: Option<ProxyAuth>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct EndpointOptions {
    pub connect_timeout_ms: Option<u64>,
    pub retry: Option<u32>,
    pub retry_delay_ms: Option<u64>,
    pub retry_backoff: Option<RetryBackoff>,
    pub retry_max_delay_ms: Option<u64>,
    pub tls_verify: Option<bool>,
    pub tls_sni: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RetryBackoff {
    Constant,
    Exponential,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointPlan {
    pub endpoint: EndpointSpec,
    pub options: EndpointOptions,
}

#[derive(Debug, Clone, Serialize)]
pub enum EndpointSpec {
    Stdio,
    TcpConnect(String),
    TcpListen(String),
    UdpConnect(String),
    UdpListen(String),
    TlsConnect(String),
    TlsListen(String),
    Socks4Connect {
        proxy: String,
        target: String,
    },
    Socks4aConnect {
        proxy: String,
        target: String,
    },
    Socks5Connect {
        proxy: String,
        target: String,
        auth: Option<ProxyAuth>,
    },
    HttpProxyConnect {
        proxy: String,
        target: String,
        auth: Option<ProxyAuth>,
    },
    ProxyChain {
        hops: Vec<ProxyHop>,
        target: String,
    },
    Exec(String),
    System(String),
    Shell(String),
    UnixConnect(PathBuf),
    UnixListen(PathBuf),
    File(PathBuf),
    NamedPipe(String),
    Unsupported(String),
}

#[derive(Clone, Copy)]
enum ProxyKind {
    Socks4,
    Socks4a,
    Socks5,
    HttpProxy,
}

#[cfg(test)]
pub fn parse_legacy(input: &str) -> Result<EndpointSpec, SocoreError> {
    Ok(parse_legacy_with_options(input)?.endpoint)
}

pub fn parse_legacy_with_options(input: &str) -> Result<EndpointPlan, SocoreError> {
    if input == "-" || input.eq_ignore_ascii_case("STDIO") {
        return Ok(EndpointPlan {
            endpoint: EndpointSpec::Stdio,
            options: EndpointOptions::default(),
        });
    }

    let (head, tail) = input
        .split_once(':')
        .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
    let head = head.to_ascii_uppercase();

    let mut parts = tail.split(',');
    let value = parts.next().unwrap_or(tail).trim().to_string();
    let options: Vec<String> = parts.map(|s| s.trim().to_string()).collect();

    let endpoint = match head.as_str() {
        "TCP" | "TCP-CONNECT" | "TCP4" | "TCP4-CONNECT" | "TCP6" | "TCP6-CONNECT" => {
            Ok(EndpointSpec::TcpConnect(value))
        }
        "TCP-LISTEN" | "TCP-L" | "TCP4-LISTEN" | "TCP6-LISTEN" | "TCP4-L" | "TCP6-L" => {
            Ok(EndpointSpec::TcpListen(value))
        }
        "UDP" | "UDP-CONNECT" | "UDP4" | "UDP4-CONNECT" | "UDP6" | "UDP6-CONNECT" => {
            Ok(EndpointSpec::UdpConnect(value))
        }
        "UDP-LISTEN" | "UDP-L" | "UDP4-LISTEN" | "UDP6-LISTEN" | "UDP4-L" | "UDP6-L" => {
            Ok(EndpointSpec::UdpListen(value))
        }
        "SSL" | "OPENSSL" | "SSL-CONNECT" | "OPENSSL-CONNECT" => {
            Ok(EndpointSpec::TlsConnect(value))
        }
        "SSL-LISTEN" | "SSL-L" | "OPENSSL-LISTEN" => Ok(EndpointSpec::TlsListen(value)),
        "SOCKS4" | "SOCKS4-CONNECT" => parse_legacy_proxy(value, ProxyKind::Socks4, None),
        "SOCKS4A" | "SOCKS4A-CONNECT" => parse_legacy_proxy(value, ProxyKind::Socks4a, None),
        "SOCKS5" | "SOCKS5-CONNECT" => {
            parse_legacy_proxy(value, ProxyKind::Socks5, parse_legacy_proxy_auth(&options))
        }
        "PROXY" | "PROXY-CONNECT" => parse_legacy_proxy(
            value,
            ProxyKind::HttpProxy,
            parse_legacy_proxy_auth(&options),
        ),
        "EXEC" => Ok(EndpointSpec::Exec(value)),
        "SYSTEM" => Ok(EndpointSpec::System(value)),
        "SHELL" => Ok(EndpointSpec::Shell(value)),
        "UNIX" | "UNIX-CONNECT" | "UNIX-CLIENT" => {
            Ok(EndpointSpec::UnixConnect(PathBuf::from(value)))
        }
        "UNIX-LISTEN" | "UNIX-L" => Ok(EndpointSpec::UnixListen(PathBuf::from(value))),
        "OPEN" | "FILE" | "GOPEN" => Ok(EndpointSpec::File(PathBuf::from(value))),
        "NPIPE" | "PIPE" => Ok(EndpointSpec::NamedPipe(normalize_named_pipe(None, &value)?)),
        _ => Ok(EndpointSpec::Unsupported(head)),
    }?;
    Ok(EndpointPlan {
        endpoint,
        options: parse_endpoint_options(&options)?,
    })
}

#[cfg(test)]
pub fn parse_simple_uri(input: &str) -> Result<EndpointSpec, SocoreError> {
    Ok(parse_simple_uri_with_options(input)?.endpoint)
}

pub fn parse_simple_uri_with_options(input: &str) -> Result<EndpointPlan, SocoreError> {
    let url = url::Url::parse(input).map_err(|_| SocoreError::InvalidAddress(input.to_string()))?;

    let endpoint = match url.scheme() {
        "stdio" => Ok(EndpointSpec::Stdio),
        "tcp" => {
            let host = url.host_str().unwrap_or("127.0.0.1");
            let port = url
                .port_or_known_default()
                .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
            Ok(EndpointSpec::TcpConnect(format!("{host}:{port}")))
        }
        "tcp-listen" => {
            let host = url.host_str().unwrap_or("0.0.0.0");
            let port = url
                .port_or_known_default()
                .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
            Ok(EndpointSpec::TcpListen(format!("{host}:{port}")))
        }
        "udp" => {
            let host = url.host_str().unwrap_or("127.0.0.1");
            let port = url
                .port_or_known_default()
                .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
            Ok(EndpointSpec::UdpConnect(format!("{host}:{port}")))
        }
        "udp-listen" => {
            let host = url.host_str().unwrap_or("0.0.0.0");
            let port = url
                .port_or_known_default()
                .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
            Ok(EndpointSpec::UdpListen(format!("{host}:{port}")))
        }
        "tls" => {
            let host = url
                .host_str()
                .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
            let port = url
                .port_or_known_default()
                .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
            Ok(EndpointSpec::TlsConnect(format!("{host}:{port}")))
        }
        "tls-listen" => {
            let host = url.host_str().unwrap_or("0.0.0.0");
            let port = url
                .port_or_known_default()
                .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
            Ok(EndpointSpec::TlsListen(format!("{host}:{port}")))
        }
        "socks4" => parse_simple_proxy_uri(&url, ProxyKind::Socks4),
        "socks4a" => parse_simple_proxy_uri(&url, ProxyKind::Socks4a),
        "socks5" => parse_simple_proxy_uri(&url, ProxyKind::Socks5),
        "http-proxy" | "proxy" => parse_simple_proxy_uri(&url, ProxyKind::HttpProxy),
        "exec" => Ok(EndpointSpec::Exec(command_from_url(&url)?)),
        "system" => Ok(EndpointSpec::System(command_from_url(&url)?)),
        "shell" => Ok(EndpointSpec::Shell(command_from_url(&url)?)),
        "unix" => Ok(EndpointSpec::UnixConnect(PathBuf::from(url.path()))),
        "unix-listen" => Ok(EndpointSpec::UnixListen(PathBuf::from(url.path()))),
        "file" => Ok(EndpointSpec::File(PathBuf::from(url.path()))),
        "npipe" => Ok(EndpointSpec::NamedPipe(parse_simple_named_pipe(&url)?)),
        other => Ok(EndpointSpec::Unsupported(other.to_string())),
    }?;
    let options: Vec<String> = url.query_pairs().map(|(k, v)| format!("{k}={v}")).collect();
    Ok(EndpointPlan {
        endpoint,
        options: parse_endpoint_options(&options)?,
    })
}

fn parse_legacy_proxy(
    value: String,
    kind: ProxyKind,
    auth: Option<ProxyAuth>,
) -> Result<EndpointSpec, SocoreError> {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() < 4 {
        return Err(SocoreError::InvalidAddress(
            "expected proxy format: <proxy-host>:<proxy-port>:<target-host>:<target-port>"
                .to_string(),
        ));
    }
    let proxy = format!("{}:{}", parts[0], parts[1]);
    let target = format!("{}:{}", parts[2], parts[3]);
    proxy_endpoint(kind, proxy, target, auth)
}

fn parse_simple_proxy_uri(url: &url::Url, kind: ProxyKind) -> Result<EndpointSpec, SocoreError> {
    let proxy_host = url
        .host_str()
        .ok_or_else(|| SocoreError::InvalidAddress("missing proxy host".to_string()))?;
    let proxy_port = url
        .port_or_known_default()
        .ok_or_else(|| SocoreError::InvalidAddress("missing proxy port".to_string()))?;
    let target = url
        .query_pairs()
        .find(|(k, _)| k == "target")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| {
            SocoreError::InvalidAddress(
                "missing target; use '?target=host:port' for proxy endpoints".to_string(),
            )
        })?;
    let proxy = format!("{proxy_host}:{proxy_port}");
    let auth = proxy_auth_from_url(url);
    proxy_endpoint(kind, proxy, target, auth)
}

fn proxy_endpoint(
    kind: ProxyKind,
    proxy: String,
    target: String,
    auth: Option<ProxyAuth>,
) -> Result<EndpointSpec, SocoreError> {
    match kind {
        ProxyKind::Socks4 => Ok(EndpointSpec::Socks4Connect { proxy, target }),
        ProxyKind::Socks4a => Ok(EndpointSpec::Socks4aConnect { proxy, target }),
        ProxyKind::Socks5 => Ok(EndpointSpec::Socks5Connect {
            proxy,
            target,
            auth,
        }),
        ProxyKind::HttpProxy => Ok(EndpointSpec::HttpProxyConnect {
            proxy,
            target,
            auth,
        }),
    }
}

pub fn parse_proxy_hop_uri(input: &str) -> Result<ProxyHop, SocoreError> {
    let url = url::Url::parse(input).map_err(|_| SocoreError::InvalidAddress(input.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| SocoreError::InvalidAddress("missing proxy host".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| SocoreError::InvalidAddress("missing proxy port".to_string()))?;
    let proxy = format!("{host}:{port}");
    let auth = proxy_auth_from_url(&url);
    let kind = match url.scheme() {
        "socks4" => ProxyType::Socks4,
        "socks4a" => ProxyType::Socks4a,
        "socks5" => ProxyType::Socks5,
        "http-proxy" | "proxy" => ProxyType::HttpProxy,
        other => {
            return Err(SocoreError::InvalidAddress(format!(
                "unsupported proxy scheme for tunnel hop: {other}"
            )));
        }
    };
    Ok(ProxyHop { kind, proxy, auth })
}

fn proxy_auth_from_url(url: &url::Url) -> Option<ProxyAuth> {
    if url.username().is_empty() {
        return None;
    }
    Some(ProxyAuth {
        username: url.username().to_string(),
        password: url.password().unwrap_or("").to_string(),
    })
}

fn parse_legacy_proxy_auth(options: &[String]) -> Option<ProxyAuth> {
    let mut user: Option<String> = None;
    let mut pass: Option<String> = None;
    for opt in options {
        if let Some((k, v)) = opt.split_once('=') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            match key.as_str() {
                "socksuser" | "proxy-user" | "proxyuser" | "user" => user = Some(val),
                "sockspass" | "proxy-pass" | "proxypass" | "pass" | "password" => pass = Some(val),
                _ => {}
            }
        }
    }
    user.map(|u| ProxyAuth {
        username: u,
        password: pass.unwrap_or_default(),
    })
}

fn parse_endpoint_options(options: &[String]) -> Result<EndpointOptions, SocoreError> {
    let mut out = EndpointOptions::default();
    for opt in options {
        let Some((k, v)) = opt.split_once('=') else {
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let value = v.trim();
        match key.as_str() {
            "connect-timeout" | "connect_timeout" | "timeout" => {
                out.connect_timeout_ms = Some(parse_duration_to_ms(value)?);
            }
            "retry" => {
                let retry = value.parse::<u32>().map_err(|_| {
                    SocoreError::InvalidAddress(format!("invalid retry value: {value}"))
                })?;
                out.retry = Some(retry);
            }
            "retry-delay" | "retry_delay" => {
                out.retry_delay_ms = Some(parse_duration_to_ms(value)?);
            }
            "retry-backoff" | "retry_backoff" => {
                out.retry_backoff = Some(match value.to_ascii_lowercase().as_str() {
                    "constant" => RetryBackoff::Constant,
                    "exponential" => RetryBackoff::Exponential,
                    other => {
                        return Err(SocoreError::InvalidAddress(format!(
                            "invalid retry-backoff value: {other}"
                        )));
                    }
                });
            }
            "retry-max-delay" | "retry_max_delay" => {
                out.retry_max_delay_ms = Some(parse_duration_to_ms(value)?);
            }
            "tls-verify" | "tls_verify" | "verify" => {
                out.tls_verify = Some(parse_bool(value)?);
            }
            "tls-sni" | "tls_sni" | "sni" => {
                let sni = value.trim();
                if sni.is_empty() {
                    return Err(SocoreError::InvalidAddress(
                        "sni cannot be empty".to_string(),
                    ));
                }
                out.tls_sni = Some(sni.to_string());
            }
            _ => {}
        }
    }
    Ok(out)
}

fn parse_duration_to_ms(value: &str) -> Result<u64, SocoreError> {
    let v = value.trim();
    if v.is_empty() {
        return Err(SocoreError::InvalidAddress(
            "duration value cannot be empty".to_string(),
        ));
    }
    if let Some(ms) = v.strip_suffix("ms") {
        return ms
            .trim()
            .parse::<u64>()
            .map_err(|_| SocoreError::InvalidAddress(format!("invalid duration value: {value}")));
    }
    if let Some(sec) = v.strip_suffix('s') {
        let sec = sec
            .trim()
            .parse::<u64>()
            .map_err(|_| SocoreError::InvalidAddress(format!("invalid duration value: {value}")))?;
        return Ok(sec.saturating_mul(1000));
    }
    v.parse::<u64>()
        .map_err(|_| SocoreError::InvalidAddress(format!("invalid duration value: {value}")))
}

fn parse_bool(value: &str) -> Result<bool, SocoreError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(SocoreError::InvalidAddress(format!(
            "invalid boolean value: {other}"
        ))),
    }
}

fn command_from_url(url: &url::Url) -> Result<String, SocoreError> {
    if let Some((_, cmd)) = url.query_pairs().find(|(k, _)| k == "cmd") {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            return Ok(cmd.to_string());
        }
    }
    let path = url.path().trim_start_matches('/').trim();
    if !path.is_empty() {
        return Ok(path.to_string());
    }
    Err(SocoreError::InvalidAddress(
        "missing command; use query `?cmd=...` or path segment".to_string(),
    ))
}

fn parse_simple_named_pipe(url: &url::Url) -> Result<String, SocoreError> {
    let host = url.host_str();
    normalize_named_pipe(host, url.path())
}

fn normalize_named_pipe(host: Option<&str>, path_or_value: &str) -> Result<String, SocoreError> {
    let raw = path_or_value.trim();
    if raw.is_empty() {
        return Err(SocoreError::InvalidAddress(
            "named pipe cannot be empty".to_string(),
        ));
    }
    let normalized = raw.replace('/', "\\");
    if normalized.starts_with("\\\\.\\pipe\\") || normalized.starts_with("\\\\?\\pipe\\") {
        return Ok(normalized);
    }

    let suffix = normalized
        .trim_start_matches('\\')
        .trim_start_matches("pipe\\")
        .trim();
    if suffix.is_empty() {
        return Err(SocoreError::InvalidAddress(
            "named pipe cannot be empty".to_string(),
        ));
    }

    let host = host.filter(|h| !h.is_empty()).unwrap_or(".");
    Ok(format!("\\\\{host}\\pipe\\{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::{
        EndpointOptions, EndpointSpec, ProxyAuth, RetryBackoff, parse_legacy,
        parse_legacy_with_options, parse_simple_uri, parse_simple_uri_with_options,
    };

    #[test]
    fn parse_legacy_stdio() {
        let got = parse_legacy("-").expect("parse legacy stdio");
        assert!(matches!(got, EndpointSpec::Stdio));
    }

    #[test]
    fn parse_legacy_tcp_listen() {
        let got = parse_legacy("TCP-LISTEN:8080").expect("parse legacy tcp listen");
        assert!(matches!(got, EndpointSpec::TcpListen(addr) if addr == "8080"));
    }

    #[test]
    fn parse_simple_tcp() {
        let got = parse_simple_uri("tcp://127.0.0.1:1234").expect("parse simple tcp");
        assert!(matches!(got, EndpointSpec::TcpConnect(addr) if addr == "127.0.0.1:1234"));
    }

    #[test]
    fn parse_legacy_udp() {
        let got = parse_legacy("UDP:127.0.0.1:9999").expect("parse legacy udp");
        assert!(matches!(got, EndpointSpec::UdpConnect(addr) if addr == "127.0.0.1:9999"));
    }

    #[test]
    fn parse_simple_udp_listen() {
        let got = parse_simple_uri("udp-listen://0.0.0.0:9999").expect("parse simple udp listen");
        assert!(matches!(got, EndpointSpec::UdpListen(addr) if addr == "0.0.0.0:9999"));
    }

    #[test]
    fn parse_legacy_exec() {
        let got = parse_legacy("EXEC:echo hello,pty").expect("parse legacy exec");
        assert!(matches!(got, EndpointSpec::Exec(cmd) if cmd == "echo hello"));
    }

    #[test]
    fn parse_simple_tls() {
        let got = parse_simple_uri("tls://example.com:443").expect("parse simple tls");
        assert!(matches!(got, EndpointSpec::TlsConnect(addr) if addr == "example.com:443"));
    }

    #[test]
    fn parse_legacy_socks5() {
        let got = parse_legacy("SOCKS5:127.0.0.1:1080:example.com:443").expect("parse socks5");
        assert!(matches!(
            got,
            EndpointSpec::Socks5Connect {
                proxy,
                target,
                auth: None
            } if proxy == "127.0.0.1:1080" && target == "example.com:443"
        ));
    }

    #[test]
    fn parse_legacy_socks5_with_auth() {
        let got = parse_legacy("SOCKS5:127.0.0.1:1080:example.com:443,socksuser=u,sockspass=p")
            .expect("parse socks5 with auth");
        assert!(matches!(
            got,
            EndpointSpec::Socks5Connect {
                proxy,
                target,
                auth: Some(ProxyAuth { username, password })
            } if proxy == "127.0.0.1:1080"
                && target == "example.com:443"
                && username == "u"
                && password == "p"
        ));
    }

    #[test]
    fn parse_simple_http_proxy() {
        let got = parse_simple_uri("http-proxy://127.0.0.1:8080?target=example.com:443")
            .expect("parse http proxy");
        assert!(matches!(
            got,
            EndpointSpec::HttpProxyConnect {
                proxy,
                target,
                auth: None
            } if proxy == "127.0.0.1:8080" && target == "example.com:443"
        ));
    }

    #[test]
    fn parse_simple_http_proxy_with_auth() {
        let got = parse_simple_uri("http-proxy://u:p@127.0.0.1:8080?target=example.com:443")
            .expect("parse http proxy auth");
        assert!(matches!(
            got,
            EndpointSpec::HttpProxyConnect {
                proxy,
                target,
                auth: Some(ProxyAuth { username, password })
            } if proxy == "127.0.0.1:8080"
                && target == "example.com:443"
                && username == "u"
                && password == "p"
        ));
    }

    #[test]
    fn parse_legacy_socks4() {
        let got = parse_legacy("SOCKS4:127.0.0.1:1080:1.2.3.4:443").expect("parse socks4");
        assert!(matches!(
            got,
            EndpointSpec::Socks4Connect { proxy, target }
                if proxy == "127.0.0.1:1080" && target == "1.2.3.4:443"
        ));
    }

    #[test]
    fn parse_simple_socks4a() {
        let got =
            parse_simple_uri("socks4a://127.0.0.1:1080?target=example.com:443").expect("parse");
        assert!(matches!(
            got,
            EndpointSpec::Socks4aConnect { proxy, target }
                if proxy == "127.0.0.1:1080" && target == "example.com:443"
        ));
    }

    #[test]
    fn parse_legacy_named_pipe() {
        let got = parse_legacy("PIPE:echo").expect("parse legacy pipe");
        assert!(matches!(got, EndpointSpec::NamedPipe(path) if path == r"\\.\pipe\echo"));
    }

    #[test]
    fn parse_simple_named_pipe() {
        let got = parse_simple_uri("npipe://./pipe/socat-rs").expect("parse npipe uri");
        assert!(matches!(got, EndpointSpec::NamedPipe(path) if path == r"\\.\pipe\socat-rs"));
    }

    #[test]
    fn parse_legacy_with_runtime_options() {
        let plan = parse_legacy_with_options("TCP:127.0.0.1:8080,retry=2,retry-delay=150ms")
            .expect("parse with options");
        assert!(matches!(
            plan.endpoint,
            EndpointSpec::TcpConnect(addr) if addr == "127.0.0.1:8080"
        ));
        assert_eq!(
            plan.options,
            EndpointOptions {
                connect_timeout_ms: None,
                retry: Some(2),
                retry_delay_ms: Some(150),
                retry_backoff: None,
                retry_max_delay_ms: None,
                tls_verify: None,
                tls_sni: None,
            }
        );
    }

    #[test]
    fn parse_simple_uri_with_runtime_options() {
        let plan = parse_simple_uri_with_options(
            "tcp://127.0.0.1:8080?connect-timeout=2s&retry=3&retry-delay=500ms&retry-backoff=exponential&retry-max-delay=2s",
        )
        .expect("parse simple options");
        assert!(matches!(
            plan.endpoint,
            EndpointSpec::TcpConnect(addr) if addr == "127.0.0.1:8080"
        ));
        assert_eq!(
            plan.options,
            EndpointOptions {
                connect_timeout_ms: Some(2000),
                retry: Some(3),
                retry_delay_ms: Some(500),
                retry_backoff: Some(RetryBackoff::Exponential),
                retry_max_delay_ms: Some(2000),
                tls_verify: None,
                tls_sni: None,
            }
        );
    }

    #[test]
    fn parse_tls_options() {
        let plan = parse_simple_uri_with_options(
            "tls://example.com:443?tls-verify=false&sni=alt.example.com",
        )
        .expect("parse tls options");
        assert_eq!(
            plan.options,
            EndpointOptions {
                connect_timeout_ms: None,
                retry: None,
                retry_delay_ms: None,
                retry_backoff: None,
                retry_max_delay_ms: None,
                tls_verify: Some(false),
                tls_sni: Some("alt.example.com".to_string()),
            }
        );
    }
}
