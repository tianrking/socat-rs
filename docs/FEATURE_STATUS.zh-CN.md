# socat-rs 当前功能与完整实现计划（中文）

## 1. 项目目标

`socat-rs` 的目标是：在 Rust 上实现对传统 `socat` 的全功能兼容，同时提供更现代、更易读、对 AI 生成命令更友好的命令形式。

核心原则：

- 兼容优先：保留 legacy 地址语法入口
- 现代体验：新增 URI 风格 simple 模式
- 跨平台一致：Windows/macOS/Linux 命令保持一致
- 工程可靠：可测试、可观测、可持续迭代

## 2. 当前已实现功能（已在代码中）

### 2.1 CLI 与运行框架

- 二进制：`socat` 与 `socat-rs`
- 双语法模式：
  - legacy: `socat <ADDR1> <ADDR2>`
  - simple: `socat link --from <uri> --to <uri>`
- 计划与校验命令：
  - `socat plan --from <addr> --to <addr>`
  - `socat validate --from <addr> --to <addr>`
- 隧道命令：
  - `socat tunnel --via <proxy-uri> --to <host:port> [--from stdio://]`
  - 支持重复 `--via` 形成多跳代理链
- 解释与清点：
  - `socat --json explain <address>`
  - `socat --json inventory`
- `--dry-run` 解析演练模式
- 运行时选项引擎 v1：
  - `connect-timeout`（支持 ms / s 后缀）
  - `retry`
  - `retry-delay`（支持 ms / s 后缀）
  - `retry-backoff`（`constant` / `exponential`）
  - `retry-max-delay`
  - `tls-verify`
  - `tls-sni`
- 内置 profile 预设：
  - `dev`
  - `prod`
  - `lan`
  - `wan`
- link 实际执行后可输出 JSON 运行报告（run-report）
- 支持 `--metrics-bind` 暴露 Prometheus 指标

### 2.2 端点能力（已可用）

- `stdio`
- `tcp-connect` / `tcp-listen`
- `udp-connect` / `udp-listen`
- `tls-connect` / `tls-listen`
  - `tls-listen` 需要：
    - `SOCAT_RS_TLS_PKCS12`
    - `SOCAT_RS_TLS_PASSWORD`（可为空）
- `socks5-connect`
- `socks4-connect`
- `socks4a-connect`
- `http-proxy-connect`
- `exec` / `system` / `shell`
- `unix-connect` / `unix-listen`（unix平台）
- `file`
- `named-pipe-connect`（Windows）

### 2.3 工程能力

- Rust workspace 拆分：
  - `socat-rs-core`
  - `socat-rs-compat`
- CI：Linux/macOS/Windows（三平台）
- 基础单元测试与解析测试

## 3. 可靠性现状

已做：

- 编译校验：`cargo check` 通过
- 测试校验：`cargo test --workspace` 通过
- 核心路径测试：UDP 回环读写测试通过

当前限制：

- 仍处于“能力逐步补齐”阶段，尚未达到旧 `socat` 全量行为等价
- 多数 legacy 选项（约 892 关键词）尚未完整实现
- 高阶协议与系统特性未完全落地

## 4. 尚未完成的主要能力

### 4.1 协议与网络栈

- SOCKS4 / SOCKS4A / SOCKS5 的 bind/listen 语义
- PROXY / HTTP CONNECT 的 bind/listen 语义
- DTLS
- SCTP / DCCP / UDPLITE
- RAW IP
- VSOCK

### 4.2 系统能力

- PTY / termios 完整语义
- TUN / interface 相关能力
- POSIXMQ

### 4.3 兼容性核心

- 选项系统全量实现（约 892 option 名称）
- 选项分组/应用阶段（phase）语义等价
- 与 upstream `test.sh` 等价覆盖

## 5. 详细实现路线（执行顺序）

### Phase A：连接链路补齐（高优先）

- SOCKS5 / PROXY / HTTP CONNECT
- TLS 选项增强（证书验证、SNI、协议版本）
- UDP 模式细化（recv/sendto/datagram 语义）

### Phase B：进程与终端语义

- EXEC/SYSTEM/SHELL 行为细化
- PTY 与 termios 兼容层
- 子进程生命周期/信号转发策略

### Phase C：高级协议族

- SCTP/DCCP/UDPLITE
- RAWIP/VSOCK/TUN
- 平台能力探测与回退

### Phase D：选项系统全量化

- 引入 typed option IR
- 实现 group + phase 应用引擎
- 建立 alias 归一化与冲突检测
- 从 v1 运行时选项扩展到 legacy option 分组语义

### Phase E：全量验证与发布

- 迁移/重写 upstream 回归测试矩阵
- 三平台行为一致性基线
- 性能与稳定性压测

## 6. 使用与运维建议

### 6.1 当前可安全使用场景

- 标准 TCP/UDP 转发
- 基础 TLS 客户端/服务端中继
- 基础进程桥接（exec/system/shell）

### 6.2 生产落地建议

- 先在受控环境灰度
- 关键链路加超时与重试策略
- 通过 `--json` 输出接入自动化系统

## 7. 里程碑定义（完成标准）

“全部实现完成”定义为：

- 地址族覆盖达到 upstream 全量可用范围
- option 兼容达到可运行等价（关键行为一致）
- upstream 关键测试集迁移完成并稳定通过
- Linux/macOS/Windows 三平台均具备一致命令体验
