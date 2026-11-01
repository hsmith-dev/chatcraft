# Milestones

_Status: 2026-10-31 · revision 1_

## Current focus

**M1 Daily driver.** Pick from the top of [gaps.md](gaps.md). The order is chosen so a small team
could move onto ChatCraft at the end of M1.

## M0 Foundations — done

- [x] Workspace layout, lints, `xtask` (assets, layers, wasm, parity, ci), shared CI gate
- [x] `core`, `text` (markup), `model` (event-sourced state), `proto`, `store` (journal), `search`
- [x] `hub`: permissions, visibility, ids, demo workspace
- [x] `engine`: session, transports, 65 commands, Slack/Teams catalog
- [x] `net`: WebSocket server and reconnecting client; `mcp`: MCP over stdio
- [x] `ui-egui`: sidebar, timeline, threads, reactions, composer, search, activity, saved, profiles, dialogs, themes
- [x] Apps: desktop, CLI, server, web (demo)

## M1 Daily driver

| Task | Notes |
|---|---|
| M1.1 Files and images | Upload, store (content-addressed blobs next to the journal), previews, size limits |
| M1.2 Per-user authentication | Password (argon2) + sessions; invite links replace the shared join token |
| M1.3 TLS | `wss://` natively (rustls) as well as behind a proxy |
| M1.4 OS notifications | Desktop notifications, dock/taskbar badge, notification preferences and quiet hours |
| M1.5 Edit history, typing indicators | |
| M1.6 Unreads view, mark unread, reminders | Mark unread needs read cursors that can move back (a new event) |
| M1.7 Browser client to a server | WebSocket transport through `web-sys` |
| M1.8 Search index | Inverted index in the hub; the scan is fine to ~10⁵ messages |
| M1.9 History paging | Snapshots carry recent history; older pages on demand |
| M1.10 Colour emoji font | Through `craft-fonts`; then offer the full emoji set |
