# socat-rs（中文说明）

`socat-rs` 是一个使用 Rust 重写的 `socat` 项目，目标是：

- 保持对 legacy `socat` 命令形态的兼容
- 提供更现代、更清晰、AI 生成更友好的命令格式
- 支持 Windows / macOS / Linux 统一命令体验

## 当前已实现（持续增加）

- `stdio`
- `tcp-connect` / `tcp-listen`
- `udp-connect` / `udp-listen`
- `tls-connect` / `tls-listen`
- `socks4-connect`
- `socks4a-connect`
- `socks5-connect`
- `http-proxy-connect`
  - 支持 URI 用户密码：`socks5://user:pass@proxy:1080?target=host:443`
  - 支持 URI 用户密码：`http-proxy://user:pass@proxy:8080?target=host:443`
- `exec` / `system` / `shell`
- `unix-connect` / `unix-listen`（Unix）
- `file`
- `named-pipe-connect`（Windows，`npipe://` / `PIPE:` / `NPIPE:`）
- 运行时选项引擎 v1：
  - `connect-timeout`
  - `retry`
  - `retry-delay`
  - `retry-backoff`（`constant` / `exponential`）
  - `retry-max-delay`
  - `tls-verify`
  - `tls-sni`
- 内置 profile：
  - `dev`
  - `prod`
  - `lan`
  - `wan`
- 运行态 `--json` 会输出 link 执行报告（run-report）
- 可将 JSON 运行报告写入文件：`--report-file <path>`
- Prometheus 指标端点：`--metrics-bind <host:port>`
- 运行环境自检：`socat doctor`
- 面向 Agent 的 JSON 输入：`socat run --input-json <文件或->`

## 常用命令

```bash
# 兼容模式
socat TCP-LISTEN:8080 STDIO

# 简化模式
socat link --from tcp-listen://0.0.0.0:8080 --to stdio://
socat link --from npipe://./pipe/socat-rs --to tcp://127.0.0.1:9000
socat link --from "tcp://127.0.0.1:9000?connect-timeout=2s&retry=3&retry-delay=500ms" --to stdio://
socat link --from "tcp://127.0.0.1:9000?retry=5&retry-backoff=exponential&retry-max-delay=8s" --to stdio://
socat link --from "tls://example.com:443?tls-verify=false&sni=alt.example.com" --to stdio://
socat link --profile prod --from tcp://example.com:443 --to stdio://
socat tunnel --via socks5://127.0.0.1:1080 --to example.com:443
socat tunnel --from stdio:// --via http-proxy://u:p@127.0.0.1:8080 --to api.example.com:443
socat tunnel --via socks5://127.0.0.1:1080 --via http-proxy://127.0.0.1:8080 --to api.example.com:443

# 计划/校验模式
socat --json plan --from "TCP:127.0.0.1:9000,retry=2" --to STDIO
socat --json validate --profile wan --from tcp://api.example.com:443 --to stdio://
socat --json check "tcp://127.0.0.1:8080?connect-timeout=1s"

# 机器可读清单
socat --json inventory
socat --json explain "TCP:127.0.0.1:9000,connect-timeout=2000,retry=3,retry-delay=500ms"
socat --json --metrics-bind 0.0.0.0:9464 link --from tcp://127.0.0.1:9000 --to stdio://
socat --json --report-file ./run-report.json link --from tcp://127.0.0.1:9000 --to stdio://
```

## AI / Agent 统一 JSON 协议

开启 `--json` 后，所有命令统一返回以下结构：

- `schema_version`
- `ok`
- `command`
- `input`
- `plan`
- `result`
- `error`
- `next_actions`
- `version`
- `timestamp`

稳定错误码（可直接用于 Agent 分支逻辑）：

- `E_ADDR_PARSE`
- `E_CONNECT_TIMEOUT`
- `E_TLS_ENV`
- `E_PROXY_AUTH`

`plan` 字段中会包含：

- `normalized_endpoints`
- `executable_command`

## JSON 输入模式（Agent 推荐）

支持结构化输入，避免命令拼接错误：

```bash
cat <<'JSON' | socat run --input-json -
{
  "mode": "plan",
  "from": "tcp://127.0.0.1:8080",
  "to": "stdio://",
  "json": true
}
JSON
```

`mode` 支持：

- `link`
- `tunnel`
- `plan`
- `validate`
- `check`
- `explain`
- `inventory`
- `doctor`
- `legacy`

## 文档

- 英文总览：`README.md`
- 中文详细状态与计划：`docs/FEATURE_STATUS.zh-CN.md`
- 英文详细状态与计划：`docs/FEATURE_STATUS.en.md`
- v1-ready（英文）：`docs/V1_READY.en.md`
- v1-ready（中文）：`docs/V1_READY.zh-CN.md`
- 全功能详解（英文）：`docs/ALL_FEATURES.en.md`
- 全功能详解（中文）：`docs/ALL_FEATURES.zh-CN.md`
- 兼容路线图：`docs/compatibility-roadmap.md`

## TLS 监听说明

`tls-listen` 需要设置：

- `SOCAT_RS_TLS_PKCS12=<identity.p12路径>`
- `SOCAT_RS_TLS_PASSWORD=<可选密码>`

## 构建与测试

```bash
cargo check
cargo test --workspace
cargo run --bin socat -- --help
cargo test --test json_protocol_smoke
```

快速环境自检：

```bash
cargo run --bin socat -- doctor
cargo run --bin socat -- --json doctor
```

## 跨平台打包（x86_64 + arm）

本地打包示例（先完成目标平台编译）：

```bash
rustup target add x86_64-unknown-linux-gnu
cargo build --release --workspace --target x86_64-unknown-linux-gnu
PACKAGE_VERSION=local ./scripts/package-artifact.sh x86_64-unknown-linux-gnu
```

CI 发布工作流（`.github/workflows/release.yml`）会自动构建并打包：

- Ubuntu: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
- Windows: `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`
- macOS: `x86_64-apple-darwin`, `aarch64-apple-darwin`
