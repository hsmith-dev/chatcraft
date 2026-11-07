# Gaps

_Status: 2026-10-31 · revision 1_

Ranked: work from the top. Each gap names the catalog command ids it unlocks, if any.

**Direction (2026-11-01): ChatCraft is a Matrix client with end-to-end encryption** (see
[matrix.md](matrix.md)). Gaps 0.x are the encryption work in progress and come first.

0.1 ~~Verifying people by emoji, cross-signing, your own devices~~ (done, 2026-11-01). Still open: QR codes, identity-change warnings, a verified-devices-only trust option.
0.2 ~~Key backup and recovery~~ (done, 2026-11-01). Still open: changing or resetting the recovery key from the UI, a passphrase alternative.
0.3 ~~OS keychain~~ (done, 2026-11-01; file fallback where there is none).
0.4 ~~Late keys~~ (done: undecryptable messages are retried when keys arrive and after restoring).
0.5 ~~Sign-in screen, remembered account, sign out, device list with remote sign-out~~ (done, 2026-11-01). Still open: SSO/OIDC sign-in, registration from the app.
0.6 ~~Rich text from other clients~~ (done, 2026-11-01): HTML ↔ ChatCraft markup both ways.
0.7 Matrix coverage: ~~directory, pins, saved items, mutes~~ (done, 2026-11-01). Still open: an option to send private read receipts (`m.read.private`). Read receipts shown (2026-11-01). Archiving done (2026-11-01: read-only power levels + marker). Typing indicators and history paging done (2026-11-01).
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
