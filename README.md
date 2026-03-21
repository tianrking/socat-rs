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
- built-in profiles:
  - `dev`
  - `prod`
  - `lan`
  - `wan`

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
socat link --profile prod --from tcp://example.com:443 --to stdio://
```

## AI-friendly workflow

Use `--dry-run` and `--json` to make planning deterministic:

```bash
socat --dry-run --json link --from tcp://127.0.0.1:80 --to stdio://
socat --json plan --from "TCP:127.0.0.1:9000,retry=2" --to STDIO
socat --json validate --profile wan --from tcp://api.example.com:443 --to stdio://
socat --json explain "TCP-LISTEN:8080"
socat --json explain "TCP:127.0.0.1:9000,connect-timeout=2000,retry=3,retry-delay=500ms"
socat --json inventory
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
- `README.zh-CN.md`

## Build

```bash
cargo build
cargo test
cargo run -- --help
```

## Legacy inventory extraction

Given upstream source at `../socat`:

```bash
./scripts/extract_legacy_inventory.sh ../socat
```
