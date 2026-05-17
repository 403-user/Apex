# Contributing to Apex Terminal

## Code of Conduct

This project adheres to a Code of Conduct. By participating, you agree to maintain a
respectful and inclusive environment. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Getting Started

```bash
# Clone and build
git clone https://github.com/apex-terminal/apex-terminal
cd apex-terminal
make build

# Run tests
make test

# Run linter
make lint
```

## Project Architecture

Apex Terminal is a Cargo workspace with 10 crates:

| Crate | Purpose |
|-------|---------|
| `vte-core` | ANSI/VT100 escape sequence parser |
| `apex-renderer` | GPU-accelerated wgpu renderer |
| `apex-server` | Background session server |
| `apex-mux` | Native terminal multiplexer |
| `apex-pty` | PTY management + shell stabilization |
| `apex-ai` | AI middleware (MCP + LLM + sandbox) |
| `apex-c2` | C2 framework connectors |
| `apex-collab` | Collaborative workspaces |
| `apex-config` | Lua configuration engine |
| `apex-protocol` | Client-server wire protocol |

See `ARCHITECTURE-*.md` in `docs/` for detailed architecture documentation.

## Development Workflow

1. **Fork and branch** from `main`
2. **Make changes** with tests
3. **Run `make check`** to verify compilation
4. **Run `make lint`** to check style
5. **Run `make test`** to verify all tests pass
6. **Open a PR** with a clear description

## Coding Conventions

- **Safe Rust only** - `unsafe` blocks require documented justification
- **Async** - Use `tokio` throughout; prefer `tokio::process::Command` over `std::process::Command`
- **Errors** - Return `anyhow::Result` from public APIs
- **Logging** - Use `tracing` crate (not `log` or `println`)
- **Serde** - Derive `Serialize`/`Deserialize` on all wire types
- **Tests** - Unit tests inline (`#[cfg(test)] mod tests`), integration tests in `tests/`

## Pull Request Checklist

- [ ] Compiles with `cargo check --workspace`
- [ ] All tests pass (`cargo test --workspace`)
- [ ] No clippy warnings (`cargo clippy -- -D warnings`)
- [ ] No `unsafe` code without justification
- [ ] New public APIs have doc comments
- [ ] New features include tests
- [ ] Log messages are structured (use `tracing` fields, not string interpolation)

## Security Notes

- Never commit API keys, tokens, or passwords
- Never send telemetry or user data to external services
- AI inference must always be local (no cloud API calls)
- C2 credentials should use environment variables or config files (not hardcoded)
