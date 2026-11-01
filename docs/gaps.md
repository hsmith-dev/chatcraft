# Gaps

_Status: 2026-10-31 · revision 1_

Ranked: work from the top. Each gap names the catalog command ids it unlocks, if any.

1. **Files and images** (`file.upload`, `file.preview`, `file.list`, `search.files`). Biggest missing piece for daily use.
2. **Per-user authentication.** Today a shared join token lets anyone holding it sign in as any *unused* handle; existing handles are protected only by the token. Passwords/invite links first, OIDC/SSO later (`admin.sso`).
3. **Native TLS.** `ws://` only; production needs a reverse proxy today.
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
