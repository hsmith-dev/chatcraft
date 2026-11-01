use serde_json::json;

use super::*;

const ME: &str = "@alice:test";

fn room(m: &mut Mapper, id: &str, name: &str, members: &[&str], encrypted: bool) -> ChannelId {
    m.upsert_room(&RoomInfo {
        room_id: id.into(),
        name: name.into(),
        encrypted,
        members: members.iter().map(|u| (u.to_string(), None)).collect(),
        ..RoomInfo::default()
    })
}

fn msg(id: &str, sender: &str, body: &str) -> serde_json::Value {
    json!({"type": "m.room.message", "event_id": id, "sender": sender, "origin_server_ts": 1000, "content": {"msgtype": "m.text", "body": body}})
}

#[test]
fn rooms_messages_threads_and_dedupe() {
    let mut m = Mapper::new(ME);
    let c = room(&mut m, "!r:test", "Launch Plan", &[ME, "@bob:test"], true);
    assert_eq!(m.state().workspace.name, "test", "the homeserver names the workspace");
    assert_eq!(m.state().channel(c).unwrap().name, "launch-plan");
    assert!(m.state().channel(c).unwrap().encrypted);
    assert_eq!(m.state().channel(c).unwrap().kind, ChannelKind::Private);
    m.timeline("!r:test", &msg("$1", "@bob:test", "hi @alice"), true, false);
    m.timeline("!r:test", &msg("$1", "@bob:test", "hi @alice"), true, false);
    let mut reply = msg("$2", ME, "in thread");
    reply["content"]["m.relates_to"] = json!({"rel_type": "m.thread", "event_id": "$1"});
    m.timeline("!r:test", &reply, true, false);
    let root = m.state().timeline(c);
    assert_eq!(root.len(), 1, "the thread reply stays out of the timeline; the duplicate is ignored");
    assert_eq!(root[0].reply_count, 1);
    assert!(root[0].encrypted);
    assert!(root[0].mentions.contains(&m.me().unwrap()));
}

#[test]
fn only_the_author_may_edit() {
    let mut m = Mapper::new(ME);
    room(&mut m, "!r:test", "general", &[ME, "@bob:test", "@eve:test"], false);
    m.timeline("!r:test", &msg("$1", "@bob:test", "original"), false, false);
    let edit = |id: &str, who: &str, text: &str| {
        json!({"type": "m.room.message", "event_id": id, "sender": who, "origin_server_ts": 2000,
               "content": {"body": format!("* {text}"), "m.new_content": {"body": text}, "m.relates_to": {"rel_type": "m.replace", "event_id": "$1"}}})
    };
    m.timeline("!r:test", &edit("$e1", "@eve:test", "forged"), false, false);
    assert_eq!(m.state().messages.values().next().unwrap().text, "original");
    m.timeline("!r:test", &edit("$e2", "@bob:test", "fixed"), false, false);
    let msg = m.state().messages.values().next().unwrap();
    assert_eq!(msg.text, "fixed");
    assert!(msg.edited.is_some());
}

#[test]
fn reactions_and_redactions() {
    let mut m = Mapper::new(ME);
    let c = room(&mut m, "!r:test", "general", &[ME, "@bob:test"], false);
    m.timeline("!r:test", &msg("$1", "@bob:test", "ship it"), false, false);
    let react = json!({"type": "m.reaction", "event_id": "$r1", "sender": ME, "origin_server_ts": 1,
                       "content": {"m.relates_to": {"rel_type": "m.annotation", "event_id": "$1", "key": "🎉"}}});
    m.timeline("!r:test", &react, false, false);
    let mid = m.state().timeline(c)[0].id;
    assert!(m.state().message(mid).unwrap().reacted("tada", m.me().unwrap()));
    assert_eq!(m.reaction_event(mid, "tada", m.me().unwrap()), Some("$r1"));
    m.timeline("!r:test", &json!({"type": "m.room.redaction", "event_id": "$x", "sender": ME, "redacts": "$r1", "content": {}}), false, false);
    assert!(m.state().message(mid).unwrap().reactions.is_empty());
    m.timeline("!r:test", &json!({"type": "m.room.redaction", "event_id": "$y", "sender": "@bob:test", "content": {"redacts": "$1"}}), false, false);
    assert!(m.state().message(mid).unwrap().deleted);
}

#[test]
fn undecryptable_messages_are_kept_and_flagged() {
    let mut m = Mapper::new(ME);
    let c = room(&mut m, "!r:test", "secret", &[ME, "@bob:test"], true);
    let enc = json!({"type": "m.room.encrypted", "event_id": "$u", "sender": "@bob:test", "origin_server_ts": 5, "content": {"algorithm": "m.megolm.v1.aes-sha2", "ciphertext": "…"}});
    m.timeline("!r:test", &enc, true, true);
    let tl = m.state().timeline(c);
    assert!(tl[0].undecryptable && tl[0].text.is_empty());
}

#[test]
fn a_known_dm_stays_direct_when_m_direct_lags() {
    let mut m = Mapper::new(ME);
    let info =
        RoomInfo { room_id: "!d:test".into(), direct: true, members: vec![(ME.into(), None), ("@bob:test".into(), None)], ..RoomInfo::default() };
    let c = m.upsert_room(&info);
    // A sync that doesn't list us yet (just joined) and lags on m.direct changes nothing.
    m.upsert_room(&RoomInfo { direct: false, name: "bob".into(), members: vec![("@bob:test".into(), None)], ..info });
    assert_eq!(m.state().channel(c).unwrap().kind, ChannelKind::Direct);
    assert_eq!(m.state().resolve_channel("@bob", m.me()), Some(c));
}

#[test]
fn undecryptable_messages_are_replaced_when_their_key_arrives() {
    let mut m = Mapper::new(ME);
    let c = room(&mut m, "!r:test", "secret", &[ME, "@bob:test"], true);
    let enc = json!({"type": "m.room.encrypted", "event_id": "$u", "sender": "@bob:test", "origin_server_ts": 5, "content": {}});
    m.timeline("!r:test", &enc, true, true);
    let id = m.state().timeline(c)[0].id;
    m.timeline("!r:test", &json!({"type": "m.reaction", "event_id": "$r", "sender": ME, "content": {"m.relates_to": {"rel_type": "m.annotation", "event_id": "$u", "key": "👀"}}}), true, false);
    // Still no key: nothing changes.
    m.timeline("!r:test", &enc, true, true);
    assert!(m.state().message(id).unwrap().undecryptable);
    // The key arrived: same id, now readable, reaction kept.
    m.timeline("!r:test", &msg("$u", "@bob:test", "now you can read me"), true, false);
    let msg = m.state().message(id).unwrap();
    assert_eq!((msg.text.as_str(), msg.undecryptable, msg.encrypted), ("now you can read me", false, true));
    assert_eq!(msg.reactions.len(), 1);
    assert_eq!(m.state().timeline(c).len(), 1);
    // A placeholder that decrypts to a reaction disappears from the timeline.
    m.timeline("!r:test", &json!({"type": "m.room.encrypted", "event_id": "$u2", "sender": "@bob:test", "content": {}}), true, true);
    assert_eq!(m.state().timeline(c).len(), 2);
    m.timeline("!r:test", &json!({"type": "m.reaction", "event_id": "$u2", "sender": "@bob:test", "content": {"m.relates_to": {"rel_type": "m.annotation", "event_id": "$u", "key": "🎉"}}}), true, false);
    assert_eq!(m.state().timeline(c).len(), 1);
    assert_eq!(m.state().message(id).unwrap().reactions.len(), 2);
}

#[test]
fn formatted_bodies_keep_their_formatting() {
    let mut m = Mapper::new(ME);
    let c = room(&mut m, "!r:test", "general", &[ME, "@bob:test"], false);
    let ev = json!({"type": "m.room.message", "event_id": "$h", "sender": "@bob:test", "origin_server_ts": 1,
        "content": {"msgtype": "m.text", "body": "Ship it alice", "format": "org.matrix.custom.html",
                    "formatted_body": "<strong>Ship</strong> it <a href=\"https://matrix.to/#/@alice:test\">Alice</a>"}});
    m.timeline("!r:test", &ev, false, false);
    let msg = m.state().timeline(c)[0];
    assert_eq!(msg.text, "*Ship* it @alice");
    assert!(msg.mentions.contains(&m.me().unwrap()), "a pill is a mention");
    // A ChatCraft message (HTML reproducible from its body) keeps its exact source.
    let src = "*Ship* it :rocket: @alice";
    let html = crate::html::from_markup(src, &|h: &str| (h == "alice").then(|| ME.to_string()));
    let ev = json!({"type": "m.room.message", "event_id": "$cc", "sender": "@bob:test", "origin_server_ts": 2,
        "content": {"msgtype": "m.text", "body": src, "format": "org.matrix.custom.html", "formatted_body": html}});
    m.timeline("!r:test", &ev, false, false);
    assert_eq!(m.state().timeline(c)[1].text, src);
}

#[test]
fn pins_mutes_and_saved_items_follow_the_account() {
    let mut m = Mapper::new(ME);
    let info = |pinned: Vec<String>, muted: bool| RoomInfo {
        room_id: "!r:test".into(),
        name: "general".into(),
        members: vec![(ME.into(), None)],
        pinned,
        muted,
        ..RoomInfo::default()
    };
    let c = m.upsert_room(&info(vec![], false));
    m.timeline("!r:test", &msg("$1", ME, "keep this"), false, false);
    m.timeline("!r:test", &msg("$2", ME, "and this"), false, false);
    m.upsert_room(&info(vec!["$1".into(), "$unknown".into()], true));
    let tl = m.state().timeline(c);
    assert!(tl[0].pinned && !tl[1].pinned);
    assert!(m.state().muted[&m.me().unwrap()].contains(&c));
    m.upsert_room(&info(vec!["$2".into()], false));
    let tl = m.state().timeline(c);
    assert!(!tl[0].pinned && tl[1].pinned);
    assert!(!m.state().muted[&m.me().unwrap()].contains(&c));
    m.set_saved_list(&[("!r:test".into(), "$2".into()), ("!old:test".into(), "$gone".into())]);
    let me = m.me().unwrap();
    assert_eq!(m.state().saved[&me].len(), 1);
    m.set_saved_list(&[]);
    assert!(m.state().saved[&me].is_empty());
}

#[test]
fn typing_comes_and_goes() {
    let mut m = Mapper::new(ME);
    let c = room(&mut m, "!r:test", "general", &[ME, "@bob:test"], false);
    m.set_typing("!r:test", &["@bob:test".into(), ME.into()]);
    let bob = m.state().user_by_handle("bob").unwrap().id;
    assert_eq!(m.state().typing[&c].iter().copied().collect::<Vec<_>>(), vec![bob], "we never see ourselves typing");
    m.timeline("!r:test", &msg("$t", "@bob:test", "done typing"), false, false);
    assert!(m.state().typing[&c].is_empty(), "posting ends typing");
    m.set_typing("!r:test", &[]);
    assert!(m.state().typing.get(&c).is_none_or(std::collections::BTreeSet::is_empty), "nobody is typing");
}

#[test]
fn read_receipts_move_forward_and_show_where_people_stopped() {
    let mut m = Mapper::new(ME);
    let c = room(&mut m, "!r:test", "general", &[ME, "@bob:test", "@cy:test"], false);
    m.timeline("!r:test", &msg("$1", ME, "one"), false, false);
    m.timeline("!r:test", &msg("$2", ME, "two"), false, false);
    m.set_receipt("!r:test", "$2", "@bob:test");
    m.set_receipt("!r:test", "$1", "@cy:test");
    m.set_receipt("!r:test", "$1", "@bob:test"); // older: ignored
    m.set_receipt("!r:test", "$2", ME); // ours: not a receipt
    let tl: Vec<MessageId> = m.state().timeline(c).iter().map(|x| x.id).collect();
    let name = |u: &UserId| m.state().user(*u).unwrap().handle.clone();
    assert_eq!(m.state().seen_at(tl[1]).iter().map(name).collect::<Vec<_>>(), vec!["bob"]);
    assert_eq!(m.state().seen_at(tl[0]).iter().map(name).collect::<Vec<_>>(), vec!["cy"]);
    let mut first: Vec<String> = m.state().read_by(tl[0]).iter().map(name).collect();
    first.sort();
    assert_eq!(first, vec!["bob", "cy"], "reading message two means message one was read too");
}

#[test]
fn dms_handles_presence_status() {
    let mut m = Mapper::new(ME);
    let c = m.upsert_room(&RoomInfo {
        room_id: "!d:test".into(),
        direct: true,
        members: vec![(ME.into(), None), ("@bob:test".into(), Some("Bob B".into()))],
        ..RoomInfo::default()
    });
    assert_eq!(m.state().channel(c).unwrap().kind, ChannelKind::Direct);
    assert_eq!(m.state().title(c, m.me()), "Bob B");
    // A message from Bob (sender without a name) doesn't reset his name.
    m.timeline("!d:test", &msg("$n", "@bob:test", "hi"), true, false);
    assert_eq!(m.state().title(c, m.me()), "Bob B");
    // Same localpart on another server gets a qualified handle.
    let other = m.user("@bob:elsewhere.org", None);
    assert_eq!(m.state().user(other).unwrap().handle, "bob:elsewhere.org");
    assert_eq!(m.resolve_mention("@bob"), m.state().user_by_handle("bob").map(|u| u.id));
    m.presence("@bob:test", "unavailable", Some(":palm_tree: On holiday"));
    let bob = m.state().user_by_handle("bob").unwrap();
    assert_eq!(bob.presence, Presence::Away);
    assert_eq!((bob.status.emoji.as_str(), bob.status.text.as_str()), ("palm_tree", "On holiday"));
    assert_eq!(format_status("On holiday", "palm_tree"), ":palm_tree: On holiday");
}

#[test]
fn hostile_events_never_panic() {
    let mut m = Mapper::new(ME);
    room(&mut m, "!r:test", "", &[ME], false);
    for ev in [
        json!(null),
        json!({}),
        json!({"event_id": "$a"}),
        json!({"event_id": 5, "sender": [], "type": {}}),
        json!({"type": "m.room.message", "event_id": "$b", "sender": "@x", "content": {"body": 7}}),
        json!({"type": "m.reaction", "event_id": "$c", "sender": "@x", "content": {"m.relates_to": {"rel_type": "m.annotation", "event_id": "$missing", "key": "x".repeat(10_000)}}}),
        json!({"type": "m.room.redaction", "event_id": "$d", "sender": "@x", "redacts": "$nothing"}),
        json!({"type": "m.room.message", "event_id": "$e", "sender": "@x", "origin_server_ts": "soon", "content": {"body": "x", "m.relates_to": {"rel_type": "m.replace", "event_id": "$nope"}}}),
    ] {
        m.timeline("!r:test", &ev, false, false);
        m.timeline("!unknown:test", &ev, true, true);
    }
    m.fully_read("!r:test", "$never");
    m.presence("@nobody:test", "online", None);
    m.left_room("!unknown:test");
}
