# socat-rs v1-ready 状态（实用功能对齐）

该文档用于定义本项目的 v1-ready 范围，目标是覆盖高频实用场景。

## 已纳入 v1-ready

- 核心转发：
  - 双向流量中继
  - JSON 干跑计划输出
  - JSON 运行报告输出
  - 可选运行报告文件输出（`--report-file`）
- 端点能力：
  - `stdio`
  - `tcp-connect` / `tcp-listen`
  - `udp-connect` / `udp-listen`
  - `tls-connect` / `tls-listen`
  - `socks4-connect` / `socks4a-connect` / `socks5-connect`
  - `http-proxy-connect`
  - 代理链运行时（`ProxyChain`）
  - `exec` / `system` / `shell`
  - `unix-connect` / `unix-listen`（unix）
  - `file`
  - `named-pipe-connect`（windows）
- 友好 CLI：
  - `link`
  - `tunnel`（支持单跳与多跳 `--via`）
  - `plan`
  - `validate`
  - `check`
  - `explain`
  - `inventory`
- 运行时策略选项：
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
- 可观测性：
  - Prometheus 指标端点（`--metrics-bind`）
  - 连接数与字节数计数器

## 明确不在 v1-ready 范围

- 与上游 `socat` 所有长尾 option/地址族的完全等价
- PTY/termios 完整等价
- DTLS/SCTP/DCCP/UDPLITE/RAWIP/VSOCK/TUN 全量实现

## v1-ready 验收项

- `cargo test --workspace` 通过
- `cargo clippy --workspace --all-targets -- -D warnings` 通过
- linux/macOS/windows CI 全绿
