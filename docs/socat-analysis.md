# Upstream `socat` Deep Analysis (for Rust rewrite)

## Scope analyzed

Source path: `/Volumes/ok/Linux_dev_rewrite/socat`

Key files inspected:

- `xioopen.c` (address registration table)
- `xioopts.c` / `xioopts.h` (option registration and typed option model)
- `xiohelp.c` (help / group / phase metadata)
- `doc/socat.yo` (official behavior and lifecycle docs)
- `test.sh` (upstream behavior coverage)

## Quantitative findings

Extracted from the local upstream source:

- Address keywords/aliases: `215`
- Distinct address handlers: `120`
- Recognizable option keywords (name strings): `~892`

Interpretation:

- `socat` is not just "TCP bridge"; it is a large endpoint DSL + option engine.
- Compatibility is primarily a parser + option-application problem, not only transport I/O.

## Core behavior model from upstream docs

`doc/socat.yo` describes four runtime phases:

1. `init`: parse cmdline, initialize logging
2. `open`: open left endpoint, then right endpoint (blocking)
3. `transfer`: bidirectional relay (`select` loop in C implementation)
4. `closing`: half-close/EOF propagation and graceful termination timeout

Rust rewrite must preserve this behavioral contract.

## Internal structure observed in C implementation

- Address families are registered in central table (`addressnames[]`)
- Options are parsed against a global option table (`optionnames[]`)
- Options carry:
  - type (`TYPE_INT`, `TYPE_TIMEVAL`, `TYPE_STRING`, ...)
  - applicability groups (`GROUP_SOCKET`, `GROUP_TCP`, `GROUP_OPENSSL`, ...)
  - application phase (`PREOPEN`, `CONNECT`, `PASTCONNECT`, ...)
- Runtime dispatch is table-driven, not handwritten switch per command example

## Rewrite implications

A professional 2026 rewrite should use:

- Stable IR (intermediate representation) for parsed endpoint specs
- Typed option system with compile-time enums and value decoders
- Per-family adapters implementing common open/apply/validate traits
- Deterministic diagnostics for AI automation (`--json` mode)
- Cross-platform abstraction layer for OS-specific capabilities

## Cross-platform requirement (Windows/macOS/Linux)

The old implementation has strong Unix bias (UNIX sockets, PTY, termios, raw sockets, netlink).
For same command interface across all OS:

- Keep parser and command model identical
- Use feature gates at runtime for unsupported kernel primitives
- Provide equivalent adapters where possible (for example Windows named pipe for some UNIX-socket use cases)
- Return explicit structured errors where platform feature is impossible

## Risks and complexity hotspots

- Option engine parity (`~892` option names, many aliases)
- Protocol breadth (TCP/UDP/SCTP/DCCP/UDPLite/raw IP/SSL/SOCKS/proxy)
- Process/PTY/termios semantics
- Edge-case lifecycle parity (`-t`, `-T`, signal handling, retry/fork/listen semantics)
- Existing shell-driven tests in `test.sh` rely on Unix tools and timing behavior

## Rewrite direction chosen in this repository

- Keep binary name `socat`
- Ship two grammars in one binary:
  - legacy grammar: full compatibility path
  - URI grammar: simple/AI-friendly path
- Build around modular Rust crates to avoid monolith
- Keep a compatibility ledger document and automate inventory extraction

