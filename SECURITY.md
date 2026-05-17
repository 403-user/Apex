# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

Apex Terminal is an offensive security tool designed for authorized penetration testing.
If you discover a security vulnerability, please report it privately.

**Do not** report security vulnerabilities through public GitHub issues.

Instead, email: **security@apex-terminal.io**

You should receive a response within 48 hours. If not, follow up.

## What to Include

- Description of the vulnerability
- Steps to reproduce
- Affected versions
- Potential impact
- Suggested fix (if known)

## Scope

The following are considered in-scope for security reports:

- Remote code execution in the AI/MCP sandbox
- Privilege escalation via the terminal multiplexer
- Unsafe deserialization in the wire protocol
- Information disclosure in collaborative sessions
- Command injection in shell stabilization

## OPSEC Considerations

As an offensive security tool, Apex Terminal follows these security principles:

1. **No telemetry** - All AI inference is 100% local. No data leaves your machine.
2. **Sandboxed AI** - The MCP command sandbox enforces permission levels (ReadOnly → Full).
3. **Encrypted C2** - All C2 framework connections use TLS/gRPC/WebSocket Secure.
4. **Minimal privileges** - The systemd service drops capabilities and uses `NoNewPrivileges=true`.
5. **Tamper tracking** - All modifications to target systems are tracked and revertible.

## Responsible Disclosure

We follow a 90-day disclosure window. We will notify you when a fix is released.
