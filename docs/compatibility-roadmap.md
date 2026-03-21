# Compatibility Roadmap

## Principle

Target is complete feature coverage of upstream `socat`, while exposing simpler commands for daily use.

## Current baseline

Implemented now:

- Parser:
  - legacy subset (`STDIO`, `TCP*`, `UDP*`, `SSL/OPENSSL*`, `SOCKS4*`, `SOCKS4A*`, `SOCKS5*`, `PROXY*`, `EXEC/SYSTEM/SHELL`, `UNIX*`, `FILE/OPEN/GOPEN`, `PIPE/NPIPE`)
  - simple URI subset (`stdio://`, `tcp://`, `tcp-listen://`, `udp://`, `udp-listen://`, `tls://`, `tls-listen://`, `socks4://`, `socks4a://`, `socks5://`, `http-proxy://`, `exec://`, `system://`, `shell://`, `unix://`, `unix-listen://`, `file://`, `npipe://`)
- Relay core: bidirectional async copy
- CI: linux/macOS/windows
- Runtime option engine v1:
  - `connect-timeout`
  - `retry`
  - `retry-delay`
  - `retry-backoff`
  - `retry-max-delay`
  - `tls-verify`
  - `tls-sni`
- Friendly CLI planning path:
  - `plan` and `validate` commands with JSON output
  - built-in `dev/prod/lan/wan` profile defaults
  - `tunnel` command for one-line proxy chains
- Observability baseline:
  - JSON run-report on executed links
  - Prometheus metrics endpoint (`--metrics-bind`)

## Phase 1: Transport completeness

- UDP / UDP4 / UDP6 + datagram modes
- OpenSSL/DTLS
- SOCKS4/SOCKS4A/SOCKS5 and HTTP proxy paths
- VSOCK

## Phase 2: Option engine parity

- Typed option parser IR
- Option groups + phase-aware option application
- Alias normalization for legacy option names
- JSON explain plan for each applied option

## Phase 3: Process and tty stack

- EXEC / SYSTEM / SHELL parity
- PTY and termios model
- Signal behavior parity (`-S`, graceful close, child handling)

## Phase 4: Advanced kernel features

- raw IP / multicast controls
- interface/tun features
- platform-specific capability checks and diagnostics

## Phase 5: Compatibility validation

- replay/adapt upstream `test.sh` as Rust integration test matrix
- golden behavior snapshots for representative command sets
- benchmark vs upstream for throughput and startup latency
