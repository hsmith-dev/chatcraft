# ChatCraft — instructions for agents

ChatCraft is a clean-room, open-source, Rust-native team chat app targeting Slack and Microsoft Teams parity — and going further on openness, privacy, self-hosting and agent control. It runs natively on macOS, Windows, Linux and BSD, and on the web via WASM. It follows the conventions of the ArtCraft "Crafting apps" (`../Wordcraft/wordcraft`, `../Pdfcraft`, `../Gridcraft`, `../Photocraft`); it is an independent project, not part of ArtCraft.

## Start every session here
1. Read `plan/STATUS.md` (current milestone, next task), then the task in `plan/execution-plan.md`. `plan/` is gitignored (local only); if it's missing, start from `ROADMAP.md`, `docs/gaps.md` and `docs/roadmap.md` (Current focus).
2. BMad planning artifacts live in `plan/bmad/` (PRD, architecture, epics, sprint status). BMad skills are installed locally (`npx bmad-method install`, see `docs/contributing.md`); use them for planning and story work, but this file and `docs/architecture.md` win over any generated artifact.
3. Don't stop to ask unless a decision is genuinely the owner's (licensing, publishing, pushing to new remotes, secrets, hosting).

## Never crash
People trust ChatCraft with their conversations; a crash loses messages and drops connections. **This outranks feature work.**
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`).
- **Errors are `Result<T, E>`** through the crate's error type and `?`. An unfinished feature returns an error or is disabled; it never panics.
- **Everything from the network, files and agents is hostile:** frames are size-capped (`core::limits`), ids parse to `Option`, `get()` instead of `[i]`, slice strings only at char boundaries, saturating arithmetic, bounded allocations and recursion.
- **`State::apply` is total:** an event naming something unknown is ignored, never a crash.
- **Prove it:** every crash fix lands with a regression test. `crates/engine/src/tests.rs::hostile_params_never_panic` runs every command with junk params; `crates/hub/src/tests.rs::hostile_ops_never_panic` does the same for the hub — keep them green.
- Production crate roots carry `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`; new crates start with it and add `[lints] workspace = true`. Tests may `unwrap` (`clippy.toml`).

## Clean room — how we "reverse engineer" Slack and Teams
We reproduce **behaviour**, never code or assets.
- **Allowed:** using Slack and Teams as an ordinary user with synthetic workspaces and observing what they do; their public documentation, help centres, public API references and published protocols; open standards (WebSocket, OAuth/OIDC, SCIM, Matrix/XMPP for bridges, RFC 5322 for email). Notes and screenshots of their UIs stay in `plan/observed/` (gitignored, never committed); screenshot the app window only, never the whole desktop.
- **Never:** decompile, disassemble, unpack or read the code or resources inside the Slack or Teams clients (Electron `app.asar`, bundles, minified JS); intercept or replay their private network APIs; use leaked or proprietary material; copy their wording beyond feature names; sign in to real workspaces you don't own.
- **Never copy GPL/AGPL/LGPL code** (e.g. Mattermost server, Rocket.Chat, Zulip are useful *references for ideas only*). MIT/Apache/BSD code may be used with attribution.
- Data import (Slack/Teams export files) is built from their **publicly documented** export formats only.

## Assets: absolutely no Slack, Microsoft or other vendor iconography or images
**This rule is non-negotiable.**
- Never copy, trace, screenshot-crop or recreate icons, logos, emoji artwork, sounds, themes, colour schemes, illustrations or fonts from Slack, Teams or any other product. No aubergine Slack sidebar, no Teams purple — ChatCraft has its own teal/amber palette (`crates/ui-egui/src/theme.rs`).
- Every asset must be **original** (drawn in code — prefer this: `crates/ui-egui/src/icons.rs`), **public domain / CC0**, **OSI open source**, or **Creative Commons that allows redistribution**.
- **Every asset file has a row in [`ATTRIBUTION.md`](ATTRIBUTION.md)**. `cargo xtask assets` fails otherwise. Add the row in the same commit as the file.
- Emoji are Unicode text rendered by open fonts; only emoji the bundled fonts can draw are offered (`every_offered_emoji_has_a_glyph`).
- Demo people, teams and conversations (`crates/hub/src/sample.rs`) are fictional and our own.

## Architecture (see `docs/architecture.md`)
| Layer | Crate | Job |
|---|---|---|
| L0 | `core`, `text` (standalone) | typed ids, timestamps, limits, name rules; message markup, mentions, emoji |
| L1 | `model` | workspace entities, `Event`s, the event-sourced `State` and its queries |
| L2 | `proto`, `store`, `search` | wire frames and ops; the append-only JSONL journal; search with modifiers |
| L3 | `hub` | the authority: validation, permissions, ids, visibility; the demo workspace — pure, no I/O |
| L4 | `engine` | client `Session` (replica, view, drafts), `Transport` + in-process `Loopback`, **command registry**, Slack/Teams catalog |
| L5 | `matrix`, `net`, `mcp` | Matrix transport with E2EE (the network protocol); legacy WebSocket server/client; MCP server |
| L6 | `ui-egui` | the egui front end (swappable) |
| apps | `chatcraft`, `chatcraft-cli`, `chatcraft-server`, `chatcraft-web` | binaries |

- **Everything is a command.** User-visible behaviour = a `CommandSpec` in `crates/engine/src/cmd/*.rs` (id, label, location, shortcut, params doc, `enabled`, `run`) + tests. UI-only commands (`ui.*`) are in `crates/ui-egui/src/lib.rs` (`UI_COMMANDS`). The UI, shortcuts, quick switcher, CLI and MCP all dispatch by id. Programmatic calls never open dialogs.
- **The hub is the only authority.** Clients never mutate their replica: they send an `Op`, the hub validates it and emits `Event`s, every replica applies them. A new behaviour usually means: an `Op` (proto) → planning in `crates/hub/src/ops.rs` → an `Event` + `State::apply` (model) → a command (engine) → UI.
- **Visibility is enforced at the hub.** Private conversations and per-user events never reach people who can't see them (`State::event_visible`, `State::visible_to`). Unknown or invisible ids answer "not found", never "forbidden".
- **Parity is measured:** `crates/engine/src/catalog.rs` lists Slack and Teams features with command ids. `cargo xtask parity` writes `docs/parity-checklist.md`; `parity_floor` only rises.
- **Layering** is enforced by `cargo xtask layers`. Nothing below L6 depends on egui/eframe/winit/rfd. The UI is thin and reads `Session`; colours come from `theme::Tokens`.
- **Time goes through `chrono`** and `Ts` (`core`); never hand-roll calendar math. The UI applies the viewer's UTC offset (`ChatApp::utc_offset_minutes`).
- **Rust only** (no handwritten JS/TS beyond the web shell's HTML). **Never break wasm** (`cargo xtask wasm`).

## Matrix and encryption
- The network protocol is **Matrix** (`crates/matrix`, `docs/matrix.md`); the homeserver is the authority there. New network features map to Matrix events/APIs, not to new ChatCraft frames.
- Encryption is never optional for private conversations and DMs. Never log, persist or display decrypted content outside the replica; secrets (store passphrase, tokens) come from the OS CSPRNG and are written 0600.
- Treat everything from the homeserver as untrusted: e.g. edits are accepted only from the original author (`mapper.rs`).
- `crates/matrix/tests/e2ee.rs` must pass against a real homeserver before encryption changes land (see `docs/matrix.md` › Testing).

## Quality gates
Before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, doctests, assets, layers, wasm). Scope day-to-day builds to the crates you touched (`cargo test -p chatcraft-hub`). One task id per commit (`M1.3: message edit history`). `main` is never pushed to directly: branch `feat/…`, `fix/…`, `docs/…`, open a PR.

## Running and looking at the app
- Desktop, demo workspace: `cargo run --release -p chatcraft`.
- Server + clients: `cargo run --release -p chatcraft-server -- --data ./chatcraft-data --name "My Team"` (prints the join token), then `cargo run --release -p chatcraft -- --server ws://127.0.0.1:7900 --handle ada --token TOKEN`.
- CLI: `chatcraft-cli commands`, `chatcraft-cli parity --markdown`, `chatcraft-cli run --demo --as ada --cmd 'message.send={"channel":"#general","text":"hi"}'`, and the same with `--server ws://… --as ada --token …`.
- MCP: `chatcraft-cli mcp` (demo) or `chatcraft-cli mcp --server ws://… --as bot --token …`. See `docs/mcp.md`.
- **Headless screenshots** (no window, no focus stealing): `cargo run -p chatcraft-ui-egui --example shot -- out.png [--dark] [--cmd 'thread.open={"message":9}']`. **For UI work, look at the result** (read the PNG).
- Web: `cd apps/chatcraft-web && trunk serve`.
- Shell gotchas: zsh doesn't word-split `$VARS` (spell arguments out); macOS has no `timeout` (use `perl -e 'alarm 60; exec @ARGV' cmd`).
- Parallel agents: separate `CARGO_TARGET_DIR` per agent; edit only the crates you own.

## Roadmap
`ROADMAP.md` (stage, numbers, progress log), `docs/target-app-parity.md` (the assessment), `docs/gaps.md` (ranked work list — pick from it), `docs/roadmap.md` (milestones, Current focus), `docs/architecture.md`, `docs/protocol.md`, `docs/parity-checklist.md` (generated). When work lands, update the gap, the parity entry and the progress log.

## CI workflows
- `.github/workflows/ci.yml` calls the public shared gate in [`storytold/craft-common-actions`](https://github.com/storytold/craft-common-actions) (`cargo xtask ci`) the way the Craft apps do. Don't add or change CI in a feature or fix PR.
