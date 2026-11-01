//! Entities.

use std::collections::BTreeSet;

use chatcraft_core::{ChannelId, MessageId, Ts, UserId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub created: Ts,
    /// The channel everyone joins and can't leave (`#general`).
    pub default_channel: Option<ChannelId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Presence {
    Active,
    Away,
    /// Do not disturb: notifications are held.
    Dnd,
    #[default]
    Offline,
}

impl Presence {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "active" | "online" | "available" => Some(Presence::Active),
            "away" | "idle" | "brb" => Some(Presence::Away),
            "dnd" | "busy" | "donotdisturb" | "do-not-disturb" => Some(Presence::Dnd),
            "offline" | "invisible" => Some(Presence::Offline),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Presence::Active => "Active",
            Presence::Away => "Away",
            Presence::Dnd => "Do not disturb",
            Presence::Offline => "Offline",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Owner,
    Admin,
    #[default]
    Member,
    Guest,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub text: String,
    /// Shortcode without colons (`palm_tree`).
    pub emoji: String,
    /// When the status clears itself, if ever.
    pub until: Option<Ts>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    /// Unique, lowercase (`ada`).
    pub handle: String,
    pub display_name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub presence: Presence,
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub bot: bool,
    /// Avatar palette index (initials on a coloured tile; drawn in code).
    #[serde(default)]
    pub color: u8,
    #[serde(default)]
    pub joined: Ts,
    #[serde(default)]
    pub deactivated: bool,
}

impl User {
    /// The name to show: display name, else the handle.
    pub fn name(&self) -> &str {
        if self.display_name.trim().is_empty() { &self.handle } else { &self.display_name }
    }
    /// Up to two initials for the avatar tile.
    pub fn initials(&self) -> String {
        let mut it = self.name().split_whitespace().filter_map(|w| w.chars().next());
        let a = it.next();
        let b = it.next_back();
        [a, b].into_iter().flatten().flat_map(char::to_uppercase).collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelKind {
    /// Anyone in the workspace can find, read and join it.
    #[default]
    Public,
    /// Invite-only; only members see it exists.
    Private,
    /// A one-to-one conversation (or a note-to-self when it has one member).
    Direct,
    /// A group direct message (3–9 people).
    Group,
}

impl ChannelKind {
    pub fn is_dm(self) -> bool {
        matches!(self, ChannelKind::Direct | ChannelKind::Group)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    /// Lowercase name for channels; empty for DMs (shown as the other people's names).
    pub name: String,
    pub kind: ChannelKind,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub purpose: String,
    pub members: BTreeSet<UserId>,
    #[serde(default)]
    pub archived: bool,
    pub created_by: UserId,
    pub created: Ts,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel: ChannelId,
    pub author: UserId,
    pub ts: Ts,
    pub text: String,
    /// The thread this is a reply in (the root message's id).
    #[serde(default)]
    pub thread: Option<MessageId>,
    /// A thread reply that was also sent to the channel.
    #[serde(default)]
    pub broadcast: bool,
    #[serde(default)]
    pub edited: Option<Ts>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub pinned: bool,
    /// Shortcode → who reacted, in first-reaction order of shortcodes.
    #[serde(default)]
    pub reactions: Vec<(String, BTreeSet<UserId>)>,
    /// Users mentioned by `@handle`, resolved when the message was posted or edited.
    #[serde(default)]
    pub mentions: BTreeSet<UserId>,
    /// `@here`, `@channel` or `@everyone` was used.
    #[serde(default)]
    pub mentions_all: bool,
    /// Thread roots: reply count and the last reply's time (maintained by `apply`).
    #[serde(default)]
    pub reply_count: u32,
    #[serde(default)]
    pub last_reply: Option<Ts>,
    #[serde(default)]
    pub repliers: BTreeSet<UserId>,
}

impl Message {
    pub fn is_reply(&self) -> bool {
        self.thread.is_some()
    }
    /// Shown in the channel's main timeline: roots and replies also sent to the channel.
    pub fn in_timeline(&self) -> bool {
        self.thread.is_none() || self.broadcast
    }
    pub fn reacted(&self, emoji: &str, user: UserId) -> bool {
        self.reactions.iter().any(|(e, who)| e == emoji && who.contains(&user))
    }
}
