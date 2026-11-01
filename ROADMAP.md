# ChatCraft roadmap

_Status: 2026-10-31 · revision 1_

**Stage: M0 — foundations (done).** A working workspace end to end: desktop, CLI, MCP and web
clients; a durable WebSocket server; the command registry; measured parity.

| Number | Value |
|---|---|
| Slack/Teams parity (catalogued features with a command) | **64 / 113 (56%)** |
| Engine commands | 65 (+ 9 UI commands) |
| Tests | 65, including hostile-input sweeps over every command and op, and a real-socket end-to-end test |
| Platforms | macOS, Windows, Linux, BSD (native), web (wasm, demo workspace) |

Details: [docs/roadmap.md](docs/roadmap.md) (milestones), [docs/gaps.md](docs/gaps.md) (ranked work
list), [docs/target-app-parity.md](docs/target-app-parity.md) (assessment),
[docs/parity-checklist.md](docs/parity-checklist.md) (generated).

**Direction change (2026-11-01):** following storytold/artcraft#2138, ChatCraft speaks Matrix with
end-to-end encryption instead of its own protocol. See [docs/matrix.md](docs/matrix.md).

## Milestones

| | Milestone | Headline |
|---|---|---|
| ✅ | **M0 Foundations** | Layered workspace, event-sourced hub, journal, server, desktop/CLI/MCP/web, demo workspace |
| 🟡 | **M1-E Matrix + E2EE** | Matrix transport, encrypted private channels and DMs, device store, emoji verification + cross-signing, key backup and recovery — **in progress**: own-device verification, SSO sign-in, Matrix coverage (pins, saved, mute, directory) next |
| ⬜ | **M1 Daily driver** | Files and images, per-user auth, TLS, OS notifications, edit history, typing indicators, unread view, mark unread, reminders |
| ⬜ | **M2 Teams-shaped** | Teams (channel groups), sections, channel tabs, scheduled/priority messages, read receipts, polls, import from Slack/Teams exports |
| ⬜ | **M3 Calls** | Huddles / voice and video calls, screen share (WebRTC), meetings and calendar |
| ⬜ | **M4 Platform** | Webhooks, bots/apps, slash commands, workflows, admin console (roles, retention, export, audit, SSO/SCIM), federation |
| ⬜ | **1.0** | Packaging and signed releases for every platform, accessibility audit, localisation, scale tests |

## Progress log

- **2026-11-01** — M1-E: device secrets in the OS keychain (with file fallback and migration); sign-in screen with a remembered account; sign out that revokes the session on the homeserver and deletes the device's keys.
- **2026-11-01** — M1-E: identity verification by emoji (Matrix SAS) with cross-signing, trust shown in profiles and member lists; key backup with a recovery key, restore on new devices, late-key re-decryption. Integration tests against Synapse for both.
- **2026-11-01** — M1-E started: `chatcraft-matrix` transport (matrix-rust-sdk 0.19), encrypted private channels and DMs, persistent device store, undecryptable-message and encryption indicators in the UI, `--matrix` in desktop/CLI, E2EE integration test against Synapse (server holds ciphertext only).
- **2026-10-31** — M0 landed: 11 crates + 4 apps, 65 tests, `cargo xtask ci` green. Clean-room rules, asset policy and BMad planning set up.
