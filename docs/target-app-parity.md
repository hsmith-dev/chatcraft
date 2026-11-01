# Target-app parity

_Status: 2026-10-31 · revision 1_

ChatCraft's targets are **Slack** and **Microsoft Teams**. Parity is measured by the catalog in
`crates/engine/src/catalog.rs`: each feature of either product, by area, mapped to the ChatCraft
command that provides it. "Implemented" means the command exists and is tested — not that it is as
polished as the original. `cargo xtask parity` regenerates [parity-checklist.md](parity-checklist.md).

| Area | Assessment |
|---|---|
| Messaging | Core loop complete; lifecycle extras (history, unread, remind, schedule, forward) missing |
| Threads | Complete except a Threads view and follow/unfollow |
| Conversations | Complete for channels and DMs; Teams' team/tab structure and shared channels missing |
| Navigation | Strong (quick switcher, unread jumps, history, activity, saved) |
| Search | Messages, people and channels with modifiers; files missing |
| People | Profiles, presence, status complete; org chart missing |
| Notifications | In-app only |
| Files | Not started |
| Calls | Not started |
| Automation | CLI and MCP for every command; webhooks/bots/workflows missing |
| Administration | Workspace basics only |

How we observe the targets: black-box use with synthetic workspaces and their public
documentation, per the clean-room rules in [AGENTS.md](../AGENTS.md).
