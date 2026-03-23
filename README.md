# socat-rs (`socat` in Rust)

A modern Rust rewrite project for `socat`, built around two goals:

- Full legacy capability coverage (compatibility-first)
- A simpler, AI-friendly and human-friendly command model

Project/package name: `socat-rs`  
Binary names: `socat` (compat) and `socat-rs` (explicit)

## Status

This repository already includes:

- Cross-platform async core (`tokio`) for Linux/macOS/Windows
- Legacy input path: `socat <ADDR1> <ADDR2>`
- Simpler path: `socat link --from <URI> --to <URI>`
- Planning path: `socat plan --from <ADDR> --to <ADDR>`
- Validation path: `socat validate --from <ADDR> --to <ADDR>`
- Tunnel path: `socat tunnel --via <proxy-uri> --to <host:port> [--from stdio://]`
- Check path: `socat check <address>`
- CI for Linux/macOS/Windows
- Legacy inventory extraction script from upstream `socat` source

Current implemented endpoint families:

- `stdio`
- `tcp-connect`
- `tcp-listen`
- `udp-connect`
- `udp-listen`
- `tls-connect`
- `tls-listen` (requires `SOCAT_RS_TLS_PKCS12`)
- `socks4-connect`
- `socks4a-connect`
- `socks5-connect`
- `http-proxy-connect`
  - supports URI auth: `socks5://user:pass@proxy:1080?target=host:443`
  - supports URI auth: `http-proxy://user:pass@proxy:8080?target=host:443`
- `exec`
- `system`
- `shell`
- `unix-connect` (unix only)
- `unix-listen` (unix only)
- `file`
- `named-pipe-connect` (Windows, `npipe://` / `PIPE:` / `NPIPE:`)
- runtime option engine v1:
  - `connect-timeout`
  - `retry`
  - `retry-delay`
  - `retry-backoff` (`constant` / `exponential`)
  - `retry-max-delay`
  - `tls-verify`
  - `tls-sni`
- built-in profiles:
  - `dev`
  - `prod`
  - `lan`
  - `wan`
- run-report JSON for executed links (`--json`)
- JSON report file output: `--report-file <path>`
- Prometheus metrics endpoint: `--metrics-bind <host:port>`

## Why two command styles

`legacy`: keep compatibility with classic `socat` grammar.

```bash
socat TCP-LISTEN:8080 STDIO
socat STDIO TCP:127.0.0.1:8080
```

`simple`: predictable and easy for AI tools to generate.

```bash
socat link --from tcp-listen://0.0.0.0:8080 --to stdio://
socat link --from stdio:// --to tcp://127.0.0.1:8080
socat link --from npipe://./pipe/socat-rs --to tcp://127.0.0.1:9000
socat link --from "tcp://127.0.0.1:9000?connect-timeout=2s&retry=3&retry-delay=500ms" --to stdio://
socat link --from "tcp://127.0.0.1:9000?retry=5&retry-backoff=exponential&retry-max-delay=8s" --to stdio://
socat link --from "tls://example.com:443?tls-verify=false&sni=alt.example.com" --to stdio://
socat link --profile prod --from tcp://example.com:443 --to stdio://
socat tunnel --via socks5://127.0.0.1:1080 --to example.com:443
socat tunnel --from stdio:// --via http-proxy://u:p@127.0.0.1:8080 --to api.example.com:443
socat tunnel --via socks5://127.0.0.1:1080 --via http-proxy://127.0.0.1:8080 --to api.example.com:443
```

## AI-friendly workflow

Use `--dry-run` and `--json` to make planning deterministic:

```bash
socat --dry-run --json link --from tcp://127.0.0.1:80 --to stdio://
socat --json plan --from "TCP:127.0.0.1:9000,retry=2" --to STDIO
socat --json validate --profile wan --from tcp://api.example.com:443 --to stdio://
socat --json check "tcp://127.0.0.1:8080?connect-timeout=1s"
socat --json explain "TCP-LISTEN:8080"
socat --json explain "TCP:127.0.0.1:9000,connect-timeout=2000,retry=3,retry-delay=500ms"
socat --json inventory
socat --json --metrics-bind 0.0.0.0:9464 link --from tcp://127.0.0.1:9000 --to stdio://
socat --json --report-file ./run-report.json link --from tcp://127.0.0.1:9000 --to stdio://
```

## Architecture

- `src/main.rs`: binary entrypoint, command name remains `socat`
- `crates/socat-rs-core`: runtime core
  - `spec.rs`: endpoint grammar parsing (legacy + URI)
  - `endpoint.rs`: endpoint openers and platform branches
  - `relay.rs`: bidirectional streaming relay
  - `error.rs`: typed errors
- `crates/socat-rs-compat`: compatibility metadata and feature counters
- `docs/`: analysis and roadmap
- `.github/workflows/ci.yml`: CI for all target OS

## Compatibility strategy

Upstream `socat` has very broad surface area (hundreds of options and many address families).
This project keeps a strict compatibility ledger and lands support incrementally in grouped modules.

See:

- `docs/socat-analysis.md`
- `docs/compatibility-roadmap.md`
- `docs/FEATURE_STATUS.zh-CN.md`
- `docs/FEATURE_STATUS.en.md`
- `docs/V1_READY.en.md`
- `docs/V1_READY.zh-CN.md`
- `docs/ALL_FEATURES.en.md`
- `docs/ALL_FEATURES.zh-CN.md`
- `README.zh-CN.md`

## Build

```bash
cargo build
cargo test
cargo run -- --help
```

Quick environment check:

```bash
cargo run --bin socat -- doctor
cargo run --bin socat -- --json doctor
```

## Release Packages (x86_64 + arm)

Local package creation (after building a target):

```bash
rustup target add x86_64-unknown-linux-gnu
cargo build --release --workspace --target x86_64-unknown-linux-gnu
PACKAGE_VERSION=local ./scripts/package-artifact.sh x86_64-unknown-linux-gnu
```

CI release workflow (`.github/workflows/release.yml`) builds and packages:

- Ubuntu: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
- Windows: `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`
- macOS: `x86_64-apple-darwin`, `aarch64-apple-darwin`

## Legacy inventory extraction

Given upstream source at `../socat`:

```bash
./scripts/extract_legacy_inventory.sh ../socat
```
