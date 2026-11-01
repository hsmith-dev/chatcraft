//! The command registry. Each module contributes its commands; ids are stable API.

mod channel;
mod info;
mod message;
mod nav;
mod profile;

use std::sync::OnceLock;

use chatcraft_model::{Message, State};
use serde_json::{Value, json};

use crate::{CommandSpec, Registry, Session};

/// Every command, in menu order.
pub fn all() -> Vec<CommandSpec> {
    let mut v = Vec::new();
    v.extend(message::commands());
    v.extend(channel::commands());
    v.extend(nav::commands());
    v.extend(profile::commands());
    v.extend(info::commands());
    v
}

pub(crate) fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| Registry::new(all()))
}

/// Requires a signed-in session.
pub(crate) fn signed_in(s: &Session) -> Option<&'static str> {
    if s.me.is_none() { Some("not signed in yet") } else { None }
}

/// A message as agents see it.
pub(crate) fn message_json(s: &State, m: &Message) -> Value {
    let author = s.user(m.author);
    json!({
        "id": m.id,
        "ref": m.id.to_string(),
        "channel": m.channel,
        "author": author.map(|u| u.handle.as_str()),
        "authorName": author.map(|u| u.name()),
        "ts": m.ts.rfc3339(),
        "text": m.text,
        "thread": m.thread,
        "replies": m.reply_count,
        "edited": m.edited.is_some(),
        "deleted": m.deleted,
        "pinned": m.pinned,
        "reactions": m.reactions.iter().map(|(e, who)| json!({
            "emoji": e,
            "count": who.len(),
            "users": who.iter().filter_map(|u| s.user(*u)).map(|u| u.handle.as_str()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}
