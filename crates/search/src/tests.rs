use std::collections::BTreeSet;

use chatcraft_model::{Channel, ChannelKind, Event, User};

use super::*;

fn state() -> State {
    let mut s = State::default();
    let users = [(1, "ada"), (2, "bo"), (3, "cy")];
    for (id, h) in users {
        s.apply_event(&Event::UserJoined { user: User { id: UserId(id), handle: h.into(), ..User::default() } });
    }
    let ch = |id, name: &str, kind, members: &[u64]| Channel {
        id: ChannelId(id),
        name: name.into(),
        kind,
        members: members.iter().map(|m| UserId(*m)).collect(),
        ..Channel::default()
    };
    s.apply_event(&Event::ChannelCreated { channel: ch(1, "general", ChannelKind::Public, &[1, 2, 3]) });
    s.apply_event(&Event::ChannelCreated { channel: ch(2, "secret", ChannelKind::Private, &[1, 2]) });
    let day = Ts::parse_date("2026-10-30").unwrap();
    let posts = [
        (1, 1, 1, "Deployment went out, see https://example.org/run"),
        (2, 1, 2, "Lunch at noon?"),
        (3, 2, 2, "the secret deploy plan"),
        (4, 1, 3, "@ada can you review the deploy checklist"),
    ];
    for (id, ch, author, text) in posts {
        let mut m = Message {
            id: MessageId(id),
            channel: ChannelId(ch),
            author: UserId(author),
            ts: day.plus_millis(id as i64),
            text: text.into(),
            ..Message::default()
        };
        if text.contains("@ada") {
            m.mentions = BTreeSet::from([UserId(1)]);
        }
        s.apply_event(&Event::MessagePosted { message: m });
    }
    s
}

fn ids(h: Vec<Hit>) -> Vec<u64> {
    let mut v: Vec<u64> = h.into_iter().map(|h| h.message.0).collect();
    v.sort_unstable();
    v
}

#[test]
fn words_prefix_and_visibility() {
    let s = state();
    assert_eq!(ids(search(&s, UserId(3), &Query::parse("deploy"), 20)), vec![1, 4], "cy can't see #secret");
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("deploy"), 20)), vec![1, 3, 4]);
}

#[test]
fn modifiers() {
    let s = state();
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("deploy in:#secret"), 20)), vec![3]);
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("from:@cy"), 20)), vec![4]);
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("to:@ada"), 20)), vec![4]);
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("to:me"), 20)), vec![4]);
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("has:link"), 20)), vec![1]);
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("deploy -checklist"), 20)), vec![1, 3]);
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("\"deploy plan\""), 20)), vec![3]);
    assert_eq!(ids(search(&s, UserId(1), &Query::parse("lunch on:2026-10-30"), 20)), vec![2]);
    assert!(search(&s, UserId(1), &Query::parse("lunch before:2026-10-30"), 20).is_empty());
    assert!(search(&s, UserId(1), &Query::parse("from:@nobody"), 20).is_empty());
    assert_eq!(Query::parse("is:weird has:cats").unknown.len(), 2);
}

#[test]
fn hostile_queries() {
    let s = state();
    for q in ["", "\"", "\"\"", "-", "::", "in:", "before:2026-13-45", "a:b:c", &"x ".repeat(5000)] {
        let _ = search(&s, UserId(1), &Query::parse(q), usize::MAX);
    }
}
