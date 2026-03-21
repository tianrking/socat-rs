# socat-rs 全功能详解（当前已实现范围）

本文档完整说明 `socat-rs` 当前已实现的全部功能，包括用途、适用需求、命令写法和注意事项。

## 1. 产品定位

`socat-rs` 重点是：

- 跨平台实用转发/隧道能力（Windows/macOS/Linux）
- 对人和 AI 都友好的 CLI
- 结构化计划/校验/报告输出
- 可靠运行策略（超时、重试、退避）

它不是“历史 socat 所有边角行为的 1:1 克隆”。

## 2. 命令面（全部已实现）

## 2.1 `link`

适用需求：

- 在两个端点之间建立直接双向转发。

命令：

```bash
socat link --from <ADDRESS_OR_URI> --to <ADDRESS_OR_URI>
```

示例：

```bash
socat link --from tcp-listen://0.0.0.0:8080 --to stdio://
socat link --from "tls://example.com:443?tls-verify=true" --to stdio://
```

## 2.2 `tunnel`

适用需求：

- 使用代理完成转发，且希望命令尽量简单。
- 支持单跳和多跳代理链。

命令：

```bash
socat tunnel --via <proxy-uri>... --to <host:port> [--from stdio://]
```

示例：

```bash
socat tunnel --via socks5://127.0.0.1:1080 --to example.com:443
socat tunnel --via socks5://127.0.0.1:1080 --via http-proxy://127.0.0.1:8080 --to api.example.com:443
```

说明：

- 重复 `--via` 会按顺序形成代理链。
- `--via` 也支持逗号分隔多个 hop。
- 最后一跳如果是 `SOCKS4`，目标必须是 IPv4。

## 2.3 `plan`

适用需求：

- 只生成解析计划，不执行连接。

命令：

```bash
socat --json plan --from <ADDRESS_OR_URI> --to <ADDRESS_OR_URI>
```

## 2.4 `validate`

适用需求：

- 校验一对端点输入是否可解析、可组成计划。

命令：

```bash
socat --json validate --from <ADDRESS_OR_URI> --to <ADDRESS_OR_URI>
```

## 2.5 `check`

适用需求：

- 快速探测单端点连通性。

命令：

```bash
socat --json check <ADDRESS_OR_URI>
```

输出含 `ok`、`latency_ms` 和错误信息。

## 2.6 `explain`

适用需求：

- 查看单个地址最终解析为哪种端点和选项。

命令：

```bash
socat --json explain <ADDRESS_OR_URI>
```

## 2.7 `inventory`

适用需求：

- 查看实现清单与兼容性统计计数。

命令：

```bash
socat --json inventory
```

## 3. 全局参数

- `--json`：输出机器可读结构
- `--dry-run`：只做解析/计划，不执行
- `--profile dev|prod|lan|wan`：套用默认运行策略
- `--metrics-bind <host:port>`：暴露 Prometheus 指标
- `--report-file <path>`：把运行报告写入文件（执行态）

## 4. 端点能力（全部已实现）

- `stdio`
- `tcp-connect` / `tcp-listen`
- `udp-connect` / `udp-listen`
- `tls-connect` / `tls-listen`
- `socks4-connect`
- `socks4a-connect`
- `socks5-connect`
- `http-proxy-connect`
- `proxy-chain`（多跳 tunnel 运行时能力）
- `exec` / `system` / `shell`
- `unix-connect` / `unix-listen`（unix）
- `file`
- `named-pipe-connect`（windows）

## 5. 地址语法

## 5.1 Simple URI 模式

已支持 scheme：

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
- `exec://?cmd=...`（或 path 形式）
- `system://?cmd=...`（或 path 形式）
- `shell://?cmd=...`（或 path 形式）
- `unix:///path/to.sock`
- `unix-listen:///path/to.sock`
- `file:///path/to.file`
- `npipe://./pipe/name`

## 5.2 Legacy 模式

双地址调用：

```bash
socat <ADDR1> <ADDR2>
```

已支持关键词族：

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

## 6. 运行时选项引擎（已实现）

支持选项（URI query 或 legacy 逗号选项）：

- `connect-timeout` / `connect_timeout` / `timeout`
- `retry`
- `retry-delay` / `retry_delay`
- `retry-backoff` / `retry_backoff`：`constant` 或 `exponential`
- `retry-max-delay` / `retry_max_delay`
- `tls-verify` / `tls_verify` / `verify`：布尔值
- `tls-sni` / `tls_sni` / `sni`
- `tls-ca-file` / `tls_ca_file` / `cafile`
- `tls-client-pkcs12` / `tls_client_pkcs12`
- `tls-client-password` / `tls_client_password`

布尔值支持：

- 真值：`1`, `true`, `yes`, `on`
- 假值：`0`, `false`, `no`, `off`

## 7. Profile 预设

Profile 只填充“未显式设置”的选项（显式参数优先）：

- `dev`
- `prod`
- `lan`
- `wan`

## 8. 可观测性

## 8.1 JSON 输出

- 计划输出（`plan`、`validate`、`--dry-run`）
- explain 输出
- check 输出
- 运行报告（执行 `link`/`tunnel`/legacy 转发且启用 `--json`）

## 8.2 Prometheus 指标

启用 `--metrics-bind` 后可获得：

- `socat_rs_connections_total`
- `socat_rs_connections_failed_total`
- `socat_rs_bytes_left_to_right_total`
- `socat_rs_bytes_right_to_left_total`

## 9. 平台说明

- Windows：
  - 支持 named pipe connect
  - 不支持 unix domain socket
- Unix（Linux/macOS）：
  - 支持 unix domain socket connect/listen
  - 不支持 Windows named pipe

## 10. 常见需求 -> 推荐命令

- 本地 TCP 监听并输出到终端：
  - `socat link --from tcp-listen://0.0.0.0:8080 --to stdio://`
- 通过单跳 SOCKS5 连接目标：
  - `socat tunnel --via socks5://127.0.0.1:1080 --to example.com:443`
- 通过多跳代理链连接目标：
  - `socat tunnel --via socks5://127.0.0.1:1080 --via http-proxy://127.0.0.1:8080 --to api.example.com:443`
- 快速健康检查：
  - `socat --json check "tcp://127.0.0.1:8080?connect-timeout=1s"`
- 自动化记录执行结果：
  - `socat --json --report-file ./run-report.json link --from ... --to ...`
