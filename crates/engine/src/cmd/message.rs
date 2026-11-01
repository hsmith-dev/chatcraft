//! Messages, threads and reactions.

use chatcraft_proto::Op;
use serde_json::{Value, json};

use super::{message_json, signed_in};
use crate::{CmdError, CmdResult, CommandSpec, Pane, Session, p};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("message.send", "Send message", "Composer", send)
            .key("Enter")
            .params(r##"{"text": string, "channel"?: "#name" | "@handle" | id}"##)
            .when(signed_in),
        CommandSpec::new("message.reply", "Reply in thread", "Message › Reply in thread", reply)
            .params(r#"{"text": string, "message"?: id (default: open thread), "broadcast"?: bool}"#)
            .when(signed_in),
        CommandSpec::new("message.edit", "Edit message", "Message › More actions", edit).params(r#"{"message": id, "text": string}"#).when(signed_in),
        CommandSpec::new("message.editLast", "Edit last message", "Composer", edit_last)
            .key("Up")
            .params(r#"{"text": string, "channel"?}"#)
            .when(signed_in),
        CommandSpec::new("message.delete", "Delete message", "Message › More actions", delete).params(r#"{"message": id}"#).when(signed_in),
        CommandSpec::new("message.react", "Add reaction", "Message › Add reaction", react)
            .params(r#"{"message": id, "emoji": "shortcode" | "glyph", "on"?: bool (default: toggle)}"#)
            .when(signed_in),
        CommandSpec::new("message.pin", "Pin to channel", "Message › More actions", pin)
            .params(r#"{"message": id, "pinned"?: bool}"#)
            .when(signed_in),
        CommandSpec::new("message.save", "Save for later", "Message › Save for later", save)
            .params(r#"{"message": id, "saved"?: bool}"#)
            .when(signed_in),
        CommandSpec::new("message.copyLink", "Copy link", "Message › More actions", copy_link).params(r#"{"message": id}"#).pure(),
        CommandSpec::new("message.get", "Get message", "Agents", get).params(r#"{"message": id}"#).pure(),
        CommandSpec::new("message.list", "List messages", "Agents", list).params(r#"{"channel"?, "limit"?: number (default 50)}"#).pure(),
        CommandSpec::new("thread.open", "Open thread", "Message › Reply in thread", thread_open).params(r#"{"message": id}"#).pure(),
        CommandSpec::new("thread.close", "Close thread", "Thread", thread_close).key("Escape").pure(),
        CommandSpec::new("thread.list", "List replies", "Agents", thread_list).params(r#"{"message": id}"#).pure(),
        CommandSpec::new("composer.setDraft", "Set draft", "Composer", set_draft).params(r#"{"text": string, "channel"?}"#).pure(),
        CommandSpec::new("composer.typing", "Typing", "Composer", |s, v| {
            let channel = s.target_channel(v)?;
            s.submit(Op::Typing { channel, typing: p::bool(v, "typing").unwrap_or(true) })
        })
        .params(r#"{"channel"?, "typing"?: bool (default true)}"#)
        .pure()
        .when(signed_in),
        CommandSpec::new("composer.send", "Send draft", "Composer", send_draft).params(r#"{"channel"?}"#).when(signed_in),
    ]
}

fn ack_ok(r: CmdResult) -> CmdResult {
    r.map(|v| if v.is_null() { json!({ "ok": true }) } else { v })
}

fn send(s: &mut Session, v: &Value) -> CmdResult {
    let text = p::req_str(v, "text")?.to_string();
    let channel = s.target_channel(v)?;
    if s.previewing() && s.view.channel == Some(channel) {
        return Err(CmdError::Disabled("join the channel to send messages".into()));
    }
    let r = s.submit(Op::PostMessage { channel, text, thread: None, broadcast: false })?;
    s.drafts.remove(&channel);
    Ok(r)
}

fn reply(s: &mut Session, v: &Value) -> CmdResult {
    let text = p::req_str(v, "text")?.to_string();
    let root = p::message(v, "message").or(s.view.thread).ok_or_else(|| CmdError::Params("`message` (thread root) is required".into()))?;
    let channel = s.state.message(root).map(|m| m.channel).ok_or_else(|| CmdError::Params(format!("no message {root}")))?;
    let broadcast = p::bool(v, "broadcast").unwrap_or(false);
    let r = s.submit(Op::PostMessage { channel, text, thread: Some(root), broadcast })?;
    s.thread_drafts.remove(&root);
    Ok(r)
}

fn edit(s: &mut Session, v: &Value) -> CmdResult {
    let message = p::req_message(v, "message")?;
    let text = p::req_str(v, "text")?.to_string();
    ack_ok(s.submit(Op::EditMessage { message, text }))
}

fn edit_last(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let message = s.last_own_message(c).ok_or_else(|| CmdError::Disabled("you haven't sent a message here".into()))?;
    match p::str(v, "text") {
        Some(text) => ack_ok(s.submit(Op::EditMessage { message, text: text.to_string() })),
        // Without text: report which message would be edited (the UI opens it for editing).
        None => Ok(json!({ "message": message })),
    }
}

fn delete(s: &mut Session, v: &Value) -> CmdResult {
    let message = p::req_message(v, "message")?;
    let r = ack_ok(s.submit(Op::DeleteMessage { message }))?;
    if s.view.thread == Some(message) {
        s.view.thread = None;
    }
    Ok(r)
}

fn react(s: &mut Session, v: &Value) -> CmdResult {
    let message = p::req_message(v, "message")?;
    let raw = p::req_str(v, "emoji")?.trim().trim_matches(':');
    let emoji = chatcraft_text::emoji::shortcode_for(raw).unwrap_or(raw).to_string();
    let me = s.me.unwrap_or_default();
    let on = p::bool(v, "on").unwrap_or_else(|| !s.state.message(message).is_some_and(|m| m.reacted(&emoji, me)));
    ack_ok(s.submit(Op::React { message, emoji, on }))
}

fn pin(s: &mut Session, v: &Value) -> CmdResult {
    let message = p::req_message(v, "message")?;
    let pinned = p::bool(v, "pinned").unwrap_or_else(|| !s.state.message(message).is_some_and(|m| m.pinned));
    ack_ok(s.submit(Op::Pin { message, pinned }))
}

fn save(s: &mut Session, v: &Value) -> CmdResult {
    let message = p::req_message(v, "message")?;
    let me = s.me.unwrap_or_default();
    let saved = p::bool(v, "saved").unwrap_or_else(|| !s.state.saved.get(&me).is_some_and(|set| set.contains(&message)));
    ack_ok(s.submit(Op::Save { message, saved }))
}

fn copy_link(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_message(v, "message")?;
    let m = s.state.message(id).ok_or_else(|| CmdError::Params(format!("no message {id}")))?;
    Ok(json!({ "link": format!("chatcraft://{}/{}", m.channel, m.id) }))
}

fn get(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_message(v, "message")?;
    let m = s.state.message(id).ok_or_else(|| CmdError::Params(format!("no message {id}")))?;
    Ok(message_json(&s.state, m))
}

fn list(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let limit = usize::try_from(p::u64(v, "limit").unwrap_or(50).clamp(1, 1000)).unwrap_or(50);
    let tl = s.state.timeline(c);
    let start = tl.len().saturating_sub(limit);
    let msgs: Vec<Value> = tl.get(start..).unwrap_or_default().iter().map(|m| message_json(&s.state, m)).collect();
    Ok(json!({ "channel": c, "title": s.state.title(c, s.me), "messages": msgs }))
}

fn thread_open(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_message(v, "message")?;
    let m = s.state.message(id).ok_or_else(|| CmdError::Params(format!("no message {id}")))?;
    let root = m.thread.unwrap_or(m.id);
    let channel = m.channel;
    if s.view.channel != Some(channel) {
        s.open_channel(channel);
    }
    s.view.thread = Some(root);
    s.view.pane = Pane::Thread;
    Ok(json!({ "thread": root }))
}

fn thread_close(s: &mut Session, _: &Value) -> CmdResult {
    s.view.thread = None;
    if s.view.pane == Pane::Thread {
        s.view.pane = Pane::None;
    }
    Ok(Value::Null)
}

fn thread_list(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_message(v, "message")?;
    let root = s.state.message(id).ok_or_else(|| CmdError::Params(format!("no message {id}")))?;
    let replies: Vec<Value> = s.state.replies(root.thread.unwrap_or(root.id)).iter().map(|m| message_json(&s.state, m)).collect();
    Ok(json!({ "root": message_json(&s.state, root), "replies": replies }))
}

fn set_draft(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let text = p::str(v, "text").unwrap_or_default();
    if text.is_empty() {
        s.drafts.remove(&c);
    } else {
        s.drafts.insert(c, text.to_string());
    }
    Ok(Value::Null)
}

fn send_draft(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    let text = s.drafts.get(&c).cloned().unwrap_or_default();
    if text.trim().is_empty() {
        return Err(CmdError::Disabled("the draft is empty".into()));
    }
    send(s, &json!({ "channel": c.to_string(), "text": text }))
}
