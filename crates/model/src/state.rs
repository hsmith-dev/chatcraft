//! The workspace state and its queries.

use std::collections::{BTreeMap, BTreeSet};

use chatcraft_core::{ChannelId, MessageId, Ts, UserId};
use serde::{Deserialize, Serialize};

use crate::{Channel, ChannelKind, Event, Message, Sequenced, User, Workspace};

/// Everything a workspace knows. Build it with [`State::apply`]; read it through the queries.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub workspace: Workspace,
    pub users: BTreeMap<UserId, User>,
    pub channels: BTreeMap<ChannelId, Channel>,
    pub messages: BTreeMap<MessageId, Message>,
    /// Per user: the last message read in each conversation.
    #[serde(default)]
    pub reads: BTreeMap<UserId, BTreeMap<ChannelId, MessageId>>,
    #[serde(default)]
    pub saved: BTreeMap<UserId, BTreeSet<MessageId>>,
    #[serde(default)]
    pub muted: BTreeMap<UserId, BTreeSet<ChannelId>>,
    #[serde(default)]
    pub starred: BTreeMap<UserId, BTreeSet<ChannelId>>,
    /// The sequence number of the last applied event.
    #[serde(default)]
    pub seq: u64,
    /// Who is typing where (transient).
    #[serde(skip)]
    pub typing: BTreeMap<ChannelId, BTreeSet<UserId>>,
    /// Channel → timeline message ids (roots and broadcast replies), ascending.
    #[serde(skip)]
    timeline: BTreeMap<ChannelId, Vec<MessageId>>,
    /// Root → reply ids, ascending.
    #[serde(skip)]
    replies: BTreeMap<MessageId, Vec<MessageId>>,
}

/// A conversation as the sidebar shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChannelSummary {
    pub id: ChannelId,
    pub title: String,
    pub kind: ChannelKind,
    pub unread: usize,
    pub mentions: usize,
    pub muted: bool,
    pub starred: bool,
    pub archived: bool,
    pub last_activity: Ts,
    /// For direct messages: the other person (for presence dots and avatars).
    pub peer: Option<UserId>,
}

/// Where a message sits in time: `(sent, id)`. Ids alone aren't chronological when older
/// history is loaded later (Matrix pagination), so every index orders by this. Unknown ids sort
/// first.
fn order_of(messages: &BTreeMap<MessageId, Message>, id: MessageId) -> (Ts, MessageId) {
    (messages.get(&id).map_or(Ts(i64::MIN), |m| m.ts), id)
}

fn insert_ordered(messages: &BTreeMap<MessageId, Message>, v: &mut Vec<MessageId>, id: MessageId) {
    let key = order_of(messages, id);
    let pos = v.partition_point(|x| order_of(messages, *x) < key);
    if v.get(pos) != Some(&id) {
        v.insert(pos, id);
    }
}

impl State {
    /// Applies a sequenced event and records its position.
    pub fn apply(&mut self, s: &Sequenced) {
        self.apply_event(&s.event);
        self.seq = self.seq.max(s.seq);
    }

    /// Applies one event. Unknown ids are ignored; nothing here panics.
    #[allow(clippy::cognitive_complexity)]
    pub fn apply_event(&mut self, e: &Event) {
        match e {
            Event::WorkspaceCreated { name, at } => {
                self.workspace.name.clone_from(name);
                self.workspace.created = *at;
            }
            Event::WorkspaceRenamed { name } => self.workspace.name.clone_from(name),
            Event::DefaultChannelSet { channel } => self.workspace.default_channel = Some(*channel),
            Event::UserJoined { user } => {
                self.users.insert(user.id, user.clone());
            }
            Event::ProfileChanged { user, display_name, title } => {
                if let Some(u) = self.users.get_mut(user) {
                    if let Some(d) = display_name {
                        u.display_name.clone_from(d);
                    }
                    if let Some(t) = title {
                        u.title.clone_from(t);
                    }
                }
            }
            Event::StatusChanged { user, status } => {
                if let Some(u) = self.users.get_mut(user) {
                    u.status = status.clone();
                }
            }
            Event::PresenceChanged { user, presence } => {
                if let Some(u) = self.users.get_mut(user) {
                    u.presence = *presence;
                }
            }
            Event::RoleChanged { user, role } => {
                if let Some(u) = self.users.get_mut(user) {
                    u.role = *role;
                }
            }
            Event::UserDeactivated { user, deactivated } => {
                if let Some(u) = self.users.get_mut(user) {
                    u.deactivated = *deactivated;
                }
            }
            Event::ChannelCreated { channel } => {
                self.channels.insert(channel.id, channel.clone());
                self.timeline.entry(channel.id).or_default();
            }
            Event::ChannelRenamed { channel, name } => {
                if let Some(c) = self.channels.get_mut(channel) {
                    c.name.clone_from(name);
                }
            }
            Event::TopicChanged { channel, topic, .. } => {
                if let Some(c) = self.channels.get_mut(channel) {
                    c.topic.clone_from(topic);
                }
            }
            Event::PurposeChanged { channel, purpose, .. } => {
                if let Some(c) = self.channels.get_mut(channel) {
                    c.purpose.clone_from(purpose);
                }
            }
            Event::ChannelArchived { channel, archived, .. } => {
                if let Some(c) = self.channels.get_mut(channel) {
                    c.archived = *archived;
                }
            }
            Event::MemberJoined { channel, user, .. } => {
                if let Some(c) = self.channels.get_mut(channel) {
                    c.members.insert(*user);
                }
            }
            Event::MemberLeft { channel, user, .. } => {
                if let Some(c) = self.channels.get_mut(channel) {
                    c.members.remove(user);
                }
            }
            Event::MessagePosted { message } => self.post(message),
            Event::MessageEdited { message, text, at, mentions, mentions_all } => {
                if let Some(m) = self.messages.get_mut(message).filter(|m| !m.deleted) {
                    m.text.clone_from(text);
                    m.edited = Some(*at);
                    m.mentions = mentions.iter().copied().collect();
                    m.mentions_all = *mentions_all;
                }
            }
            Event::MessageDeleted { message, .. } => self.delete(*message),
            Event::ReactionAdded { message, emoji, user } => {
                if let Some(m) = self.messages.get_mut(message).filter(|m| !m.deleted) {
                    match m.reactions.iter_mut().find(|(e, _)| e == emoji) {
                        Some((_, who)) => {
                            who.insert(*user);
                        }
                        None => m.reactions.push((emoji.clone(), BTreeSet::from([*user]))),
                    }
                }
            }
            Event::ReactionRemoved { message, emoji, user } => {
                if let Some(m) = self.messages.get_mut(message) {
                    for (e, who) in &mut m.reactions {
                        if e == emoji {
                            who.remove(user);
                        }
                    }
                    m.reactions.retain(|(_, who)| !who.is_empty());
                }
            }
            Event::PinChanged { message, pinned, .. } => {
                if let Some(m) = self.messages.get_mut(message) {
                    m.pinned = *pinned;
                }
            }
            Event::ReadMarked { user, channel, upto } => {
                // Read cursors only move forward in time.
                let current = self.last_read(*user, *channel);
                if self.is_after(*upto, current) {
                    self.reads.entry(*user).or_default().insert(*channel, *upto);
                }
            }
            Event::SavedChanged { user, message, saved } => toggle(self.saved.entry(*user).or_default(), *message, *saved),
            Event::ChannelMuted { user, channel, muted } => toggle(self.muted.entry(*user).or_default(), *channel, *muted),
            Event::ChannelStarred { user, channel, starred } => toggle(self.starred.entry(*user).or_default(), *channel, *starred),
            Event::MessageReplaced { message } => {
                if let Some(old) = self.messages.remove(&message.id) {
                    if let Some(tl) = self.timeline.get_mut(&old.channel) {
                        tl.retain(|id| *id != old.id);
                    }
                    let mut m = message.clone();
                    m.reply_count = old.reply_count;
                    m.last_reply = old.last_reply;
                    m.repliers = old.repliers;
                    if m.reactions.is_empty() {
                        m.reactions = old.reactions;
                    }
                    self.post(&m);
                }
            }
            Event::Typing { channel, users } => {
                if users.is_empty() {
                    self.typing.remove(channel);
                } else {
                    self.typing.insert(*channel, users.iter().copied().collect());
                }
            }
            Event::UserVerified { user, verified } => {
                if let Some(u) = self.users.get_mut(user) {
                    u.verified = *verified;
                }
            }
        }
    }

    fn post(&mut self, m: &Message) {
        if self.messages.contains_key(&m.id) {
            return;
        }
        // Sending a message ends typing.
        if let Some(t) = self.typing.get_mut(&m.channel) {
            t.remove(&m.author);
        }
        // In the map first, so its own sort key is known when it's indexed.
        self.messages.insert(m.id, m.clone());
        if let Some(root) = m.thread {
            insert_ordered(&self.messages, self.replies.entry(root).or_default(), m.id);
            if let Some(r) = self.messages.get_mut(&root) {
                r.reply_count = r.reply_count.saturating_add(1);
                r.last_reply = Some(r.last_reply.map_or(m.ts, |t| t.max(m.ts)));
                r.repliers.insert(m.author);
            }
        }
        if m.in_timeline() {
            insert_ordered(&self.messages, self.timeline.entry(m.channel).or_default(), m.id);
        }
    }

    /// Where a message sits in time (see [`order_of`]).
    pub fn order(&self, id: MessageId) -> (Ts, MessageId) {
        order_of(&self.messages, id)
    }

    /// `a` comes after `b` in time.
    pub fn is_after(&self, a: MessageId, b: MessageId) -> bool {
        self.order(a) > self.order(b)
    }

    fn delete(&mut self, id: MessageId) {
        let Some(m) = self.messages.get_mut(&id) else { return };
        if m.deleted {
            return;
        }
        m.deleted = true;
        m.text.clear();
        m.reactions.clear();
        m.pinned = false;
        m.mentions.clear();
        m.mentions_all = false;
        if let Some(root) = m.thread
            && let Some(r) = self.messages.get_mut(&root)
        {
            r.reply_count = r.reply_count.saturating_sub(1);
        }
    }

    /// Rebuilds the derived indices (after deserializing a snapshot).
    pub fn reindex(&mut self) {
        self.timeline.clear();
        self.replies.clear();
        for c in self.channels.keys() {
            self.timeline.insert(*c, Vec::new());
        }
        for m in self.messages.values() {
            if let Some(root) = m.thread {
                self.replies.entry(root).or_default().push(m.id);
            }
            if m.in_timeline() {
                self.timeline.entry(m.channel).or_default().push(m.id);
            }
        }
        let msgs = &self.messages;
        for v in self.timeline.values_mut().chain(self.replies.values_mut()) {
            v.sort_by_key(|id| order_of(msgs, *id));
        }
    }

    // ---- lookups ------------------------------------------------------------------------------

    pub fn user(&self, id: UserId) -> Option<&User> {
        self.users.get(&id)
    }
    pub fn channel(&self, id: ChannelId) -> Option<&Channel> {
        self.channels.get(&id)
    }
    pub fn message(&self, id: MessageId) -> Option<&Message> {
        self.messages.get(&id)
    }
    pub fn user_by_handle(&self, handle: &str) -> Option<&User> {
        let h = handle.trim().trim_start_matches('@').to_lowercase();
        self.users.values().find(|u| u.handle == h)
    }
    pub fn channel_by_name(&self, name: &str) -> Option<&Channel> {
        let n = name.trim().trim_start_matches('#').to_lowercase();
        self.channels.values().find(|c| !c.kind.is_dm() && c.name == n)
    }
    /// `"U3"`, `"3"` or `"@ada"`.
    pub fn resolve_user(&self, s: &str) -> Option<UserId> {
        UserId::parse(s).filter(|id| self.users.contains_key(id)).or_else(|| self.user_by_handle(s).map(|u| u.id))
    }
    /// `"C2"`, `"#general"` or `"general"`; for `"@ada"` the viewer's direct message with Ada.
    pub fn resolve_channel(&self, s: &str, viewer: Option<UserId>) -> Option<ChannelId> {
        if let Some(id) = ChannelId::parse(s).filter(|id| self.channels.contains_key(id)) {
            return Some(id);
        }
        if s.trim().starts_with('@') {
            let peer = self.user_by_handle(s)?.id;
            let me = viewer?;
            let set: BTreeSet<UserId> = [me, peer].into_iter().collect();
            return self.dm_with(&set);
        }
        self.channel_by_name(s).map(|c| c.id)
    }

    /// The direct or group message with exactly these members.
    pub fn dm_with(&self, members: &BTreeSet<UserId>) -> Option<ChannelId> {
        self.channels.values().find(|c| c.kind.is_dm() && &c.members == members).map(|c| c.id)
    }

    /// Timeline messages of a conversation, oldest first. Deleted messages are left out unless
    /// they still have replies (shown as "This message was deleted.").
    pub fn timeline(&self, channel: ChannelId) -> Vec<&Message> {
        self.timeline
            .get(&channel)
            .map(|ids| ids.iter().filter_map(|id| self.messages.get(id)).filter(|m| !m.deleted || m.reply_count > 0).collect())
            .unwrap_or_default()
    }

    /// Replies in a thread, oldest first (deleted replies left out).
    pub fn replies(&self, root: MessageId) -> Vec<&Message> {
        self.replies.get(&root).map(|ids| ids.iter().filter_map(|id| self.messages.get(id)).filter(|m| !m.deleted).collect()).unwrap_or_default()
    }

    /// Whether `user` may see `channel` exists and read it.
    pub fn can_see(&self, user: UserId, channel: ChannelId) -> bool {
        let Some(c) = self.channels.get(&channel) else { return false };
        match c.kind {
            ChannelKind::Public => self.users.get(&user).is_some_and(|u| !u.deactivated && u.role != crate::Role::Guest) || c.members.contains(&user),
            _ => c.members.contains(&user),
        }
    }

    pub fn is_member(&self, user: UserId, channel: ChannelId) -> bool {
        self.channels.get(&channel).is_some_and(|c| c.members.contains(&user))
    }

    pub fn last_read(&self, user: UserId, channel: ChannelId) -> MessageId {
        self.reads.get(&user).and_then(|r| r.get(&channel)).copied().unwrap_or_default()
    }

    /// The newest timeline message in a conversation.
    pub fn latest(&self, channel: ChannelId) -> Option<MessageId> {
        self.timeline.get(&channel).and_then(|v| v.last()).copied()
    }

    /// Unread messages and mentions for `user` in `channel` (their own messages never count).
    pub fn unread(&self, user: UserId, channel: ChannelId) -> (usize, usize) {
        let read = self.last_read(user, channel);
        let dm = self.channels.get(&channel).is_some_and(|c| c.kind.is_dm());
        let mut unread = 0;
        let mut mentions = 0;
        if let Some(ids) = self.timeline.get(&channel) {
            let read_at = self.order(read);
            let start = ids.partition_point(|id| self.order(*id) <= read_at);
            for m in ids.get(start..).unwrap_or_default().iter().filter_map(|id| self.messages.get(id)) {
                if m.author == user || m.deleted {
                    continue;
                }
                unread += 1;
                if dm || m.mentions.contains(&user) || m.mentions_all {
                    mentions += 1;
                }
            }
        }
        (unread, mentions)
    }

    /// Messages that mention `user` (newest first), across the conversations they can see.
    pub fn mentions_of(&self, user: UserId, limit: usize) -> Vec<&Message> {
        let mut v: Vec<&Message> = self
            .messages
            .values()
            .filter(|m| !m.deleted && m.author != user && (m.mentions.contains(&user) || m.mentions_all) && self.can_see(user, m.channel))
            .collect();
        v.sort_by_key(|m| std::cmp::Reverse((m.ts, m.id)));
        v.truncate(limit);
        v
    }

    /// The title people see: `#name` for channels; other members' names for DMs.
    pub fn title(&self, channel: ChannelId, viewer: Option<UserId>) -> String {
        let Some(c) = self.channels.get(&channel) else { return String::new() };
        if !c.kind.is_dm() {
            return format!("#{}", c.name);
        }
        let others: Vec<&str> = c.members.iter().filter(|u| Some(**u) != viewer).filter_map(|u| self.users.get(u)).map(User::name).collect();
        if others.is_empty() {
            viewer.and_then(|v| self.users.get(&v)).map(|u| format!("{} (you)", u.name())).unwrap_or_default()
        } else {
            others.join(", ")
        }
    }

    /// The sidebar list for a viewer: conversations they belong to.
    pub fn summaries(&self, viewer: UserId) -> Vec<ChannelSummary> {
        let muted = self.muted.get(&viewer);
        let starred = self.starred.get(&viewer);
        self.channels
            .values()
            .filter(|c| c.members.contains(&viewer))
            .map(|c| {
                let (unread, mentions) = self.unread(viewer, c.id);
                let last_activity = self.latest(c.id).and_then(|m| self.messages.get(&m)).map_or(c.created, |m| m.ts);
                let peer = if c.kind.is_dm() { c.members.iter().find(|u| **u != viewer).copied().or(Some(viewer)) } else { None };
                ChannelSummary {
                    id: c.id,
                    title: self.title(c.id, Some(viewer)),
                    kind: c.kind,
                    unread,
                    mentions,
                    muted: muted.is_some_and(|m| m.contains(&c.id)),
                    starred: starred.is_some_and(|s| s.contains(&c.id)),
                    archived: c.archived,
                    last_activity,
                    peer,
                }
            })
            .collect()
    }

    /// Whether `viewer` should receive `event` (evaluated on the state *after* applying it).
    pub fn event_visible(&self, event: &Event, viewer: UserId) -> bool {
        if let Some(u) = event.private_to() {
            return u == viewer;
        }
        // The person who joined or left always hears about it, even when they can no longer see.
        if let Event::MemberJoined { user, .. } | Event::MemberLeft { user, .. } = event
            && *user == viewer
        {
            return true;
        }
        if let Some(c) = event.channel() {
            return self.can_see(viewer, c);
        }
        if let Some(m) = event.message() {
            return self.messages.get(&m).is_some_and(|m| self.can_see(viewer, m.channel));
        }
        true
    }

    /// A copy holding only what `viewer` may see (their own reads, saved items, mutes, stars).
    pub fn visible_to(&self, viewer: UserId) -> State {
        let mut s = self.clone();
        s.retain_visible(viewer);
        s
    }

    /// Drops what `viewer` may no longer see (after leaving a private conversation).
    pub fn retain_visible(&mut self, viewer: UserId) {
        let hidden: BTreeSet<ChannelId> = self.channels.keys().copied().filter(|c| !self.can_see(viewer, *c)).collect();
        if !hidden.is_empty() {
            self.channels.retain(|id, _| !hidden.contains(id));
            self.messages.retain(|_, m| !hidden.contains(&m.channel));
            for id in &hidden {
                self.timeline.remove(id);
            }
            let msgs = &self.messages;
            self.replies.retain(|root, _| msgs.contains_key(root));
        }
        self.reads.retain(|u, _| *u == viewer);
        self.saved.retain(|u, _| *u == viewer);
        self.muted.retain(|u, _| *u == viewer);
        self.starred.retain(|u, _| *u == viewer);
    }

    /// The highest ids in use (the hub continues numbering after them).
    pub fn max_ids(&self) -> (UserId, ChannelId, MessageId) {
        (
            self.users.keys().next_back().copied().unwrap_or_default(),
            self.channels.keys().next_back().copied().unwrap_or_default(),
            self.messages.keys().next_back().copied().unwrap_or_default(),
        )
    }
}

fn toggle<T: Ord>(set: &mut BTreeSet<T>, item: T, on: bool) {
    if on {
        set.insert(item);
    } else {
        set.remove(&item);
    }
}
