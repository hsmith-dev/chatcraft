//! Operation planning: permission and limit checks, then the events an op produces.

use std::collections::BTreeSet;

use chatcraft_core::limits::{MAX_GROUP_DM, MAX_MESSAGE_BYTES, MAX_REACTIONS_PER_MESSAGE, MAX_STATUS};
use chatcraft_core::{ChannelId, MessageId, Ts, UserId, normalize_channel_name};
use chatcraft_model::{Channel, ChannelKind, Event, Message, Role, State, Status};
use chatcraft_proto::Op;
use serde_json::{Value, json};

use crate::{Hub, HubError, clean_label};

type Plan = (Vec<Event>, Value, Vec<UserId>);

fn ok(events: Vec<Event>) -> Result<Plan, HubError> {
    Ok((events, Value::Null, Vec::new()))
}

fn channel(s: &State, id: ChannelId, actor: UserId) -> Result<&Channel, HubError> {
    // Private conversations the actor can't see are reported as missing, not forbidden, so ids
    // can't be probed.
    s.channel(id).filter(|_| s.can_see(actor, id)).ok_or_else(|| HubError::NotFound(format!("channel {id}")))
}

fn member(s: &State, id: ChannelId, actor: UserId) -> Result<&Channel, HubError> {
    let c = channel(s, id, actor)?;
    if !c.members.contains(&actor) {
        return Err(HubError::Forbidden(format!("join #{} first", c.name)));
    }
    Ok(c)
}

fn writable(s: &State, id: ChannelId, actor: UserId) -> Result<&Channel, HubError> {
    let c = member(s, id, actor)?;
    if c.archived {
        return Err(HubError::Forbidden("the channel is archived".into()));
    }
    Ok(c)
}

fn message(s: &State, id: MessageId, actor: UserId) -> Result<&Message, HubError> {
    s.message(id).filter(|m| !m.deleted && s.can_see(actor, m.channel)).ok_or_else(|| HubError::NotFound(format!("message {id}")))
}

fn is_admin(s: &State, actor: UserId) -> bool {
    s.user(actor).is_some_and(|u| matches!(u.role, Role::Owner | Role::Admin))
}

/// Channel managers: its creator and workspace admins.
fn manages(s: &State, c: &Channel, actor: UserId) -> bool {
    c.created_by == actor || is_admin(s, actor)
}

fn is_default(s: &State, id: ChannelId) -> bool {
    s.workspace.default_channel == Some(id)
}

fn body(text: &str) -> Result<String, HubError> {
    let t = text.trim_end();
    if t.trim().is_empty() {
        return Err(HubError::Invalid("message is empty".into()));
    }
    if t.len() > MAX_MESSAGE_BYTES {
        return Err(HubError::Invalid(format!("message is longer than {MAX_MESSAGE_BYTES} bytes")));
    }
    Ok(t.to_string())
}

/// `@handle` mentions that name real people, and whether `@here`/`@channel` was used.
fn mentions(s: &State, text: &str) -> (BTreeSet<UserId>, bool) {
    let ids = chatcraft_text::mentions(text).iter().filter_map(|h| s.user_by_handle(h)).map(|u| u.id).collect();
    (ids, chatcraft_text::broadcasts(text))
}

fn unique_name(s: &State, raw: &str, except: Option<ChannelId>) -> Result<String, HubError> {
    let name = normalize_channel_name(raw).map_err(|e| HubError::Invalid(format!("channel name: {e}")))?;
    if s.channels.values().any(|c| !c.kind.is_dm() && c.name == name && Some(c.id) != except) {
        return Err(HubError::Invalid(format!("#{name} already exists")));
    }
    Ok(name)
}

fn users_exist(s: &State, users: &[UserId]) -> Result<(), HubError> {
    match users.iter().find(|u| s.user(**u).is_none_or(|u| u.deactivated)) {
        Some(u) => Err(HubError::NotFound(format!("user {u}"))),
        None => Ok(()),
    }
}

#[allow(clippy::cognitive_complexity)]
pub(crate) fn plan(hub: &mut Hub, actor: UserId, op: Op, now: Ts) -> Result<Plan, HubError> {
    if let Op::CreateChannel { external, .. } | Op::Invite { external, .. } | Op::OpenDm { external, .. } = &op
        && let Some(x) = external.first()
    {
        return Err(HubError::NotFound(format!("user {x}")));
    }
    match op {
        Op::PostMessage { channel, text, thread, broadcast } => post(hub, actor, channel, &text, thread, broadcast, now),
        Op::EditMessage { message: id, text } => {
            let s = &hub.state;
            let m = message(s, id, actor)?;
            if m.author != actor {
                return Err(HubError::Forbidden("only the author can edit a message".into()));
            }
            writable(s, m.channel, actor)?;
            let text = body(&text)?;
            if text == m.text {
                return ok(Vec::new());
            }
            let (ids, all) = mentions(s, &text);
            ok(vec![Event::MessageEdited { message: id, text, at: now, mentions: ids.into_iter().collect(), mentions_all: all }])
        }
        Op::DeleteMessage { message: id } => {
            let s = &hub.state;
            let m = message(s, id, actor)?;
            if m.author != actor && !is_admin(s, actor) {
                return Err(HubError::Forbidden("only the author or an admin can delete a message".into()));
            }
            ok(vec![Event::MessageDeleted { message: id, at: now }])
        }
        Op::React { message: id, emoji, on } => {
            let s = &hub.state;
            let m = message(s, id, actor)?;
            writable(s, m.channel, actor)?;
            let emoji = emoji.trim().trim_matches(':').to_string();
            let emoji = chatcraft_text::emoji::shortcode_for(&emoji).map(str::to_string).unwrap_or(emoji);
            if !chatcraft_text::emoji::valid_name(&emoji) {
                return Err(HubError::Invalid(format!("`{emoji}` is not an emoji shortcode")));
            }
            let has = m.reacted(&emoji, actor);
            if on == has {
                return ok(Vec::new());
            }
            if on && !m.reactions.iter().any(|(e, _)| *e == emoji) && m.reactions.len() >= MAX_REACTIONS_PER_MESSAGE {
                return Err(HubError::Invalid(format!("a message can have at most {MAX_REACTIONS_PER_MESSAGE} different reactions")));
            }
            ok(vec![if on {
                Event::ReactionAdded { message: id, emoji, user: actor }
            } else {
                Event::ReactionRemoved { message: id, emoji, user: actor }
            }])
        }
        Op::Pin { message: id, pinned } => {
            let s = &hub.state;
            let m = message(s, id, actor)?;
            writable(s, m.channel, actor)?;
            if m.pinned == pinned {
                return ok(Vec::new());
            }
            ok(vec![Event::PinChanged { message: id, pinned, by: actor }])
        }
        Op::CreateChannel { name, private, topic, purpose, members, .. } => {
            let s = &hub.state;
            let name = unique_name(s, &name, None)?;
            users_exist(s, &members)?;
            let topic = clean_label(&topic)?;
            let purpose = clean_label(&purpose)?;
            let mut set: BTreeSet<UserId> = members.into_iter().collect();
            set.insert(actor);
            let id = hub.alloc_channel();
            let kind = if private { ChannelKind::Private } else { ChannelKind::Public };
            let channel =
                Channel { id, name, kind, topic, purpose, members: set, archived: false, encrypted: false, created_by: actor, created: now };
            Ok((vec![Event::ChannelCreated { channel }], json!({ "channel": id }), Vec::new()))
        }
        Op::JoinChannel { channel: id } => {
            let s = &hub.state;
            let c = channel(s, id, actor)?;
            if c.members.contains(&actor) {
                return Ok((Vec::new(), json!({ "channel": id }), Vec::new()));
            }
            if c.kind != ChannelKind::Public {
                return Err(HubError::Forbidden("private conversations are invite-only".into()));
            }
            if c.archived {
                return Err(HubError::Forbidden("the channel is archived".into()));
            }
            Ok((vec![Event::MemberJoined { channel: id, user: actor, by: None }], json!({ "channel": id }), Vec::new()))
        }
        Op::LeaveChannel { channel: id } => {
            let s = &hub.state;
            let c = member(s, id, actor)?;
            if is_default(s, id) {
                return Err(HubError::Forbidden(format!("everyone stays in #{}", c.name)));
            }
            if c.kind.is_dm() {
                return Err(HubError::Invalid("direct messages can be closed, not left".into()));
            }
            ok(vec![Event::MemberLeft { channel: id, user: actor, by: None }])
        }
        Op::Invite { channel: id, users, .. } => {
            let s = &hub.state;
            let c = writable(s, id, actor)?;
            if c.kind.is_dm() {
                return Err(HubError::Invalid("people can't be added to a direct message; start a new group message".into()));
            }
            users_exist(s, &users)?;
            let new: BTreeSet<UserId> = users.into_iter().filter(|u| !c.members.contains(u)).collect();
            let resync = if c.kind == ChannelKind::Private { new.iter().copied().collect() } else { Vec::new() };
            let evs = new.into_iter().map(|u| Event::MemberJoined { channel: id, user: u, by: Some(actor) }).collect();
            Ok((evs, Value::Null, resync))
        }
        Op::Remove { channel: id, user } => {
            let s = &hub.state;
            let c = member(s, id, actor)?;
            if !manages(s, c, actor) {
                return Err(HubError::Forbidden("only the channel's creator or an admin can remove people".into()));
            }
            if is_default(s, id) || c.kind.is_dm() {
                return Err(HubError::Forbidden("people can't be removed from this conversation".into()));
            }
            if !c.members.contains(&user) {
                return ok(Vec::new());
            }
            ok(vec![Event::MemberLeft { channel: id, user, by: Some(actor) }])
        }
        Op::RenameChannel { channel: id, name } => {
            let s = &hub.state;
            let c = writable(s, id, actor)?;
            if c.kind.is_dm() {
                return Err(HubError::Invalid("direct messages have no name".into()));
            }
            if !manages(s, c, actor) {
                return Err(HubError::Forbidden("only the channel's creator or an admin can rename it".into()));
            }
            let name = unique_name(s, &name, Some(id))?;
            ok(vec![Event::ChannelRenamed { channel: id, name }])
        }
        Op::SetTopic { channel: id, topic } => {
            writable(&hub.state, id, actor)?;
            ok(vec![Event::TopicChanged { channel: id, topic: clean_label(&topic)?, by: actor }])
        }
        Op::SetPurpose { channel: id, purpose } => {
            writable(&hub.state, id, actor)?;
            ok(vec![Event::PurposeChanged { channel: id, purpose: clean_label(&purpose)?, by: actor }])
        }
        Op::Archive { channel: id, archived } => {
            let s = &hub.state;
            let c = member(s, id, actor)?;
            if is_default(s, id) || c.kind.is_dm() {
                return Err(HubError::Forbidden("this conversation can't be archived".into()));
            }
            if !manages(s, c, actor) {
                return Err(HubError::Forbidden("only the channel's creator or an admin can archive it".into()));
            }
            if c.archived == archived {
                return ok(Vec::new());
            }
            ok(vec![Event::ChannelArchived { channel: id, archived, by: actor }])
        }
        Op::OpenDm { users, .. } => {
            let s = &hub.state;
            users_exist(s, &users)?;
            let mut set: BTreeSet<UserId> = users.into_iter().collect();
            set.insert(actor);
            if set.len() > MAX_GROUP_DM {
                return Err(HubError::Invalid(format!("a group message can have at most {MAX_GROUP_DM} people")));
            }
            if let Some(id) = s.dm_with(&set) {
                return Ok((Vec::new(), json!({ "channel": id }), Vec::new()));
            }
            let id = hub.alloc_channel();
            let kind = if set.len() <= 2 { ChannelKind::Direct } else { ChannelKind::Group };
            let channel = Channel { id, name: String::new(), kind, members: set, created_by: actor, created: now, ..Channel::default() };
            Ok((vec![Event::ChannelCreated { channel }], json!({ "channel": id }), Vec::new()))
        }
        Op::SetProfile { display_name, title } => {
            let display_name = display_name.as_deref().map(clean_label).transpose()?;
            if display_name.as_deref().is_some_and(str::is_empty) {
                return Err(HubError::Invalid("display name can't be empty".into()));
            }
            let title = title.as_deref().map(clean_label).transpose()?;
            ok(vec![Event::ProfileChanged { user: actor, display_name, title }])
        }
        Op::SetStatus { text, emoji, minutes } => {
            let text = clean_label(&text)?;
            if text.chars().count() > MAX_STATUS {
                return Err(HubError::Invalid(format!("status is longer than {MAX_STATUS} characters")));
            }
            let emoji = emoji.trim().trim_matches(':').to_string();
            if !emoji.is_empty() && !chatcraft_text::emoji::valid_name(&emoji) {
                return Err(HubError::Invalid(format!("`{emoji}` is not an emoji shortcode")));
            }
            let until = minutes.map(|m| now.plus_millis(i64::from(m).saturating_mul(60_000)));
            ok(vec![Event::StatusChanged { user: actor, status: Status { text, emoji, until } }])
        }
        Op::SetPresence { presence } => {
            if hub.state.user(actor).is_some_and(|u| u.presence == presence) {
                return ok(Vec::new());
            }
            ok(vec![Event::PresenceChanged { user: actor, presence }])
        }
        Op::MarkRead { channel: id, upto } => {
            let s = &hub.state;
            channel(s, id, actor)?;
            let upto = upto.min(s.latest(id).unwrap_or_default());
            if upto <= s.last_read(actor, id) {
                return ok(Vec::new());
            }
            ok(vec![Event::ReadMarked { user: actor, channel: id, upto }])
        }
        Op::Save { message: id, saved } => {
            let s = &hub.state;
            message(s, id, actor)?;
            if s.saved.get(&actor).is_some_and(|set| set.contains(&id)) == saved {
                return ok(Vec::new());
            }
            ok(vec![Event::SavedChanged { user: actor, message: id, saved }])
        }
        Op::Mute { channel: id, muted } => {
            member(&hub.state, id, actor)?;
            ok(vec![Event::ChannelMuted { user: actor, channel: id, muted }])
        }
        Op::Star { channel: id, starred } => {
            member(&hub.state, id, actor)?;
            ok(vec![Event::ChannelStarred { user: actor, channel: id, starred }])
        }
        Op::VerifyUser { .. }
        | Op::Verification { .. }
        | Op::SetUpRecovery
        | Op::Recover { .. }
        | Op::SignOut
        | Op::Directory { .. }
        | Op::JoinExternal { .. }
        | Op::Devices
        | Op::VerifyDevice { .. }
        | Op::RemoveDevice { .. } => Err(HubError::Invalid("this needs a Matrix account".into())),
        Op::RenameWorkspace { name } => {
            if !is_admin(&hub.state, actor) {
                return Err(HubError::Forbidden("only admins can rename the workspace".into()));
            }
            let name = clean_label(&name)?;
            if name.is_empty() {
                return Err(HubError::Invalid("the workspace needs a name".into()));
            }
            ok(vec![Event::WorkspaceRenamed { name }])
        }
    }
}

fn post(
    hub: &mut Hub,
    actor: UserId,
    channel_id: ChannelId,
    text: &str,
    thread: Option<MessageId>,
    broadcast: bool,
    now: Ts,
) -> Result<Plan, HubError> {
    let s = &hub.state;
    writable(s, channel_id, actor)?;
    let text = body(text)?;
    let thread = match thread {
        None => None,
        Some(t) => {
            let root = message(s, t, actor)?;
            if root.channel != channel_id {
                return Err(HubError::Invalid("the thread is in another conversation".into()));
            }
            // Replying to a reply continues the same thread.
            Some(root.thread.unwrap_or(root.id))
        }
    };
    let (ids, all) = mentions(s, &text);
    let id = hub.alloc_message();
    let message = Message {
        id,
        channel: channel_id,
        author: actor,
        ts: now,
        text,
        thread,
        broadcast: thread.is_some() && broadcast,
        mentions: ids,
        mentions_all: all,
        ..Message::default()
    };
    let in_timeline = message.in_timeline();
    let mut evs = vec![Event::MessagePosted { message }];
    // Posting reads the conversation up to your own message.
    if in_timeline {
        evs.push(Event::ReadMarked { user: actor, channel: channel_id, upto: id });
    }
    Ok((evs, json!({ "message": id, "channel": channel_id }), Vec::new()))
}
