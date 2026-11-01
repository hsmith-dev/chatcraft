//! You: status, presence, profile.

use chatcraft_model::Presence;
use chatcraft_proto::Op;
use serde_json::{Value, json};

use super::signed_in;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("profile.setStatus", "Set a status", "Profile menu › Status", set_status)
            .key("Mod+Shift+Y")
            .params(r#"{"text": string, "emoji"?: "shortcode", "minutes"?: number}"#)
            .when(signed_in),
        CommandSpec::new("profile.clearStatus", "Clear status", "Profile menu › Status", |s, _| {
            ok(s.submit(Op::SetStatus { text: String::new(), emoji: String::new(), minutes: None }))
        })
        .when(signed_in),
        CommandSpec::new("profile.setPresence", "Set availability", "Profile menu", set_presence)
            .params(r#"{"presence": "active" | "away" | "dnd" | "offline"}"#)
            .when(signed_in),
        CommandSpec::new("profile.toggleAway", "Toggle away", "Profile menu", toggle_away).when(signed_in),
        CommandSpec::new("profile.setName", "Edit display name", "Profile menu › Edit profile", |s, v| {
            ok(s.submit(Op::SetProfile { display_name: Some(p::req_str(v, "name")?.into()), title: None }))
        })
        .params(r#"{"name": string}"#)
        .when(signed_in),
        CommandSpec::new("profile.setTitle", "Edit title", "Profile menu › Edit profile", |s, v| {
            ok(s.submit(Op::SetProfile { display_name: None, title: Some(p::str(v, "title").unwrap_or_default().into()) }))
        })
        .params(r#"{"title": string}"#)
        .when(signed_in),
        CommandSpec::new("workspace.rename", "Rename workspace", "Workspace menu › Settings", |s, v| {
            ok(s.submit(Op::RenameWorkspace { name: p::req_str(v, "name")?.into() }))
        })
        .params(r#"{"name": string}"#)
        .when(signed_in),
    ]
}

fn ok(r: CmdResult) -> CmdResult {
    r.map(|v| if v.is_null() { json!({ "ok": true }) } else { v })
}

fn set_status(s: &mut Session, v: &Value) -> CmdResult {
    let minutes = p::u64(v, "minutes").map(|m| u32::try_from(m).unwrap_or(u32::MAX));
    ok(s.submit(Op::SetStatus { text: p::req_str(v, "text")?.into(), emoji: p::str(v, "emoji").unwrap_or_default().into(), minutes }))
}

fn set_presence(s: &mut Session, v: &Value) -> CmdResult {
    let raw = p::req_str(v, "presence")?;
    let presence = Presence::parse(raw).ok_or_else(|| CmdError::Params(format!("unknown presence `{raw}`")))?;
    ok(s.submit(Op::SetPresence { presence }))
}

fn toggle_away(s: &mut Session, _: &Value) -> CmdResult {
    let cur = s.me.and_then(|m| s.state.user(m)).map(|u| u.presence).unwrap_or_default();
    let presence = if cur == Presence::Away { Presence::Active } else { Presence::Away };
    ok(s.submit(Op::SetPresence { presence }))
}
