# Security policy

Please report vulnerabilities privately through GitHub's **Report a vulnerability** button on this repository (Security › Advisories), not in a public issue. Include steps to reproduce and the version or commit.

Scope worth knowing about:
- `chatcraft-server` speaks plain `ws://` and binds to `127.0.0.1` by default. Expose it only behind a TLS-terminating reverse proxy (`wss://`).
- The workspace join token is a shared secret: anyone holding it can sign in with any unused handle. Per-user authentication (passwords, OIDC/SSO) is on the roadmap (`docs/gaps.md`).
- Every input from the network, files and agents is treated as hostile: frames are size-capped, ids and text are validated, and no code path may panic (`AGENTS.md`).
