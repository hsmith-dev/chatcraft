//! Conversations: create, join, leave, invite, settings, direct messages, read state.

use chatcraft_core::UserId;
use chatcraft_model::ChannelKind;
use chatcraft_proto::Op;
use serde_json::{Value, json};

use super::signed_in;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("channel.create", "Create a channel", "Sidebar › Add channels", create)
            .params(r#"{"name": string, "private"?: bool, "topic"?: string, "purpose"?: string, "members"?: ["@handle"]}"#)
            .when(signed_in),
        CommandSpec::new("channel.join", "Join channel", "Channel › Join", join).params(r#"{"channel"?}"#).when(signed_in),
        CommandSpec::new("channel.leave", "Leave channel", "Channel › Settings", leave).params(r#"{"channel"?}"#).when(signed_in),
        CommandSpec::new("channel.invite", "Add people", "Channel › Members", invite).params(r#"{"channel"?, "users": ["@handle"]}"#).when(signed_in),
        CommandSpec::new("channel.remove", "Remove from channel", "Channel › Members", remove)
            .params(r#"{"channel"?, "user": "@handle"}"#)
            .when(signed_in),
        CommandSpec::new("channel.rename", "Rename channel", "Channel › Settings", rename).params(r#"{"channel"?, "name": string}"#).when(signed_in),
        CommandSpec::new("channel.setTopic", "Edit topic", "Channel › About", set_topic).params(r#"{"channel"?, "topic": string}"#).when(signed_in),
        CommandSpec::new("channel.setPurpose", "Edit description", "Channel › About", set_purpose)
            .params(r#"{"channel"?, "purpose": string}"#)
            .when(signed_in),
        CommandSpec::new("channel.archive", "Archive channel", "Channel › Settings", archive).params(r#"{"channel"?}"#).when(signed_in),
        CommandSpec::new("channel.unarchive", "Unarchive channel", "Channel › Settings", unarchive).params(r#"{"channel"}"#).when(signed_in),
        CommandSpec::new("channel.mute", "Mute conversation", "Channel › Notifications", mute)
            .params(r#"{"channel"?, "muted"?: bool}"#)
            .when(signed_in),
        CommandSpec::new("channel.star", "Star conversation", "Channel › Star", star).params(r#"{"channel"?, "starred"?: bool}"#).when(signed_in),
        CommandSpec::new("channel.markRead", "Mark as read", "Channel", mark_read).key("Escape").params(r#"{"channel"?}"#).when(signed_in),
        CommandSpec::new("workspace.markAllRead", "Mark all as read", "Sidebar", mark_all_read).key("Shift+Escape").when(signed_in),
        CommandSpec::new("dm.open", "New message", "Sidebar › Direct messages", dm_open)
            .key("Mod+N")
            .params(r#"{"users": ["@handle"]}"#)
            .when(signed_in),
        CommandSpec::new("channel.directory", "Room directory", "Sidebar › Add channels", |s, v| {
            s.submit(Op::Directory { query: p::str(v, "query").unwrap_or_default().to_string() })
        })
        .params(r#"{"query"?: string}"#)
        .when(signed_in),
        CommandSpec::new("channel.browse", "Browse channels", "Sidebar › Add channels", browse).params(r#"{"query"?: string}"#).pure(),
        CommandSpec::new("channel.members", "Members", "Channel › Members", members).params(r#"{"channel"?}"#).pure(),
        CommandSpec::new("channel.info", "Channel details", "Channel › About", info).params(r#"{"channel"?}"#).pure(),
        CommandSpec::new("channel.pins", "Pinned messages", "Channel › Pins", pins).params(r#"{"channel"?}"#).pure(),
    ]
}

/// Known people, plus Matrix IDs (`@bob:example.org`) this replica hasn't met yet.
fn split_users(s: &Session, v: &Value, key: &str) -> Result<(Vec<UserId>, Vec<String>), CmdError> {
    let mut known = Vec::new();
    let mut external = Vec::new();
    for u in p::list(v, key) {
        match s.resolve_user(&u) {
            Ok(id) => known.push(id),
            Err(_) if is_mxid(&u) => external.push(u),
            Err(e) => return Err(e),
        }
    }
    Ok((known, external))
}

/// `@localpart:server`.
fn is_mxid(s: &str) -> bool {
    s.strip_prefix('@').and_then(|r| r.split_once(':')).is_some_and(|(l, d)| !l.is_empty() && !d.is_empty()) && !s.contains(char::is_whitespace)
}

/// Both lists of `users` and `user`.
fn people(s: &Session, v: &Value) -> Result<(Vec<UserId>, Vec<String>), CmdError> {
    let (mut list, mut external) = split_users(s, v, "users")?;
    let (more, more_ext) = split_users(s, v, "user")?;
    list.extend(more);
    external.extend(more_ext);
    if list.is_empty() && external.is_empty() {
        return Err(CmdError::Params("`users` is required".into()));
    }
    Ok((list, external))
}

fn ok(r: CmdResult) -> CmdResult {
    r.map(|v| if v.is_null() { json!({ "ok": true }) } else { v })
}

fn create(s: &mut Session, v: &Value) -> CmdResult {
    let name = p::req_str(v, "name")?.to_string();
    let (members, external) = split_users(s, v, "members")?;
    let op = Op::CreateChannel {
        name,
        private: p::bool(v, "private").unwrap_or(false),
        topic: p::str(v, "topic").unwrap_or_default().into(),
        purpose: p::str(v, "purpose").unwrap_or_default().into(),
        members,
        external,
    };
    s.submit_and_open(op)
}

fn join(s: &mut Session, v: &Value) -> CmdResult {
    // A Matrix room this replica hasn't seen: a room id (`!abc`, or `!abc:server` in older room
    // versions) or an alias (`#name:server`).
    if let Some(room) = p::str(v, "channel").filter(|r| (r.starts_with('!') && r.len() > 1) || (r.starts_with('#') && r.contains(':')))
        && s.state.resolve_channel(room, s.me).is_none()
    {
        return s.submit_and_open(Op::JoinExternal { room: room.to_string() });
    }
    let c = s.target_channel(v)?;
    s.submit_and_open(Op::JoinChannel { channel: c })
}

fn leave(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    ok(s.submit(Op::LeaveChannel { channel: c }))
}

fn invite(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let (list, external) = people(s, v)?;
    ok(s.submit(Op::Invite { channel: c, users: list, external }))
}

fn remove(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let user = s.resolve_user(p::req_str(v, "user")?)?;
    ok(s.submit(Op::Remove { channel: c, user }))
}

fn rename(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    ok(s.submit(Op::RenameChannel { channel: c, name: p::req_str(v, "name")?.into() }))
}

fn set_topic(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    ok(s.submit(Op::SetTopic { channel: c, topic: p::str(v, "topic").unwrap_or_default().into() }))
}

fn set_purpose(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    ok(s.submit(Op::SetPurpose { channel: c, purpose: p::str(v, "purpose").unwrap_or_default().into() }))
}

fn archive(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    ok(s.submit(Op::Archive { channel: c, archived: true }))
}

fn unarchive(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    ok(s.submit(Op::Archive { channel: c, archived: false }))
}

fn mute(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let me = s.me.unwrap_or_default();
    let muted = p::bool(v, "muted").unwrap_or_else(|| !s.state.muted.get(&me).is_some_and(|m| m.contains(&c)));
    ok(s.submit(Op::Mute { channel: c, muted }))
}

fn star(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let me = s.me.unwrap_or_default();
    let starred = p::bool(v, "starred").unwrap_or_else(|| !s.state.starred.get(&me).is_some_and(|m| m.contains(&c)));
    ok(s.submit(Op::Star { channel: c, starred }))
}

fn mark_read(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let Some(upto) = s.state.latest(c) else { return Ok(json!({ "ok": true })) };
    ok(s.submit(Op::MarkRead { channel: c, upto }))
}

fn mark_all_read(s: &mut Session, _: &Value) -> CmdResult {
    let me = s.me.unwrap_or_default();
    let unread: Vec<_> = s.state.summaries(me).into_iter().filter(|c| c.unread > 0).map(|c| c.id).collect();
    let n = unread.len();
    for c in unread {
        if let Some(upto) = s.state.latest(c) {
            s.submit(Op::MarkRead { channel: c, upto })?;
        }
    }
    Ok(json!({ "marked": n }))
}

fn dm_open(s: &mut Session, v: &Value) -> CmdResult {
    let (list, external) = people(s, v)?;
    s.submit_and_open(Op::OpenDm { users: list, external })
}

fn browse(s: &mut Session, v: &Value) -> CmdResult {
    let me = s.me.unwrap_or_default();
    let q = p::str(v, "query").unwrap_or_default().trim().trim_start_matches('#').to_lowercase();
    let list: Vec<Value> = s
        .state
        .channels
        .values()
        .filter(|c| !c.kind.is_dm() && s.state.can_see(me, c.id) && (q.is_empty() || c.name.contains(&q) || c.purpose.to_lowercase().contains(&q)))
        .map(|c| {
            json!({
                "id": c.id, "name": c.name, "private": c.kind == ChannelKind::Private, "archived": c.archived,
                "members": c.members.len(), "joined": c.members.contains(&me), "topic": c.topic, "purpose": c.purpose,
            })
        })
        .collect();
    Ok(Value::Array(list))
}

fn members(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let ch = s.state.channel(c).ok_or_else(|| CmdError::Params(format!("no conversation {c}")))?;
    let list: Vec<Value> = ch
        .members
        .iter()
        .filter_map(|u| s.state.user(*u))
        .map(|u| json!({"id": u.id, "handle": u.handle, "name": u.name(), "presence": u.presence, "title": u.title}))
        .collect();
    Ok(Value::Array(list))
}

fn info(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let ch = s.state.channel(c).ok_or_else(|| CmdError::Params(format!("no conversation {c}")))?;
    let me = s.me.unwrap_or_default();
    let (unread, mentions) = s.state.unread(me, c);
    Ok(json!({
        "id": ch.id, "title": s.state.title(c, s.me), "name": ch.name, "kind": ch.kind, "topic": ch.topic, "purpose": ch.purpose,
        "archived": ch.archived, "members": ch.members.len(), "created": ch.created.rfc3339(),
        "createdBy": s.state.user(ch.created_by).map(|u| u.handle.as_str()), "unread": unread, "mentions": mentions,
    }))
}

fn pins(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let list: Vec<Value> = s.state.timeline(c).iter().filter(|m| m.pinned).map(|m| super::message_json(&s.state, m)).collect();
    Ok(Value::Array(list))
}
