# Gaps

_Status: 2026-10-31 · revision 1_

Ranked: work from the top. Each gap names the catalog command ids it unlocks, if any.

**Direction (2026-11-01): ChatCraft is a Matrix client with end-to-end encryption** (see
[matrix.md](matrix.md)). Gaps 0.x are the encryption work in progress and come first.

0.1 **Device verification and cross-signing** (SAS emoji and QR), trust indicators per person and device, warnings for unverified devices.
0.2 **Key backup and recovery** (encrypted server-side backup, recovery key), so new devices can read history; secret storage (4S).
0.3 **OS keychain** for the store passphrase and access token (macOS Keychain, Windows Credential Manager, Secret Service).
0.4 **Late keys**: retry decryption when a room key arrives after the message.
0.5 **Sign-in UI** (homeserver, user, password, SSO/OIDC) instead of command-line flags; sign out and device management.
0.6 **Rich text from other clients**: render `formatted_body` HTML (bold, links, pills) instead of the plain body.
0.7 **Matrix coverage**: room directory browsing, pins (`m.room.pinned_events`), saved items and mutes (account data / push rules), archiving (tombstones), history paging, typing and read receipts display.
0.8 **Browser client** over Matrix (matrix-sdk's IndexedDB store on wasm).
0.9 **Retire `chatcraft-server`** once Matrix covers its features (or keep it as a tiny self-host option).

1. **Files and images** (`file.upload`, `file.preview`, `file.list`, `search.files`). Biggest missing piece for daily use.
2. ~~Per-user authentication~~ and ~~TLS~~: provided by the Matrix homeserver (accounts, SSO, `https`). Still open for the legacy `chatcraft-server`.
4. **Notifications.** In-app toasts exist; OS notifications, badges, schedules and keywords don't (`notifications.schedule`, `notifications.keywords`, `notifications.level`).
5. **Browser client to a server.** The web build runs the demo only.
6. **Message lifecycle**: edit history, mark unread, reminders, scheduled send, forward (`message.history`, `message.markUnread`, `message.remind`, `message.schedule`, `message.forward`).
7. **Presence**: typing indicators, automatic away, status expiry applied by the hub (expiry is stored but not yet enforced).
8. **Scale**: snapshots send the whole visible history; search scans; the hub is one mutex. Fine for small teams; needs paging, an index and sharding by conversation later.
9. **Teams-shaped features**: teams/channel groups, sidebar sections, tabs, read receipts, priority and announcement posts (`team.create`, `sidebar.createSection`, `channel.addTab`, `message.readReceipts`, `message.markImportant`, `message.announce`).
10. **Import** from Slack and Teams export files (`admin.import`), from their publicly documented formats.
11. **Calls**: huddles, calls, screen share, meetings, calendar (WebRTC; `huddle.start`, `call.start`, `call.shareScreen`, `meeting.schedule`, `calendar.view`, `clip.record`).
12. **Platform**: webhooks, bots, slash commands, workflows, polls (`integration.webhook`, `app.install`, `command.slash`, `workflow.create`, `poll.create`, `reminder.create`).
13. **Admin**: roles UI, deactivation, retention, export, audit log (`admin.setRole`, `admin.deactivate`, `admin.retention`, `admin.export`, `admin.audit`, `workspace.invite`).
14. **Emoji**: colour emoji font; custom emoji (uploaded, attributed).
15. **Accessibility and localisation**: painted widgets expose labels; a full screen-reader pass and translations remain.
16. **Packaging**: installers, signing, auto-update (follow the Craft apps' `packaging/` and release workflow).
