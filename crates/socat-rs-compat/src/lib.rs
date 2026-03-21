pub const LEGACY_ADDRESS_KEYWORDS: usize = 215;
pub const LEGACY_OPTION_KEYWORDS: usize = 892;
pub const LEGACY_ADDRESS_HANDLERS: usize = 120;

pub const COMPAT_MODES: &[&str] = &[
    "stdio",
    "tcp-connect",
    "tcp-listen",
    "udp-connect",
    "udp-listen",
    "tls-connect",
    "tls-listen",
    "socks4-connect",
    "socks4a-connect",
    "socks5-connect",
    "http-proxy-connect",
    "exec",
    "system",
    "shell",
    "unix-connect",
    "unix-listen",
    "file",
    "named-pipe-connect",
];
