//! Matrix ↔ ChatCraft mapping. Pure and synchronous: it turns Matrix rooms, members and event
//! JSON into ChatCraft [`Event`]s, keeps the replica it implies, and remembers which Matrix id
//! each local id stands for. All Matrix input is untrusted.

use std::collections::{BTreeSet, HashMap, HashSet};

use chatcraft_core::{ChannelId, MessageId, Ts, UserId};
use chatcraft_model::{Channel, ChannelKind, Event, Message, Presence, Sequenced, State, Status, User};
use chatcraft_text::emoji;
use serde_json::Value;

/// Interns string ids as small numbers, both ways.
#[derive(Default)]
struct Interner {
    to_num: HashMap<String, u64>,
    to_str: HashMap<u64, String>,
    next: u64,
}

impl Interner {
    fn get_or_add(&mut self, s: &str) -> u64 {
        if let Some(n) = self.to_num.get(s) {
            return *n;
        }
        self.next = self.next.saturating_add(1);
        self.to_num.insert(s.to_string(), self.next);
        self.to_str.insert(self.next, s.to_string());
        self.next
    }
    fn get(&self, s: &str) -> Option<u64> {
        self.to_num.get(s).copied()
    }
    fn name(&self, n: u64) -> Option<&str> {
        self.to_str.get(&n).map(String::as_str)
    }
}

/// What the adapter knows about a room after a sync.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomInfo {
    pub room_id: String,
    /// The room's name, or its computed display name.
    pub name: String,
    pub topic: String,
    pub public: bool,
    pub direct: bool,
    pub encrypted: bool,
    pub favourite: bool,
    /// Joined members: `(mxid, display name)`.
    pub members: Vec<(String, Option<String>)>,
    /// Pinned event ids (`m.room.pinned_events`).
    pub pinned: Vec<String>,
    /// Notifications muted for us (push rules).
    pub muted: bool,
}

/// The mapper.
#[derive(Default)]
pub struct Mapper {
    users: Interner,
    rooms: Interner,
    events: Interner,
    /// Reaction event id → what it reacted with (for removals by redaction).
    reactions: HashMap<String, (MessageId, String, UserId)>,
    /// Event ids already handled (local echoes, then the same events from sync).
    seen: HashSet<String>,
    state: State,
    out: Vec<Sequenced>,
    me: Option<UserId>,
}

/// `@ada:example.org` → `ada`.
fn localpart(mxid: &str) -> &str {
    mxid.trim_start_matches('@').split(':').next().unwrap_or(mxid)
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(k)).and_then(Value::as_str)
}

impl Mapper {
    pub fn new(me: &str) -> Mapper {
        let mut m = Mapper::default();
        // A Matrix account has no workspace; the homeserver's name stands in for it.
        let server = me.split_once(':').map_or("Matrix", |(_, s)| s).to_string();
        m.emit(Event::WorkspaceCreated { name: server, at: Ts(0) });
        let id = m.user(me, None);
        m.me = Some(id);
        m
    }

    pub fn me(&self) -> Option<UserId> {
        self.me
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Events produced since the last call.
    pub fn take(&mut self) -> Vec<Sequenced> {
        std::mem::take(&mut self.out)
    }

    fn emit(&mut self, event: Event) {
        let s = Sequenced { seq: self.state.seq.saturating_add(1), event };
        self.state.apply(&s);
        self.out.push(s);
    }

    // ---- ids ----------------------------------------------------------------------------------

    pub fn mxid(&self, u: UserId) -> Option<&str> {
        self.users.name(u.0)
    }
    pub fn room_id(&self, c: ChannelId) -> Option<&str> {
        self.rooms.name(c.0)
    }
    pub fn event_id(&self, m: MessageId) -> Option<&str> {
        self.events.name(m.0)
    }
    pub fn channel_for(&mut self, room_id: &str) -> ChannelId {
        ChannelId(self.rooms.get_or_add(room_id))
    }
    pub fn known_channel(&self, room_id: &str) -> Option<ChannelId> {
        self.rooms.get(room_id).map(ChannelId)
    }
    pub fn message_for(&mut self, event_id: &str) -> MessageId {
        MessageId(self.events.get_or_add(event_id))
    }
    /// The reaction event by which `user` reacted to `message` with `emoji`.
    pub fn reaction_event(&self, message: MessageId, emoji: &str, user: UserId) -> Option<&str> {
        self.reactions.iter().find(|(_, (m, e, u))| *m == message && e == emoji && *u == user).map(|(id, _)| id.as_str())
    }

    /// A unique handle: the localpart, or `localpart:server` when two servers share a localpart.
    fn handle_for(&self, mxid: &str) -> String {
        let local = localpart(mxid).to_lowercase();
        let taken = self.state.users.values().any(|u| u.handle == local && self.users.name(u.id.0) != Some(mxid));
        if taken { mxid.trim_start_matches('@').to_lowercase() } else { local }
    }

    /// Interns a user, announcing them (or their new name) to the replica. `None` means the name
    /// isn't known here (a message's sender), which never overwrites a known name.
    pub fn user(&mut self, mxid: &str, display_name: Option<&str>) -> UserId {
        let id = UserId(self.users.get_or_add(mxid));
        let given = display_name.map(str::trim).filter(|n| !n.is_empty());
        let name = given.unwrap_or_else(|| localpart(mxid)).to_string();
        match self.state.user(id) {
            Some(u) if u.display_name == name || given.is_none() => {}
            Some(u) => {
                let mut u = u.clone();
                u.display_name = name;
                self.emit(Event::UserJoined { user: u });
            }
            None => {
                let user =
                    User { id, handle: self.handle_for(mxid), display_name: name, color: u8::try_from(id.0 % 12).unwrap_or(0), ..User::default() };
                self.emit(Event::UserJoined { user });
            }
        }
        id
    }

    /// Resolves `@handle` / `@mxid` text to a known user.
    pub fn resolve_mention(&self, handle: &str) -> Option<UserId> {
        self.state.resolve_user(handle)
    }

    // ---- rooms --------------------------------------------------------------------------------

    fn slug(&self, raw: &str, me: ChannelId) -> String {
        let base = chatcraft_core::normalize_channel_name(&raw.replace(|c: char| !c.is_alphanumeric() && c != ' ' && c != '-' && c != '_', ""))
            .unwrap_or_else(|_| format!("room-{}", me.0));
        let clash = |n: &str| self.state.channels.values().any(|c| c.id != me && !c.kind.is_dm() && c.name == n);
        if !clash(&base) {
            return base;
        }
        (2..1000).map(|i| format!("{base}-{i}")).find(|n| !clash(n)).unwrap_or_else(|| format!("{base}-{}", me.0))
    }

    /// Creates or updates a room's channel from what the SDK knows.
    pub fn upsert_room(&mut self, info: &RoomInfo) -> ChannelId {
        let id = self.channel_for(&info.room_id);
        let mut members: BTreeSet<UserId> = info.members.iter().map(|(mxid, dn)| self.user(mxid, dn.as_deref())).collect();
        // Every room upserted here is one we're in, even before our own membership event syncs.
        members.extend(self.me);
        // A conversation we know to be direct stays direct: `m.direct` can lag behind a sync.
        let direct = info.direct || self.state.channel(id).is_some_and(|c| c.kind.is_dm());
        let kind = if direct {
            if members.len() <= 2 { ChannelKind::Direct } else { ChannelKind::Group }
        } else if info.public {
            ChannelKind::Public
        } else {
            ChannelKind::Private
        };
        let name = if kind.is_dm() { String::new() } else { self.slug(&info.name, id) };
        let prev = self.state.channel(id).cloned();
        let channel = Channel {
            id,
            name,
            kind,
            topic: info.topic.clone(),
            purpose: prev.as_ref().map(|c| c.purpose.clone()).unwrap_or_default(),
            members,
            archived: false,
            encrypted: info.encrypted,
            created_by: prev.as_ref().map(|c| c.created_by).unwrap_or_default(),
            created: prev.as_ref().map(|c| c.created).unwrap_or_default(),
        };
        if prev.as_ref() != Some(&channel) {
            self.emit(Event::ChannelCreated { channel });
        }
        if let Some(me) = self.me {
            let starred = self.state.starred.get(&me).is_some_and(|s| s.contains(&id));
            if starred != info.favourite {
                self.emit(Event::ChannelStarred { user: me, channel: id, starred: info.favourite });
            }
            self.set_muted(id, info.muted);
        }
        // Pins: the room's list is the truth for the messages we know.
        let pinned: HashSet<MessageId> = info.pinned.iter().filter_map(|e| self.events.get(e)).map(MessageId).collect();
        let changes: Vec<(MessageId, bool)> = self
            .state
            .messages
            .values()
            .filter(|m| m.channel == id && !m.deleted && m.pinned != pinned.contains(&m.id))
            .map(|m| (m.id, !m.pinned))
            .collect();
        for (m, on) in changes {
            self.set_pinned(m, on);
        }
        id
    }

    pub fn set_pinned(&mut self, message: MessageId, pinned: bool) {
        let by = self.me.unwrap_or_default();
        if self.state.message(message).is_some_and(|m| m.pinned != pinned) {
            self.emit(Event::PinChanged { message, pinned, by });
        }
    }

    pub fn set_muted(&mut self, channel: ChannelId, muted: bool) {
        let Some(me) = self.me else { return };
        if self.state.muted.get(&me).is_some_and(|s| s.contains(&channel)) != muted {
            self.emit(Event::ChannelMuted { user: me, channel, muted });
        }
    }

    /// Saved items, from the account's `dev.chatcraft.saved` list of `(room, event)` ids.
    pub fn set_saved_list(&mut self, list: &[(String, String)]) {
        let Some(me) = self.me else { return };
        let want: BTreeSet<MessageId> = list.iter().filter_map(|(_, e)| self.events.get(e)).map(MessageId).collect();
        let have = self.state.saved.get(&me).cloned().unwrap_or_default();
        for m in want.difference(&have).copied().collect::<Vec<_>>() {
            self.emit(Event::SavedChanged { user: me, message: m, saved: true });
        }
        for m in have.difference(&want).copied().collect::<Vec<_>>() {
            self.emit(Event::SavedChanged { user: me, message: m, saved: false });
        }
    }

    /// The viewer's trust in someone's identity.
    pub fn set_verified(&mut self, mxid: &str, verified: bool) {
        let Some(u) = self.users.get(mxid).map(UserId) else { return };
        if self.state.user(u).is_some_and(|x| x.verified != verified) {
            self.emit(Event::UserVerified { user: u, verified });
        }
    }

    pub fn starred(&mut self, c: ChannelId, on: bool) {
        if let Some(me) = self.me {
            self.emit(Event::ChannelStarred { user: me, channel: c, starred: on });
        }
    }

    /// We left (or were removed from) a room.
    pub fn left_room(&mut self, room_id: &str) {
        let (Some(id), Some(me)) = (self.known_channel(room_id), self.me) else { return };
        if self.state.is_member(me, id) {
            self.emit(Event::MemberLeft { channel: id, user: me, by: None });
            self.state.retain_visible(me);
        }
    }

    pub fn fully_read(&mut self, room_id: &str, event_id: &str) {
        let (Some(c), Some(me), Some(m)) = (self.known_channel(room_id), self.me, self.events.get(event_id).map(MessageId)) else { return };
        if m > self.state.last_read(me, c) {
            self.emit(Event::ReadMarked { user: me, channel: c, upto: m });
        }
    }

    /// `m.presence`: `online`/`unavailable`/`offline`; status text may start with `:emoji:`.
    pub fn presence(&mut self, mxid: &str, presence: &str, status_msg: Option<&str>) {
        let Some(u) = self.users.get(mxid).map(UserId) else { return };
        let p = match presence {
            "online" => Presence::Active,
            "unavailable" => Presence::Away,
            _ => Presence::Offline,
        };
        if self.state.user(u).is_some_and(|x| x.presence != p) {
            self.emit(Event::PresenceChanged { user: u, presence: p });
        }
        let status = parse_status(status_msg.unwrap_or_default());
        if self.state.user(u).is_some_and(|x| x.status != status) {
            self.emit(Event::StatusChanged { user: u, status });
        }
    }

    // ---- timeline -----------------------------------------------------------------------------

    /// One timeline event as JSON (decrypted when it was encrypted). `utd`: the device couldn't
    /// decrypt it.
    #[allow(clippy::cognitive_complexity)]
    pub fn timeline(&mut self, room_id: &str, ev: &Value, encrypted: bool, utd: bool) {
        let (Some(event_id), Some(sender)) = (str_at(ev, &["event_id"]), str_at(ev, &["sender"])) else { return };
        // An event shown as undecryptable may come back decrypted (late key, key backup).
        let placeholder = self.events.get(event_id).map(MessageId).filter(|m| self.state.message(*m).is_some_and(|x| x.undecryptable));
        if !self.seen.insert(event_id.to_string()) && (placeholder.is_none() || utd) {
            return;
        }
        let Some(channel) = self.known_channel(room_id) else { return };
        let author = self.user(sender, None);
        let ts = Ts(ev.get("origin_server_ts").and_then(Value::as_i64).unwrap_or(0));
        let kind = str_at(ev, &["type"]).unwrap_or_default();
        let content = ev.get("content").cloned().unwrap_or(Value::Null);
        let rel = content.get("m.relates_to");
        let rel_type = rel.and_then(|r| str_at(r, &["rel_type"]));
        let rel_target = rel.and_then(|r| str_at(r, &["event_id"]));
        match (kind, rel_type) {
            ("m.room.encrypted", _) if utd => {
                let id = self.message_for(event_id);
                let message = Message { id, channel, author, ts, encrypted: true, undecryptable: true, ..Message::default() };
                self.emit(Event::MessagePosted { message });
            }
            ("m.room.message", Some("m.replace")) => {
                let new_content = content.get("m.new_content").cloned().unwrap_or(Value::Null);
                let (Some(target), Some(text)) = (rel_target, self.text_of(&new_content)) else { return };
                let text = text.as_str();
                let Some(mid) = self.events.get(target).map(MessageId) else { return };
                // Only the author may edit; anything else is forged and ignored.
                if self.state.message(mid).is_some_and(|m| m.author == author) {
                    let (mentions, all) = self.mentions(content.get("m.new_content").unwrap_or(&Value::Null), text);
                    self.emit(Event::MessageEdited {
                        message: mid,
                        text: text.to_string(),
                        at: ts,
                        mentions: mentions.into_iter().collect(),
                        mentions_all: all,
                    });
                }
            }
            // Verification requests travel as room messages; they're not chat.
            ("m.room.message", _) if str_at(&content, &["msgtype"]).is_some_and(|t| t.starts_with("m.key.verification")) => {}
            ("m.room.message", _) => {
                let Some(text) = self.text_of(&content) else { return };
                let text = text.as_str();
                let thread = match rel_type {
                    Some("m.thread") => rel_target.and_then(|t| self.events.get(t)).map(MessageId),
                    _ => None,
                };
                let (mentions, mentions_all) = self.mentions(&content, text);
                let id = self.message_for(event_id);
                let message =
                    Message { id, channel, author, ts, text: text.to_string(), thread, mentions, mentions_all, encrypted, ..Message::default() };
                if placeholder.is_some() {
                    self.emit(Event::MessageReplaced { message });
                } else {
                    self.emit(Event::MessagePosted { message });
                }
                return;
            }
            ("m.reaction", Some("m.annotation")) => {
                let (Some(target), Some(key)) = (rel_target, rel.and_then(|r| str_at(r, &["key"]))) else { return };
                let Some(mid) = self.events.get(target).map(MessageId) else { return };
                let emoji = emoji::shortcode_for(key).map(str::to_string).unwrap_or_else(|| key.trim_matches(':').to_string());
                if emoji.is_empty() || emoji.len() > 64 {
                    return;
                }
                self.reactions.insert(event_id.to_string(), (mid, emoji.clone(), author));
                self.emit(Event::ReactionAdded { message: mid, emoji, user: author });
            }
            ("m.room.redaction", _) => {
                let Some(target) = str_at(ev, &["redacts"]).or_else(|| str_at(&content, &["redacts"])) else { return };
                if let Some((mid, emoji, user)) = self.reactions.remove(target) {
                    self.emit(Event::ReactionRemoved { message: mid, emoji, user });
                } else if let Some(mid) = self.events.get(target).map(MessageId) {
                    // The homeserver only accepts redactions from the sender or a moderator.
                    self.emit(Event::MessageDeleted { message: mid, at: ts });
                }
            }
            _ => {}
        }
        // A placeholder that turned out to be a reaction, edit or redaction isn't a message.
        if let Some(p) = placeholder {
            self.emit(Event::MessageDeleted { message: p, at: ts });
        }
    }

    /// The message text in ChatCraft markup: the HTML `formatted_body` when there is one (it keeps
    /// the formatting), else the plain `body`.
    fn text_of(&self, content: &Value) -> Option<String> {
        let html = (str_at(content, &["format"]) == Some("org.matrix.custom.html")).then(|| str_at(content, &["formatted_body"])).flatten();
        let body = str_at(content, &["body"]);
        match html {
            // ChatCraft wrote it (our renderer reproduces the HTML from the body): the body is
            // the exact source, shortcodes and all.
            Some(h)
                if body.is_some_and(|b| {
                    let mxid_of = |handle: &str| self.state.resolve_user(handle).and_then(|u| self.users.name(u.0)).map(str::to_string);
                    crate::html::from_markup(b, &mxid_of) == h
                }) =>
            {
                body.map(str::to_string)
            }
            Some(h) => {
                let handle = |mxid: &str| match self.users.get(mxid).and_then(|u| self.state.user(UserId(u))) {
                    Some(u) => format!("@{}", u.handle),
                    None => format!("@{}", localpart(mxid)),
                };
                Some(crate::html::to_markup(h, &handle)).filter(|t| !t.trim().is_empty()).or_else(|| str_at(content, &["body"]).map(str::to_string))
            }
            None => str_at(content, &["body"]).map(str::to_string),
        }
    }

    /// Mentions: intentional (`m.mentions`), else `@handle` in the text.
    fn mentions(&self, content: &Value, text: &str) -> (BTreeSet<UserId>, bool) {
        let mut ids: BTreeSet<UserId> = BTreeSet::new();
        if let Some(list) = content.get("m.mentions").and_then(|m| m.get("user_ids")).and_then(Value::as_array) {
            ids.extend(list.iter().filter_map(Value::as_str).filter_map(|m| self.users.get(m)).map(UserId));
        }
        ids.extend(chatcraft_text::mentions(text).iter().filter_map(|h| self.state.resolve_user(h)));
        let room = content.get("m.mentions").and_then(|m| m.get("room")).and_then(Value::as_bool).unwrap_or(false);
        (ids, room || chatcraft_text::broadcasts(text))
    }
}

/// `":palm_tree: On holiday"` → emoji `palm_tree`, text `On holiday`.
pub fn parse_status(msg: &str) -> Status {
    let msg = msg.trim();
    if let Some(rest) = msg.strip_prefix(':')
        && let Some((code, text)) = rest.split_once(':')
        && emoji::valid_name(code)
    {
        return Status { text: text.trim().to_string(), emoji: code.to_string(), until: None };
    }
    Status { text: msg.to_string(), emoji: String::new(), until: None }
}

/// The inverse of [`parse_status`].
pub fn format_status(text: &str, emoji: &str) -> String {
    if emoji.is_empty() { text.to_string() } else { format!(":{emoji}: {text}").trim().to_string() }
}

#[cfg(test)]
mod tests;
