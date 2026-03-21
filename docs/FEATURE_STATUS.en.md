# socat-rs Current Features and Full Implementation Plan (English)

## 1. Project Goal

`socat-rs` aims to deliver a Rust-native, modern implementation of `socat` with full legacy compatibility while offering a simpler, AI-friendly command model.

Core principles:

- Compatibility-first: preserve legacy address syntax
- Modern UX: provide URI-based simple mode
- Cross-platform parity: same command shape on Windows/macOS/Linux
- Engineering quality: testable, observable, maintainable

## 2. Features Implemented Today

### 2.1 CLI and Runtime

- Binaries: `socat` and `socat-rs`
- Two command styles:
  - legacy: `socat <ADDR1> <ADDR2>`
  - simple: `socat link --from <uri> --to <uri>`
- Planning and validation commands:
  - `socat plan --from <addr> --to <addr>`
  - `socat validate --from <addr> --to <addr>`
- Connectivity check command:
  - `socat check <address>`
- Tunnel command:
  - `socat tunnel --via <proxy-uri> --to <host:port> [--from stdio://]`
  - supports multi-hop chains with repeated `--via`
- Introspection commands:
  - `socat --json explain <address>`
  - `socat --json inventory`
- `--dry-run` parse-and-plan mode
- Runtime option engine v1:
  - `connect-timeout` (ms / `s` / `ms`)
  - `retry`
  - `retry-delay` (ms / `s` / `ms`)
  - `retry-backoff` (`constant` / `exponential`)
  - `retry-max-delay`
  - `tls-verify`
  - `tls-sni`
- Built-in profile presets:
  - `dev`
  - `prod`
  - `lan`
  - `wan`
- JSON run-report for executed link operations
- Optional JSON run-report file output (`--report-file`)
- Prometheus metrics endpoint via `--metrics-bind`

### 2.2 Endpoint Capabilities Available

- `stdio`
- `tcp-connect` / `tcp-listen`
- `udp-connect` / `udp-listen`
- `tls-connect` / `tls-listen`
  - `tls-listen` requires:
    - `SOCAT_RS_TLS_PKCS12`
    - `SOCAT_RS_TLS_PASSWORD` (optional)
- `socks5-connect`
- `socks4-connect`
- `socks4a-connect`
- `http-proxy-connect`
- `exec` / `system` / `shell`
- `unix-connect` / `unix-listen` (unix platforms)
- `file`
- `named-pipe-connect` (Windows)

### 2.3 Engineering Foundation

- Workspace split:
  - `socat-rs-core`
  - `socat-rs-compat`
- CI on Linux/macOS/Windows
- Parser tests + core runtime tests

## 3. Reliability Status

Already validated:

- `cargo check` passes
- `cargo test --workspace` passes
- UDP roundtrip runtime test passes

Current constraints:

- This is still a staged compatibility build, not full upstream parity yet
- Most legacy options (about 892 keywords upstream) are not fully implemented yet
- Several advanced protocol families are still pending

## 4. Major Gaps Remaining

### 4.1 Network / Protocol Families

- SOCKS4 / SOCKS4A / SOCKS5 bind/listen semantics
- PROXY / HTTP CONNECT bind/listen semantics
- DTLS
- SCTP / DCCP / UDPLITE
- RAW IP
- VSOCK

### 4.2 System Features

- Full PTY / termios semantics
- TUN / interface-related features
- POSIXMQ

### 4.3 Compatibility Engine

- Complete option engine implementation (about 892 option names)
- Group/phase-equivalent option application model
- Full behavioral replay against upstream-style test scenarios

## 5. Detailed Execution Plan

### Phase A: High-priority connectivity completion

- SOCKS5 / PROXY / HTTP CONNECT
- Extended TLS option compatibility (verify, SNI, protocol controls)
- UDP semantic expansion (`recv` / `sendto` / datagram compatibility)

### Phase B: Process and terminal behavior

- Deeper EXEC/SYSTEM/SHELL parity
- PTY and termios compatibility layer
- Child process lifecycle and signal propagation policy

### Phase C: Advanced transport families

- SCTP / DCCP / UDPLITE
- RAWIP / VSOCK / TUN
- Runtime feature probing and platform fallback behavior

### Phase D: Full option system parity

- Typed option IR
- Group + phase application engine
- Alias normalization and conflict detection
- Expand from v1 runtime options to legacy option groups

### Phase E: Validation and release hardening

- Port/adapt upstream-style regression matrix
- Cross-platform behavior baselines
- Stability and performance benchmarking

## 6. Operational Guidance

### 6.1 What is production-usable now

- Standard TCP/UDP relay scenarios
- Basic TLS client/server relays
- Basic process bridging (`exec/system/shell`)

### 6.2 Recommended rollout path

- Start with controlled canary deployments
- Add explicit timeout/retry policy in critical paths
- Integrate `--json` output into automation pipelines

## 7. Completion Criteria

“Fully implemented” means:

- Full address-family coverage aligned with upstream practical surface
- Option behavior compatibility is operationally equivalent
- Upstream-style key regression suites pass reliably
- Same command ergonomics across Linux/macOS/Windows
