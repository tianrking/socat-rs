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
- `exec` / `system` / `shell`
- `unix-connect` / `unix-listen`（Unix）
- `file`

## 常用命令

```bash
# 兼容模式
socat TCP-LISTEN:8080 STDIO

# 简化模式
socat link --from tcp-listen://0.0.0.0:8080 --to stdio://

# 机器可读清单
socat --json inventory
```

## 文档

- 英文总览：`README.md`
- 中文详细状态与计划：`docs/FEATURE_STATUS.zh-CN.md`
- 英文详细状态与计划：`docs/FEATURE_STATUS.en.md`
- 兼容路线图：`docs/compatibility-roadmap.md`

## TLS 监听说明

`tls-listen` 需要设置：

- `SOCAT_RS_TLS_PKCS12=<identity.p12路径>`
- `SOCAT_RS_TLS_PASSWORD=<可选密码>`

## 构建与测试

```bash
cargo check
cargo test --workspace
cargo run -- --help
```
