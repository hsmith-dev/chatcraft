//! Navigation and views: open conversations, unread jumps, history, panes, search.

use chatcraft_search::Query;
use serde_json::{Value, json};

use crate::{CmdError, CmdResult, CommandSpec, Pane, Session, p};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("nav.channel", "Go to conversation", "Sidebar", go).params(r##"{"channel": "#name" | "@handle" | id, "message"?: id}"##).pure(),
        CommandSpec::new("nav.back", "Back", "Title bar", back).key("Mod+[").pure().when(|s| if s.can_go_back() { None } else { Some("nothing to go back to") }),
        CommandSpec::new("nav.forward", "Forward", "Title bar", forward)
            .key("Mod+]")
            .pure()
            .when(|s| if s.can_go_forward() { None } else { Some("nothing to go forward to") }),
        CommandSpec::new("nav.nextUnread", "Next unread conversation", "Sidebar", |s, _| step(s, 1, true)).key("Alt+Shift+Down").pure(),
        CommandSpec::new("nav.prevUnread", "Previous unread conversation", "Sidebar", |s, _| step(s, -1, true)).key("Alt+Shift+Up").pure(),
        CommandSpec::new("nav.next", "Next conversation", "Sidebar", |s, _| step(s, 1, false)).key("Alt+Down").pure(),
        CommandSpec::new("nav.prev", "Previous conversation", "Sidebar", |s, _| step(s, -1, false)).key("Alt+Up").pure(),
        CommandSpec::new("nav.activity", "Activity", "Rail › Activity", |s, _| pane(s, Pane::Activity)).key("Mod+Shift+M").pure(),
        CommandSpec::new("nav.saved", "Saved items", "Rail › Later", |s, _| pane(s, Pane::Saved)).key("Mod+Shift+S").pure(),
        CommandSpec::new("nav.details", "Conversation details", "Channel header", |s, _| pane(s, Pane::Details)).key("Mod+Shift+I").pure(),
        CommandSpec::new("nav.profile", "View profile", "Message › Author", profile).params(r#"{"user": "@handle"}"#).pure(),
        CommandSpec::new("nav.closePane", "Close side pane", "Side pane", |s, _| {
            s.view.pane = Pane::None;
            s.view.thread = None;
            Ok(Value::Null)
        })
        .pure(),
        CommandSpec::new("search.messages", "Search messages", "Title bar › Search", search)
            .key("Mod+G")
            .params(r##"{"query": "words \"phrase\" -not in:#chan from:@who to:me has:link|reaction|pin is:thread|saved before:|after:|on:YYYY-MM-DD", "limit"?}"##)
            .pure(),
        CommandSpec::new("search.clear", "Clear search", "Search", |s, _| {
            s.search_query.clear();
            s.search_hits.clear();
            if s.view.pane == Pane::Search {
                s.view.pane = Pane::None;
            }
            Ok(Value::Null)
        })
        .pure(),
    ]
}

fn go(s: &mut Session, v: &Value) -> CmdResult {
    let c = s.target_channel(v)?;
    s.open_channel(c);
    if let Some(m) = p::message(v, "message").and_then(|m| s.state.message(m)).filter(|m| m.channel == c) {
        let (id, root) = (m.id, m.thread);
        s.view.focus = Some(root.unwrap_or(id));
        if let Some(r) = root {
            s.view.thread = Some(r);
            s.view.pane = Pane::Thread;
        }
    }
    Ok(json!({ "channel": c, "title": s.state.title(c, s.me) }))
}

fn back(s: &mut Session, _: &Value) -> CmdResult {
    if s.go_back() { Ok(json!({ "channel": s.view.channel })) } else { Err(CmdError::Disabled("nothing to go back to".into())) }
}

fn forward(s: &mut Session, _: &Value) -> CmdResult {
    if s.go_forward() { Ok(json!({ "channel": s.view.channel })) } else { Err(CmdError::Disabled("nothing to go forward to".into())) }
}

/// Moves through the sidebar order, optionally only to conversations with unread messages.
fn step(s: &mut Session, dir: i64, unread_only: bool) -> CmdResult {
    let order: Vec<_> = s.sidebar().order().map(|c| (c.id, c.unread > 0 && !c.muted)).collect();
    if order.is_empty() {
        return Err(CmdError::Disabled("no conversations".into()));
    }
    let n = i64::try_from(order.len()).unwrap_or(i64::MAX);
    let cur = s.view.channel.and_then(|c| order.iter().position(|(id, _)| *id == c)).and_then(|i| i64::try_from(i).ok()).unwrap_or(-1);
    for k in 1..=n {
        let i = usize::try_from((cur + dir * k).rem_euclid(n)).unwrap_or(0);
        if let Some((id, unread)) = order.get(i).copied()
            && (!unread_only || unread)
        {
            s.open_channel(id);
            return Ok(json!({ "channel": id, "title": s.state.title(id, s.me) }));
        }
    }
    Err(CmdError::Disabled("no unread conversations".into()))
}

fn pane(s: &mut Session, p: Pane) -> CmdResult {
    s.view.pane = if s.view.pane == p { Pane::None } else { p };
    Ok(json!({ "pane": s.view.pane }))
}

fn profile(s: &mut Session, v: &Value) -> CmdResult {
    let u = s.resolve_user(p::req_str(v, "user")?)?;
    s.view.profile = Some(u);
    s.view.pane = Pane::Profile;
    Ok(json!({ "user": u }))
}

fn search(s: &mut Session, v: &Value) -> CmdResult {
    let q = p::str(v, "query").unwrap_or_default().to_string();
    let limit = usize::try_from(p::u64(v, "limit").unwrap_or(50)).unwrap_or(50);
    let me = s.me.ok_or_else(|| CmdError::Disabled("not signed in".into()))?;
    let query = Query::parse(&q);
    let hits = chatcraft_search::search(&s.state, me, &query, limit);
    let out: Vec<Value> = hits
        .iter()
        .filter_map(|h| s.state.message(h.message))
        .map(|m| {
            let mut j = super::message_json(&s.state, m);
            if let Some(o) = j.as_object_mut() {
                o.insert("channelTitle".into(), json!(s.state.title(m.channel, s.me)));
            }
            j
        })
        .collect();
    s.search_query = q;
    s.search_hits = hits;
    s.view.pane = Pane::Search;
    Ok(json!({ "count": out.len(), "results": out, "unknownModifiers": query.unknown }))
}
