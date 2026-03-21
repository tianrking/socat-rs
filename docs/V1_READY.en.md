# socat-rs v1-ready Status (Practical Feature Alignment)

This document tracks the "v1-ready" scope for practical, high-frequency usage.

## Included in v1-ready

- Core relay:
  - bidirectional stream relay
  - JSON dry-run plan output
  - JSON run-report output
  - optional run-report file output (`--report-file`)
- Endpoint families:
  - `stdio`
  - `tcp-connect` / `tcp-listen`
  - `udp-connect` / `udp-listen`
  - `tls-connect` / `tls-listen`
  - `socks4-connect` / `socks4a-connect` / `socks5-connect`
  - `http-proxy-connect`
  - proxy chain runtime (`ProxyChain`)
  - `exec` / `system` / `shell`
  - `unix-connect` / `unix-listen` (unix)
  - `file`
  - `named-pipe-connect` (windows)
- Friendly CLI:
  - `link`
  - `tunnel` (single-hop and multi-hop `--via`)
  - `plan`
  - `validate`
  - `check`
  - `explain`
  - `inventory`
- Runtime policy options:
  - `connect-timeout`
  - `retry`
  - `retry-delay`
  - `retry-backoff`
  - `retry-max-delay`
  - `tls-verify`
  - `tls-sni`
  - `tls-ca-file`
  - `tls-client-pkcs12`
  - `tls-client-password`
- Observability:
  - Prometheus endpoint (`--metrics-bind`)
  - connection and byte counters

## Explicitly out of v1-ready scope

- Full upstream `socat` legacy parity across all long-tail options and families
- PTY/termios full equivalence
- DTLS/SCTP/DCCP/UDPLITE/RAWIP/VSOCK/TUN full implementation

## v1-ready acceptance checks

- `cargo test --workspace` passes
- `cargo clippy --workspace --all-targets -- -D warnings` passes
- CI green on linux/macOS/windows
