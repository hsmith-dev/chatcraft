//! Read-only queries for agents and scripts.

use serde_json::{Value, json};

use crate::{CmdError, CmdResult, CommandSpec, Session, p};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("session.state", "Session state", "Agents", |s, _| Ok(s.describe())).pure(),
        CommandSpec::new("workspace.info", "Workspace details", "Workspace menu", workspace).pure(),
        CommandSpec::new("conversations.list", "List my conversations", "Sidebar", conversations).pure(),
        CommandSpec::new("people.list", "People directory", "Rail › People", people).params(r#"{"query"?: string}"#).pure(),
        CommandSpec::new("people.get", "Person details", "Profile", person).params(r#"{"user": "@handle"}"#).pure(),
        CommandSpec::new("activity.mentions", "Mentions and reactions", "Rail › Activity", mentions).params(r#"{"limit"?}"#).pure(),
        CommandSpec::new("saved.list", "Saved items", "Rail › Later", saved).pure(),
        CommandSpec::new("commands.list", "All commands", "Help › Keyboard shortcuts", |_, _| Ok(Session::registry().describe())).pure(),
        CommandSpec::new("parity.summary", "Slack / Teams parity", "Help › About", |_, _| Ok(crate::catalog::summary_json())).pure(),
    ]
}

fn workspace(s: &mut Session, _: &Value) -> CmdResult {
    let w = &s.state.workspace;
    Ok(json!({
        "name": w.name, "created": w.created.rfc3339(), "people": s.state.users.values().filter(|u| !u.deactivated && !u.bot).count(),
        "channels": s.state.channels.values().filter(|c| !c.kind.is_dm()).count(),
        "defaultChannel": w.default_channel.map(|c| s.state.title(c, None)),
    }))
}

fn conversations(s: &mut Session, _: &Value) -> CmdResult {
    serde_json::to_value(s.sidebar()).map_err(|e| CmdError::Failed(e.to_string()))
}

fn user_json(u: &chatcraft_model::User) -> Value {
    json!({
        "id": u.id, "handle": u.handle, "name": u.name(), "title": u.title, "presence": u.presence, "role": u.role, "bot": u.bot,
        "status": if u.status.text.is_empty() && u.status.emoji.is_empty() { Value::Null } else { json!({"text": u.status.text, "emoji": u.status.emoji}) },
    })
}

fn people(s: &mut Session, v: &Value) -> CmdResult {
    let q = p::str(v, "query").unwrap_or_default().trim().trim_start_matches('@').to_lowercase();
    let list: Vec<Value> = s
        .state
        .users
        .values()
        .filter(|u| !u.deactivated && (q.is_empty() || u.handle.contains(&q) || u.name().to_lowercase().contains(&q)))
        .map(user_json)
        .collect();
    Ok(Value::Array(list))
}

fn person(s: &mut Session, v: &Value) -> CmdResult {
    let id = s.resolve_user(p::req_str(v, "user")?)?;
    s.state.user(id).map(user_json).ok_or_else(|| CmdError::Params(format!("no person {id}")))
}

fn mentions(s: &mut Session, v: &Value) -> CmdResult {
    let me = s.me.ok_or_else(|| CmdError::Disabled("not signed in".into()))?;
    let limit = usize::try_from(p::u64(v, "limit").unwrap_or(50).min(500)).unwrap_or(50);
    let list: Vec<Value> = s.state.mentions_of(me, limit).into_iter().map(|m| super::message_json(&s.state, m)).collect();
    Ok(Value::Array(list))
}

fn saved(s: &mut Session, _: &Value) -> CmdResult {
    let me = s.me.ok_or_else(|| CmdError::Disabled("not signed in".into()))?;
    let ids: Vec<_> = s.state.saved.get(&me).map(|set| set.iter().rev().copied().collect()).unwrap_or_default();
    let list: Vec<Value> =
        ids.iter().filter_map(|id| s.state.message(*id)).filter(|m| !m.deleted).map(|m| super::message_json(&s.state, m)).collect();
    Ok(Value::Array(list))
}
