# Changelog

## 2026-03-23

### Architecture
- Refactored `socat-rs-core` entry logic into clearer modules:
  - `app.rs`: command dispatch and runtime flow
  - `cli.rs`: CLI schema and profile defaults
  - `lib.rs`: lean module/export entry
- Kept existing behavior compatibility while making command handling easier to extend.

### Usability
- Added `doctor` command:
  - `socat doctor`
  - `socat --json doctor`
- `doctor` reports runtime platform information, socket capability hints, TLS listener env readiness, and recommended release targets.
- Added `run --input-json <path|->` for structured machine input.

### Cross-platform Packaging
- Added reusable packaging script: `scripts/package-artifact.sh`
  - Produces `tar.gz` package and `.sha256` checksum.
  - Bundles both binaries: `socat` and `socat-rs`.
- Added release pipeline: `.github/workflows/release.yml`
  - Ubuntu: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
  - Windows: `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`
  - macOS: `x86_64-apple-darwin`, `aarch64-apple-darwin`
  - On tag pushes (`v*`), automatically publishes release assets.

### Documentation
- Updated `README.md` and `README.zh-CN.md` with:
  - `doctor` usage
  - local packaging flow
  - CI target matrix overview
  - unified JSON protocol and input-json mode

### AI/JSON Protocol
- Unified `--json` output envelope across commands:
  - `schema_version`, `ok`, `command`, `input`, `plan`, `result`, `error`, `next_actions`, `version`, `timestamp`
- Added stable error codes:
  - `E_ADDR_PARSE`
  - `E_CONNECT_TIMEOUT`
  - `E_TLS_ENV`
  - `E_PROXY_AUTH`
- Added `plan.executable_command` and `plan.normalized_endpoints`.
- Added JSON protocol smoke tests and CI check:
  - `tests/json_protocol_smoke.rs`
  - `.github/workflows/ci.yml` step: `JSON schema smoke check`
