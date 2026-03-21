use serde::Serialize;
use std::path::PathBuf;

use crate::error::SocoreError;

#[derive(Debug, Clone, Serialize)]
pub enum EndpointSpec {
    Stdio,
    TcpConnect(String),
    TcpListen(String),
    UdpConnect(String),
    UdpListen(String),
    TlsConnect(String),
    TlsListen(String),
    Exec(String),
    System(String),
    Shell(String),
    UnixConnect(PathBuf),
    UnixListen(PathBuf),
    File(PathBuf),
    Unsupported(String),
}

#[cfg(test)]
mod tests {
    use super::{EndpointSpec, parse_legacy, parse_simple_uri};

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
}

pub fn parse_legacy(input: &str) -> Result<EndpointSpec, SocoreError> {
    if input == "-" || input.eq_ignore_ascii_case("STDIO") {
        return Ok(EndpointSpec::Stdio);
    }

    let (head, tail) = input
        .split_once(':')
        .ok_or_else(|| SocoreError::InvalidAddress(input.to_string()))?;
    let head = head.to_ascii_uppercase();
    let value = tail.split(',').next().unwrap_or(tail).trim().to_string();

    match head.as_str() {
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
        "EXEC" => Ok(EndpointSpec::Exec(value)),
        "SYSTEM" => Ok(EndpointSpec::System(value)),
        "SHELL" => Ok(EndpointSpec::Shell(value)),
        "UNIX" | "UNIX-CONNECT" | "UNIX-CLIENT" => {
            Ok(EndpointSpec::UnixConnect(PathBuf::from(value)))
        }
        "UNIX-LISTEN" | "UNIX-L" => Ok(EndpointSpec::UnixListen(PathBuf::from(value))),
        "OPEN" | "FILE" | "GOPEN" => Ok(EndpointSpec::File(PathBuf::from(value))),
        _ => Ok(EndpointSpec::Unsupported(head)),
    }
}

pub fn parse_simple_uri(input: &str) -> Result<EndpointSpec, SocoreError> {
    let url = url::Url::parse(input).map_err(|_| SocoreError::InvalidAddress(input.to_string()))?;

    match url.scheme() {
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
        "exec" => Ok(EndpointSpec::Exec(command_from_url(&url)?)),
        "system" => Ok(EndpointSpec::System(command_from_url(&url)?)),
        "shell" => Ok(EndpointSpec::Shell(command_from_url(&url)?)),
        "unix" => Ok(EndpointSpec::UnixConnect(PathBuf::from(url.path()))),
        "unix-listen" => Ok(EndpointSpec::UnixListen(PathBuf::from(url.path()))),
        "file" => Ok(EndpointSpec::File(PathBuf::from(url.path()))),
        other => Ok(EndpointSpec::Unsupported(other.to_string())),
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
