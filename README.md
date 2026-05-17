# Apex Terminal

**Next-Generation Integrated Offensive Environment for Kali Linux**

Apex Terminal synthesizes the high-performance rendering of Alacritty, the persistent session management of Tmux, the collaborative capabilities of Warp, and the modular microservice orchestration of modern C2 frameworks into a singular, cohesive, AI-augmented platform.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    APEX TERMINAL                         │
├─────────────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────┐ │
│  │ VTE Core │  │  WGPU    │  │  Native  │  │  AI    │ │
│  │  Parser  │  │ Renderer │  │Multiplexer│  │MCP Layer│ │
│  └──────────┘  └──────────┘  └──────────┘  └────────┘ │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────┐ │
│  │  Shell   │  │C2 Framework│ │ Collab   │  │  Lua   │ │
│  │Stabilizer│  │Connectors │  │Workspaces│  │  Config│ │
│  └──────────┘  └──────────┘  └──────────┘  └────────┘ │
└─────────────────────────────────────────────────────────┘
```

## Quick Start

```bash
# Build
cargo build --release

# Run as client (GUI)
cargo run

# Run as background server
cargo run -- --server

# With specific config
cargo run -- --config ~/.config/apex/config.toml
```

## Project Structure

| Crate | Description |
|-------|-------------|
| `vte-core` | High-throughput ANSI/VT100 escape sequence parser |
| `apex-renderer` | GPU-accelerated renderer via wgpu/WebGPU |
| `apex-server` | Persistent background session server |
| `apex-mux` | Native terminal multiplexer |
| `apex-pty` | PTY management and shell stabilization |
| `apex-ai` | AI middleware with MCP protocol |
| `apex-c2` | C2 framework connectors (Sliver, Havoc, Mythic, Empire) |
| `apex-collab` | Collaborative workspace management |
| `apex-config` | Lua-based configuration engine |
| `apex-protocol` | Client-server wire protocol |

## Development

```bash
# Build
make build

# Run checks
make check

# Run linter
make lint

# Format code
make fmt

# Run tests
make test

# Run in development mode
make dev
```
