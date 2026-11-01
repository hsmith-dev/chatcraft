//! The parity catalog: features of Slack and Microsoft Teams, by area, each mapped to the
//! ChatCraft command id that provides it. A feature counts as implemented when its command is
//! registered. `cargo xtask parity` writes `docs/parity-checklist.md` from this list, and a test
//! keeps the implemented count from going down.
//!
//! Only feature *names* appear here (fair use for describing behaviour); no wording, artwork or
//! assets from either product.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::Session;

/// Which product the feature is from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Both,
    Slack,
    Teams,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Both => "Slack + Teams",
            Origin::Slack => "Slack",
            Origin::Teams => "Teams",
        }
    }
}

pub struct Feature {
    pub from: Origin,
    pub area: &'static str,
    pub name: &'static str,
    pub command: &'static str,
}

const fn f(from: Origin, area: &'static str, name: &'static str, command: &'static str) -> Feature {
    Feature { from, area, name, command }
}

use Origin::{Both, Slack, Teams};

pub const FEATURES: &[Feature] = &[
    // Messaging
    f(Both, "Messaging", "Send a message", "message.send"),
    f(Both, "Messaging", "Rich text formatting (bold, italic, strike, code, quotes, lists)", "message.send"),
    f(Both, "Messaging", "Mention a person (@name)", "message.send"),
    f(Both, "Messaging", "Mention everyone (@here, @channel)", "message.send"),
    f(Both, "Messaging", "Edit a message", "message.edit"),
    f(Slack, "Messaging", "Edit your last message (Up arrow)", "message.editLast"),
    f(Both, "Messaging", "Delete a message", "message.delete"),
    f(Both, "Messaging", "Emoji reactions", "message.react"),
    f(Both, "Messaging", "Pin a message", "message.pin"),
    f(Both, "Messaging", "Save for later / bookmark", "message.save"),
    f(Both, "Messaging", "Copy link to message", "message.copyLink"),
    f(Both, "Messaging", "Drafts kept per conversation", "composer.setDraft"),
    f(Slack, "Messaging", "Mark a message unread", "message.markUnread"),
    f(Slack, "Messaging", "Remind me about this message", "message.remind"),
    f(Both, "Messaging", "Forward / share a message", "message.forward"),
    f(Both, "Messaging", "Schedule a message", "message.schedule"),
    f(Teams, "Messaging", "Mark a message important or urgent", "message.markImportant"),
    f(Teams, "Messaging", "Announcement posts", "message.announce"),
    f(Teams, "Messaging", "Read receipts", "message.readReceipts"),
    f(Both, "Messaging", "Typing indicator", "composer.typing"),
    f(Both, "Messaging", "Edit history", "message.history"),
    f(Both, "Messaging", "Link previews (unfurling)", "message.unfurl"),
    f(Teams, "Messaging", "Translate a message", "message.translate"),
    // Threads
    f(Both, "Threads", "Reply in thread", "message.reply"),
    f(Slack, "Threads", "Also send a reply to the channel", "message.reply"),
    f(Both, "Threads", "Open a thread in the side pane", "thread.open"),
    f(Both, "Threads", "Thread summary (reply count, last reply)", "thread.list"),
    f(Slack, "Threads", "Threads view (all followed threads)", "nav.threads"),
    f(Slack, "Threads", "Follow / unfollow a thread", "thread.follow"),
    // Conversations
    f(Both, "Conversations", "Create a public channel", "channel.create"),
    f(Both, "Conversations", "Create a private channel", "channel.create"),
    f(Both, "Conversations", "Browse and join channels", "channel.browse"),
    f(Both, "Conversations", "Join a channel", "channel.join"),
    f(Both, "Conversations", "Leave a channel", "channel.leave"),
    f(Both, "Conversations", "Add people to a channel", "channel.invite"),
    f(Both, "Conversations", "Remove people from a channel", "channel.remove"),
    f(Both, "Conversations", "Rename a channel", "channel.rename"),
    f(Both, "Conversations", "Channel topic", "channel.setTopic"),
    f(Both, "Conversations", "Channel description", "channel.setPurpose"),
    f(Both, "Conversations", "Archive a channel", "channel.archive"),
    f(Both, "Conversations", "Unarchive a channel", "channel.unarchive"),
    f(Both, "Conversations", "Channel members list", "channel.members"),
    f(Both, "Conversations", "Channel details", "channel.info"),
    f(Both, "Conversations", "Pinned messages list", "channel.pins"),
    f(Both, "Conversations", "Direct messages", "dm.open"),
    f(Both, "Conversations", "Group direct messages", "dm.open"),
    f(Slack, "Conversations", "Message yourself (notes to self)", "dm.open"),
    f(Slack, "Conversations", "Star / favourite a conversation", "channel.star"),
    f(Slack, "Conversations", "Custom sidebar sections", "sidebar.createSection"),
    f(Teams, "Conversations", "Teams (groups of channels)", "team.create"),
    f(Teams, "Conversations", "Channel tabs", "channel.addTab"),
    f(Both, "Conversations", "Shared channels with other organizations", "channel.shareExternal"),
    f(Slack, "Conversations", "Canvases / Teams Loop pages", "canvas.create"),
    f(Slack, "Conversations", "Lists", "list.create"),
    // Reading and navigation
    f(Both, "Navigation", "Unread badges and mention counts", "conversations.list"),
    f(Both, "Navigation", "Mark a conversation read", "channel.markRead"),
    f(Slack, "Navigation", "Mark all as read", "workspace.markAllRead"),
    f(Slack, "Navigation", "Jump to next unread conversation", "nav.nextUnread"),
    f(Both, "Navigation", "Move between conversations with the keyboard", "nav.next"),
    f(Both, "Navigation", "Jump to a conversation (quick switcher)", "nav.channel"),
    f(Both, "Navigation", "Back and forward history", "nav.back"),
    f(Slack, "Navigation", "All unreads view", "nav.unreads"),
    f(Both, "Navigation", "Activity feed (mentions)", "activity.mentions"),
    f(Both, "Navigation", "Saved items view", "saved.list"),
    f(Both, "Navigation", "Conversation details pane", "nav.details"),
    // Search
    f(Both, "Search", "Search messages", "search.messages"),
    f(Both, "Search", "Search modifiers (in:, from:, has:, before:/after:)", "search.messages"),
    f(Both, "Search", "Search people", "people.list"),
    f(Both, "Search", "Search channels", "channel.browse"),
    f(Both, "Search", "Search files", "search.files"),
    // People and presence
    f(Both, "People", "Profiles with title", "people.get"),
    f(Both, "People", "View someone's profile", "nav.profile"),
    f(Both, "People", "People directory", "people.list"),
    f(Both, "People", "Custom status with emoji and expiry", "profile.setStatus"),
    f(Both, "People", "Clear status", "profile.clearStatus"),
    f(Both, "People", "Availability (active, away, do not disturb)", "profile.setPresence"),
    f(Slack, "People", "Toggle away", "profile.toggleAway"),
    f(Both, "People", "Edit display name", "profile.setName"),
    f(Both, "People", "Edit title", "profile.setTitle"),
    f(Teams, "People", "Org chart", "people.orgChart"),
    // Notifications
    f(Both, "Notifications", "Notifications for mentions and direct messages", "activity.mentions"),
    f(Both, "Notifications", "Mute a conversation", "channel.mute"),
    f(Both, "Notifications", "Notification schedule / quiet hours", "notifications.schedule"),
    f(Slack, "Notifications", "Keyword notifications", "notifications.keywords"),
    f(Both, "Notifications", "Per-conversation notification level", "notifications.level"),
    // Files
    f(Both, "Files", "Upload files and images", "file.upload"),
    f(Both, "Files", "Image and document previews", "file.preview"),
    f(Both, "Files", "Files list per conversation", "file.list"),
    f(Slack, "Files", "Code snippets", "file.snippet"),
    // Calls and meetings
    f(Slack, "Calls", "Huddles (drop-in audio)", "huddle.start"),
    f(Both, "Calls", "Voice and video calls", "call.start"),
    f(Both, "Calls", "Screen sharing", "call.shareScreen"),
    f(Teams, "Calls", "Scheduled meetings", "meeting.schedule"),
    f(Teams, "Calls", "Calendar", "calendar.view"),
    f(Both, "Calls", "Recorded video clips", "clip.record"),
    // Automation and apps
    f(Both, "Automation", "Agent control over MCP", "commands.list"),
    f(Both, "Automation", "Command-line access", "commands.list"),
    f(Both, "Automation", "Slash commands", "command.slash"),
    f(Slack, "Automation", "Reminders (/remind)", "reminder.create"),
    f(Both, "Automation", "Polls", "poll.create"),
    f(Both, "Automation", "Workflows", "workflow.create"),
    f(Both, "Automation", "Incoming webhooks", "integration.webhook"),
    f(Both, "Automation", "Bots and apps", "app.install"),
    // Administration
    f(Both, "Administration", "Workspace details", "workspace.info"),
    f(Both, "Administration", "Rename the workspace", "workspace.rename"),
    f(Both, "Administration", "Invite people to the workspace", "workspace.invite"),
    f(Both, "Administration", "Roles (owner, admin, member, guest)", "admin.setRole"),
    f(Both, "Administration", "Deactivate an account", "admin.deactivate"),
    f(Both, "Administration", "Message retention", "admin.retention"),
    f(Both, "Administration", "Export workspace data", "admin.export"),
    f(Both, "Administration", "Import from another chat app", "admin.import"),
    f(Both, "Administration", "Single sign-on", "admin.sso"),
    f(Both, "Administration", "Audit log", "admin.audit"),
];

pub fn implemented(f: &Feature) -> bool {
    Session::registry().get(f.command).is_some()
}

/// `(implemented, total)`.
pub fn counts() -> (usize, usize) {
    (FEATURES.iter().filter(|f| implemented(f)).count(), FEATURES.len())
}

pub fn summary_json() -> Value {
    let (done, total) = counts();
    let mut areas: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for f in FEATURES {
        let e = areas.entry(f.area).or_default();
        e.1 += 1;
        if implemented(f) {
            e.0 += 1;
        }
    }
    json!({
        "implemented": done, "total": total,
        "areas": areas.iter().map(|(a, (d, t))| json!({"area": a, "implemented": d, "total": t})).collect::<Vec<_>>(),
        "missing": FEATURES.iter().filter(|f| !implemented(f)).map(|f| json!({"feature": f.name, "from": f.from.label(), "command": f.command})).collect::<Vec<_>>(),
    })
}

/// The checklist as Markdown (`docs/parity-checklist.md`).
pub fn markdown() -> String {
    let (done, total) = counts();
    let pct = (done * 100).checked_div(total).unwrap_or(0);
    let mut s = format!(
        "# Parity checklist\n\n_Generated by `cargo xtask parity` from `crates/engine/src/catalog.rs`. Do not edit by hand._\n\n**{done} of {total} features ({pct}%)** have a ChatCraft command.\n\n"
    );
    let mut area = "";
    for f in FEATURES {
        if f.area != area {
            area = f.area;
            s.push_str(&format!("\n## {area}\n\n| | Feature | From | Command |\n|---|---|---|---|\n"));
        }
        let mark = if implemented(f) { "✅" } else { "⬜" };
        s.push_str(&format!("| {mark} | {} | {} | `{}` |\n", f.name, f.from.label(), f.command));
    }
    s
}
