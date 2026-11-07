use serde_json::json;

use super::*;
use crate::model::ChannelKind;

fn demo() -> Session {
    let mut s = Session::demo();
    s.focused = true;
    s
}

#[test]
fn demo_signs_in_and_opens_general() {
    let s = demo();
    let me = s.me.unwrap();
    assert_eq!(s.state.user(me).unwrap().handle, "morgan");
    assert_eq!(s.view.channel, s.state.workspace.default_channel);
    let sb = s.sidebar();
    assert!(sb.channels.iter().any(|c| c.title == "#launch-plan"));
    assert!(sb.dms.len() >= 3);
}

#[test]
fn send_reply_react_edit_delete() {
    let mut s = demo();
    let r = s.run("message.send", &json!({"channel": "#random", "text": "hello *world* @ada"})).unwrap();
    let m = r["message"].as_u64().unwrap();
    let msg = s.state.message(chatcraft_core::MessageId(m)).unwrap().clone();
    assert_eq!(msg.mentions.len(), 1);
    s.run("message.react", &json!({"message": m, "emoji": ":tada:"})).unwrap();
    assert!(s.state.message(msg.id).unwrap().reacted("tada", s.me.unwrap()));
    // Toggling again removes it.
    s.run("message.react", &json!({"message": format!("M{m}"), "emoji": "🎉"})).unwrap();
    assert!(s.state.message(msg.id).unwrap().reactions.is_empty());
    s.run("thread.open", &json!({"message": m})).unwrap();
    assert_eq!(s.view.pane, Pane::Thread);
    s.run("message.reply", &json!({"text": "in thread"})).unwrap();
    assert_eq!(s.state.message(msg.id).unwrap().reply_count, 1);
    s.run("message.editLast", &json!({"channel": "#random", "text": "edited"})).unwrap();
    assert_eq!(s.state.message(msg.id).unwrap().text, "edited");
    s.run("message.delete", &json!({"message": m})).unwrap();
    assert!(s.state.message(msg.id).unwrap().deleted);
}

#[test]
fn errors_from_the_hub_come_back_as_errors() {
    let mut s = demo();
    // Ada's message can't be edited by Morgan.
    let ada = s.state.user_by_handle("ada").unwrap().id;
    let theirs = s.state.messages.values().find(|m| m.author == ada).unwrap().id;
    let e = s.run("message.edit", &json!({"message": theirs.0, "text": "nope"})).unwrap_err();
    assert!(e.to_string().contains("author"), "{e}");
    assert!(s.notices.is_empty(), "the caller gets the error; no duplicate notice");
    assert!(matches!(s.run("no.such", &json!({})), Err(CmdError::Unknown(_))));
}

#[test]
fn channels_and_dms() {
    let mut s = demo();
    let r = s.run("channel.create", &json!({"name": "Field Notes", "private": true, "members": ["@ada"]})).unwrap();
    let c = chatcraft_core::ChannelId(r["channel"].as_u64().unwrap());
    assert_eq!(s.view.channel, Some(c));
    assert_eq!(s.state.channel(c).unwrap().kind, ChannelKind::Private);
    s.run("channel.setTopic", &json!({"topic": "Notes from the field"})).unwrap();
    assert_eq!(s.state.channel(c).unwrap().topic, "Notes from the field");
    s.run("channel.leave", &json!({})).unwrap();
    assert!(s.state.channel(c).is_none(), "a private channel you left disappears");
    assert_ne!(s.view.channel, Some(c));
    let r = s.run("dm.open", &json!({"users": ["@kenji", "@noor"]})).unwrap();
    let g = chatcraft_core::ChannelId(r["channel"].as_u64().unwrap());
    assert_eq!(s.state.channel(g).unwrap().kind, ChannelKind::Group);
    assert_eq!(s.run("nav.channel", &json!({"channel": "@ada"})).unwrap()["title"], "Ada Park");
}

#[test]
fn unread_navigation_and_mark_read() {
    let mut s = demo();
    let me = s.me.unwrap();
    let before = s.state.summaries(me).iter().filter(|c| c.unread > 0).count();
    assert!(before > 0);
    s.run("nav.nextUnread", &json!({})).unwrap();
    let opened = s.view.channel.unwrap();
    assert_eq!(s.state.unread(me, opened).0, 0, "opening a conversation reads it");
    assert!(s.can_go_back());
    s.run("nav.back", &json!({})).unwrap();
    s.run("workspace.markAllRead", &json!({})).unwrap();
    assert!(s.state.summaries(me).iter().all(|c| c.unread == 0));
    assert!(s.run("nav.nextUnread", &json!({})).is_err());
}

#[test]
fn search_and_queries() {
    let mut s = demo();
    let r = s.run("search.messages", &json!({"query": "launch in:#launch-plan"})).unwrap();
    assert!(r["count"].as_u64().unwrap() >= 1);
    assert_eq!(s.view.pane, Pane::Search);
    let m = s.run("activity.mentions", &json!({})).unwrap();
    assert!(!m.as_array().unwrap().is_empty());
    let list = s.run("message.list", &json!({"channel": "#engineering", "limit": 2})).unwrap();
    assert_eq!(list["messages"].as_array().unwrap().len(), 2);
    let ch = s.run("channel.browse", &json!({})).unwrap();
    assert!(ch.as_array().unwrap().iter().any(|c| c["name"] == "design"));
}

#[test]
fn status_and_presence() {
    let mut s = demo();
    s.run("profile.setStatus", &json!({"text": "Focus time", "emoji": "headphones", "minutes": 60})).unwrap();
    let me = s.state.user(s.me.unwrap()).unwrap();
    assert_eq!(me.status.text, "Focus time");
    assert!(me.status.until.is_some());
    s.run("profile.setPresence", &json!({"presence": "dnd"})).unwrap();
    assert_eq!(s.state.user(s.me.unwrap()).unwrap().presence, model::Presence::Dnd);
    assert!(s.run("profile.setPresence", &json!({"presence": "sleepy"})).is_err());
}

#[test]
fn hostile_params_never_panic() {
    let junk = [
        json!(null),
        json!({}),
        json!([]),
        json!("x"),
        json!(42),
        json!({"channel": "", "text": "", "message": -1, "emoji": "", "users": [null, 1, "@nobody"], "query": "\"", "limit": 1e300}),
        json!({"channel": 18446744073709551615u64, "message": "M18446744073709551615", "name": "a/b", "presence": 5, "minutes": -3}),
        json!({"channel": "@nobody", "message": "Mx", "text": "x".repeat(50_000), "user": "", "users": "a,b,,c"}),
        json!({"channel": "#general", "message": 1, "text": "ok", "emoji": "+1", "users": ["@ada"], "user": "@ada", "name": "dup", "query": "in: from: has:"}),
    ];
    for spec in Session::registry().all() {
        let mut s = demo();
        for p in &junk {
            let _ = s.run(spec.id, p);
        }
    }
}

#[test]
fn ids_are_unique_and_shortcuts_unambiguous() {
    let all = Session::registry().all();
    let mut ids: Vec<_> = all.iter().map(|s| s.id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(n, ids.len(), "duplicate command ids");
    let mut keys: Vec<&str> = all.iter().flat_map(|s| s.shortcut.split(" / ")).filter(|k| !k.is_empty()).collect();
    keys.sort_unstable();
    // Escape is intentionally shared (close thread, then mark read), resolved by the UI.
    keys.retain(|k| *k != "Escape");
    let n = keys.len();
    keys.dedup();
    assert_eq!(n, keys.len(), "two commands share a shortcut");
}

/// The parity floor only rises: raise it when you implement catalog features.
#[test]
fn parity_floor() {
    let (done, total) = catalog::counts();
    assert!(done >= 65, "parity fell to {done}/{total}");
    assert!(catalog::markdown().contains("| ✅ | Send a message |"));
}
