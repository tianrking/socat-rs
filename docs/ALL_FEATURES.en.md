# socat-rs Full Feature Guide (Current Implemented Scope)

This document describes all features currently implemented in `socat-rs`, with usage, typical requirements, and command examples.

## 1. Product Positioning

`socat-rs` focuses on:

- Practical cross-platform relay/tunnel workflows (Windows/macOS/Linux)
- AI-friendly and human-friendly CLI
- Structured planning/validation/reporting output
- Reliable runtime controls (timeout/retry/backoff)

This is not a full historical 1:1 clone of every legacy `socat` edge behavior.

## 2. Command Surface (All Implemented Commands)

## 2.1 `link`

Use case:

- Direct relay between two endpoints.

Command:

```bash
socat link --from <ADDRESS_OR_URI> --to <ADDRESS_OR_URI>
```

Examples:

```bash
socat link --from tcp-listen://0.0.0.0:8080 --to stdio://
socat link --from "tls://example.com:443?tls-verify=true" --to stdio://
```

## 2.2 `tunnel`

Use case:

- Proxy-based forwarding with simple one-line command.
- Single-hop and multi-hop proxy chains.

Command:

```bash
socat tunnel --via <proxy-uri>... --to <host:port> [--from stdio://]
```

Examples:

```bash
socat tunnel --via socks5://127.0.0.1:1080 --to example.com:443
socat tunnel --via socks5://127.0.0.1:1080 --via http-proxy://127.0.0.1:8080 --to api.example.com:443
```

Notes:

- Repeated `--via` builds an ordered proxy chain.
- `--via` also supports comma-separated values in one arg.
- Last `SOCKS4` hop requires IPv4 target.

## 2.3 `plan`

Use case:

- Build and inspect resolved plans without execution.

Command:

```bash
socat --json plan --from <ADDRESS_OR_URI> --to <ADDRESS_OR_URI>
```

## 2.4 `validate`

Use case:

- Parse and validate two endpoints as a pair (no data relay).

Command:

```bash
socat --json validate --from <ADDRESS_OR_URI> --to <ADDRESS_OR_URI>
```

## 2.5 `check`

Use case:

- Fast connectivity probe of a single endpoint.

Command:

```bash
socat --json check <ADDRESS_OR_URI>
```

Output includes `ok`, `latency_ms`, and optional error text.

## 2.6 `explain`

Use case:

- Parse one address and print resolved endpoint/options.

Command:

```bash
socat --json explain <ADDRESS_OR_URI>
```

## 2.7 `inventory`

Use case:

- Feature and compatibility counters.

Command:

```bash
socat --json inventory
```

## 3. Global Options

- `--json`: machine-readable output
- `--dry-run`: parse/plan only, no runtime relay
- `--profile dev|prod|lan|wan`: apply default runtime policy
- `--metrics-bind <host:port>`: expose Prometheus metrics endpoint
- `--report-file <path>`: write run-report JSON to file (for executed `link`/`tunnel`/legacy pair)

## 4. Endpoint Families (Implemented)

- `stdio`
- `tcp-connect` / `tcp-listen`
- `udp-connect` / `udp-listen`
- `tls-connect` / `tls-listen`
- `socks4-connect`
- `socks4a-connect`
- `socks5-connect`
- `http-proxy-connect`
- `proxy-chain` (runtime chain for multi-hop tunnel)
- `exec` / `system` / `shell`
- `unix-connect` / `unix-listen` (unix)
- `file`
- `named-pipe-connect` (windows)

## 5. Address Syntax

## 5.1 Simple URI mode

Supported schemes:

- `stdio://`
- `tcp://host:port`
- `tcp-listen://host:port`
- `udp://host:port`
- `udp-listen://host:port`
- `tls://host:port`
- `tls-listen://host:port`
- `socks4://proxy:port?target=host:port`
- `socks4a://proxy:port?target=host:port`
- `socks5://[user:pass@]proxy:port?target=host:port`
- `http-proxy://[user:pass@]proxy:port?target=host:port`
- `exec://?cmd=...` (or path form)
- `system://?cmd=...` (or path form)
- `shell://?cmd=...` (or path form)
- `unix:///path/to.sock`
- `unix-listen:///path/to.sock`
- `file:///path/to.file`
- `npipe://./pipe/name`

## 5.2 Legacy mode

Two-address legacy call:

```bash
socat <ADDR1> <ADDR2>
```

Supported keyword families include:

- `STDIO` / `-`
- `TCP*`, `TCP-LISTEN*`
- `UDP*`, `UDP-LISTEN*`
- `SSL*`, `OPENSSL*`
- `SOCKS4*`, `SOCKS4A*`, `SOCKS5*`
- `PROXY*`
- `EXEC`, `SYSTEM`, `SHELL`
- `UNIX*`, `UNIX-LISTEN*`
- `FILE` / `OPEN` / `GOPEN`
- `PIPE` / `NPIPE`

## 6. Runtime Option Engine (Implemented)

Supported options (URI query or legacy comma options):

- `connect-timeout` / `connect_timeout` / `timeout`
- `retry`
- `retry-delay` / `retry_delay`
- `retry-backoff` / `retry_backoff`: `constant` or `exponential`
- `retry-max-delay` / `retry_max_delay`
- `tls-verify` / `tls_verify` / `verify`: boolean
- `tls-sni` / `tls_sni` / `sni`
- `tls-ca-file` / `tls_ca_file` / `cafile`
- `tls-client-pkcs12` / `tls_client_pkcs12`
- `tls-client-password` / `tls_client_password`

Boolean accepted values:

- true-like: `1`, `true`, `yes`, `on`
- false-like: `0`, `false`, `no`, `off`

## 7. Profiles

Profiles fill missing runtime options only (explicit values win).

- `dev`
- `prod`
- `lan`
- `wan`

## 8. Observability

## 8.1 JSON outputs

- Plan output (`plan`, `validate`, `--dry-run`)
- Explain output
- Check output
- Run report (`link`/`tunnel`/legacy run, when `--json`)

## 8.2 Prometheus metrics

When `--metrics-bind` is enabled:

- `socat_rs_connections_total`
- `socat_rs_connections_failed_total`
- `socat_rs_bytes_left_to_right_total`
- `socat_rs_bytes_right_to_left_total`

## 9. Platform Notes

- Windows:
  - named pipe connect is supported
  - unix domain sockets unavailable
- Unix (Linux/macOS):
  - unix domain socket connect/listen supported
  - Windows named pipes unavailable

## 10. Typical Requirements -> Recommended Command

- Forward local TCP to terminal:
  - `socat link --from tcp-listen://0.0.0.0:8080 --to stdio://`
- Connect through single SOCKS5 proxy:
  - `socat tunnel --via socks5://127.0.0.1:1080 --to example.com:443`
- Connect through proxy chain:
  - `socat tunnel --via socks5://127.0.0.1:1080 --via http-proxy://127.0.0.1:8080 --to api.example.com:443`
- Check endpoint quickly:
  - `socat --json check "tcp://127.0.0.1:8080?connect-timeout=1s"`
- Automation-friendly run archive:
  - `socat --json --report-file ./run-report.json link --from ... --to ...`
