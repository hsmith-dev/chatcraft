//! Events: the only way state changes.

use chatcraft_core::{ChannelId, MessageId, Ts, UserId};
use serde::{Deserialize, Serialize};

use crate::{Channel, Message, Presence, Role, Status, User};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    WorkspaceCreated {
        name: String,
        at: Ts,
    },
    WorkspaceRenamed {
        name: String,
    },
    DefaultChannelSet {
        channel: ChannelId,
    },
    UserJoined {
        user: User,
    },
    ProfileChanged {
        user: UserId,
        display_name: Option<String>,
        title: Option<String>,
    },
    StatusChanged {
        user: UserId,
        status: Status,
    },
    PresenceChanged {
        user: UserId,
        presence: Presence,
    },
    RoleChanged {
        user: UserId,
        role: Role,
    },
    UserDeactivated {
        user: UserId,
        deactivated: bool,
    },
    ChannelCreated {
        channel: Channel,
    },
    ChannelRenamed {
        channel: ChannelId,
        name: String,
    },
    TopicChanged {
        channel: ChannelId,
        topic: String,
        by: UserId,
    },
    PurposeChanged {
        channel: ChannelId,
        purpose: String,
        by: UserId,
    },
    ChannelArchived {
        channel: ChannelId,
        archived: bool,
        by: UserId,
    },
    MemberJoined {
        channel: ChannelId,
        user: UserId,
        by: Option<UserId>,
    },
    MemberLeft {
        channel: ChannelId,
        user: UserId,
        by: Option<UserId>,
    },
    MessagePosted {
        message: Message,
    },
    MessageEdited {
        message: MessageId,
        text: String,
        at: Ts,
        mentions: Vec<UserId>,
        mentions_all: bool,
    },
    MessageDeleted {
        message: MessageId,
        at: Ts,
    },
    ReactionAdded {
        message: MessageId,
        emoji: String,
        user: UserId,
    },
    ReactionRemoved {
        message: MessageId,
        emoji: String,
        user: UserId,
    },
    PinChanged {
        message: MessageId,
        pinned: bool,
        by: UserId,
    },
    ReadMarked {
        user: UserId,
        channel: ChannelId,
        upto: MessageId,
    },
    SavedChanged {
        user: UserId,
        message: MessageId,
        saved: bool,
    },
    ChannelMuted {
        user: UserId,
        channel: ChannelId,
        muted: bool,
    },
    ChannelStarred {
        user: UserId,
        channel: ChannelId,
        starred: bool,
    },
    /// A message's content became known later (an encrypted message decrypted when its key
    /// arrived): replaces the record, keeping its replies and reactions.
    MessageReplaced {
        message: Message,
    },
    /// Someone's public read receipt moved (Matrix; the in-process hub keeps read positions
    /// private and never emits this).
    Receipt {
        channel: ChannelId,
        user: UserId,
        upto: MessageId,
    },
    /// Who is typing in a conversation right now (transient: never journaled, not in snapshots).
    Typing {
        channel: ChannelId,
        users: Vec<UserId>,
    },
    /// The viewer's trust in someone's identity changed (client-side, from Matrix verification).
    UserVerified {
        user: UserId,
        verified: bool,
    },
}

/// An event with its position in the workspace log (1-based, gapless on the server).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sequenced {
    pub seq: u64,
    pub event: Event,
}

impl Event {
    /// The conversation this event belongs to, if any (for visibility filtering).
    pub fn channel(&self) -> Option<ChannelId> {
        match self {
            Event::ChannelCreated { channel } => Some(channel.id),
            Event::MessagePosted { message } | Event::MessageReplaced { message } => Some(message.channel),
            Event::ChannelRenamed { channel, .. }
            | Event::TopicChanged { channel, .. }
            | Event::PurposeChanged { channel, .. }
            | Event::ChannelArchived { channel, .. }
            | Event::MemberJoined { channel, .. }
            | Event::MemberLeft { channel, .. }
            | Event::Typing { channel, .. }
            | Event::Receipt { channel, .. }
            | Event::DefaultChannelSet { channel } => Some(*channel),
            _ => None,
        }
    }
    /// The message this event is about, if any.
    pub fn message(&self) -> Option<MessageId> {
        match self {
            Event::MessagePosted { message } | Event::MessageReplaced { message } => Some(message.id),
            Event::MessageEdited { message, .. }
            | Event::MessageDeleted { message, .. }
            | Event::ReactionAdded { message, .. }
            | Event::ReactionRemoved { message, .. }
            | Event::PinChanged { message, .. } => Some(*message),
            _ => None,
        }
    }
    /// The one user an event is private to (read cursors, saved items, mutes, stars).
    pub fn private_to(&self) -> Option<UserId> {
        match self {
            Event::ReadMarked { user, .. }
            | Event::SavedChanged { user, .. }
            | Event::ChannelMuted { user, .. }
            | Event::ChannelStarred { user, .. } => Some(*user),
            _ => None,
        }
    }
}
