use std::collections::BTreeSet;

use super::*;

fn user(id: u64, handle: &str) -> User {
    User { id: UserId(id), handle: handle.into(), display_name: handle.to_uppercase(), ..User::default() }
}

fn channel(id: u64, name: &str, kind: ChannelKind, members: &[u64]) -> Channel {
    Channel { id: ChannelId(id), name: name.into(), kind, members: members.iter().map(|m| UserId(*m)).collect(), ..Channel::default() }
}

fn msg(id: u64, ch: u64, author: u64, text: &str) -> Message {
    Message { id: MessageId(id), channel: ChannelId(ch), author: UserId(author), ts: Ts(id as i64 * 1000), text: text.into(), ..Message::default() }
}

fn seeded() -> State {
    let mut s = State::default();
    for e in [
        Event::UserJoined { user: user(1, "ada") },
        Event::UserJoined { user: user(2, "bo") },
        Event::UserJoined { user: user(3, "cy") },
        Event::ChannelCreated { channel: channel(1, "general", ChannelKind::Public, &[1, 2, 3]) },
        Event::ChannelCreated { channel: channel(2, "secret", ChannelKind::Private, &[1, 2]) },
        Event::MessagePosted { message: msg(1, 1, 1, "hello") },
        Event::MessagePosted { message: msg(2, 2, 2, "psst") },
    ] {
        s.apply_event(&e);
    }
    s
}

#[test]
fn threads_count_replies_and_deletions() {
    let mut s = seeded();
    let mut r = msg(3, 1, 2, "reply");
    r.thread = Some(MessageId(1));
    s.apply_event(&Event::MessagePosted { message: r.clone() });
    // Re-applying the same event is idempotent.
    s.apply_event(&Event::MessagePosted { message: r });
    assert_eq!(s.message(MessageId(1)).unwrap().reply_count, 1);
    assert_eq!(s.replies(MessageId(1)).len(), 1);
    assert_eq!(s.timeline(ChannelId(1)).len(), 1, "replies stay out of the timeline");
    s.apply_event(&Event::MessageDeleted { message: MessageId(3), at: Ts(9) });
    assert_eq!(s.message(MessageId(1)).unwrap().reply_count, 0);
    assert!(s.replies(MessageId(1)).is_empty());
}

#[test]
fn unread_and_mentions() {
    let mut s = seeded();
    let mut m = msg(4, 1, 2, "@cy look");
    m.mentions = BTreeSet::from([UserId(3)]);
    s.apply_event(&Event::MessagePosted { message: m });
    assert_eq!(s.unread(UserId(3), ChannelId(1)), (2, 1));
    assert_eq!(s.unread(UserId(2), ChannelId(1)), (1, 0), "own messages never count");
    s.apply_event(&Event::ReadMarked { user: UserId(3), channel: ChannelId(1), upto: MessageId(4) });
    assert_eq!(s.unread(UserId(3), ChannelId(1)), (0, 0));
    // Read cursors never move backwards.
    s.apply_event(&Event::ReadMarked { user: UserId(3), channel: ChannelId(1), upto: MessageId(1) });
    assert_eq!(s.last_read(UserId(3), ChannelId(1)), MessageId(4));
}

#[test]
fn private_channels_are_filtered() {
    let s = seeded();
    assert!(s.can_see(UserId(3), ChannelId(1)));
    assert!(!s.can_see(UserId(3), ChannelId(2)));
    let v = s.visible_to(UserId(3));
    assert!(v.channel(ChannelId(2)).is_none());
    assert!(v.message(MessageId(2)).is_none());
    assert!(!s.event_visible(&Event::MessagePosted { message: msg(2, 2, 2, "psst") }, UserId(3)));
    assert!(s.event_visible(&Event::MemberLeft { channel: ChannelId(2), user: UserId(3), by: None }, UserId(3)));
    assert!(!s.event_visible(&Event::ReadMarked { user: UserId(1), channel: ChannelId(1), upto: MessageId(1) }, UserId(2)));
}

#[test]
fn reactions_toggle_and_clean_up() {
    let mut s = seeded();
    let add = |u| Event::ReactionAdded { message: MessageId(1), emoji: "tada".into(), user: UserId(u) };
    s.apply_event(&add(1));
    s.apply_event(&add(2));
    s.apply_event(&add(2));
    assert_eq!(s.message(MessageId(1)).unwrap().reactions[0].1.len(), 2);
    s.apply_event(&Event::ReactionRemoved { message: MessageId(1), emoji: "tada".into(), user: UserId(1) });
    s.apply_event(&Event::ReactionRemoved { message: MessageId(1), emoji: "tada".into(), user: UserId(2) });
    assert!(s.message(MessageId(1)).unwrap().reactions.is_empty());
}

#[test]
fn snapshot_round_trips_through_json() {
    let s = seeded();
    let json = serde_json::to_string(&s).unwrap();
    let mut back: State = serde_json::from_str(&json).unwrap();
    back.reindex();
    assert_eq!(back.timeline(ChannelId(1)).len(), 1);
    assert_eq!(back.users.len(), 3);
}

#[test]
fn events_on_missing_things_are_ignored() {
    let mut s = State::default();
    for e in [
        Event::MessageDeleted { message: MessageId(9), at: Ts(0) },
        Event::ReactionAdded { message: MessageId(9), emoji: "x".into(), user: UserId(1) },
        Event::MemberJoined { channel: ChannelId(9), user: UserId(1), by: None },
        Event::ProfileChanged { user: UserId(9), display_name: Some("x".into()), title: None },
        Event::MessagePosted { message: Message { thread: Some(MessageId(77)), ..msg(5, 9, 9, "orphan") } },
    ] {
        s.apply_event(&e);
    }
    assert!(s.channels.is_empty());
    assert_eq!(s.unread(UserId(1), ChannelId(9)), (0, 0));
}

#[test]
fn titles_and_resolution() {
    let mut s = seeded();
    s.apply_event(&Event::ChannelCreated { channel: channel(3, "", ChannelKind::Direct, &[1, 2]) });
    assert_eq!(s.title(ChannelId(3), Some(UserId(1))), "BO");
    assert_eq!(s.title(ChannelId(1), None), "#general");
    assert_eq!(s.resolve_channel("#General", None), Some(ChannelId(1)));
    assert_eq!(s.resolve_channel("@bo", Some(UserId(1))), Some(ChannelId(3)));
    assert_eq!(s.resolve_user("@ADA"), Some(UserId(1)));
    assert_eq!(s.resolve_user("U2"), Some(UserId(2)));
    assert_eq!(s.resolve_user("U99"), None);
}

#[test]
fn older_history_loaded_later_still_sorts_by_time() {
    let mut s = seeded();
    // Message 1 (ts 1000) is in #general. Ids 20 and 21 arrive later but were sent earlier.
    let mut old = msg(20, 1, 2, "older");
    old.ts = Ts(500);
    let mut oldest = msg(21, 1, 2, "oldest");
    oldest.ts = Ts(100);
    s.apply_event(&Event::MessagePosted { message: old });
    s.apply_event(&Event::MessagePosted { message: oldest });
    let tl: Vec<u64> = s.timeline(ChannelId(1)).iter().map(|m| m.id.0).collect();
    assert_eq!(tl, vec![21, 20, 1], "time order, not id order");
    assert_eq!(s.latest(ChannelId(1)), Some(MessageId(1)));
    // Reading up to message 1 reads the older ones too.
    s.apply_event(&Event::ReadMarked { user: UserId(3), channel: ChannelId(1), upto: MessageId(1) });
    assert_eq!(s.unread(UserId(3), ChannelId(1)), (0, 0));
    // An older message never moves the cursor back.
    s.apply_event(&Event::ReadMarked { user: UserId(3), channel: ChannelId(1), upto: MessageId(21) });
    assert_eq!(s.last_read(UserId(3), ChannelId(1)), MessageId(1));
    // A snapshot keeps the order.
    let mut back: State = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    back.reindex();
    assert_eq!(back.timeline(ChannelId(1)).iter().map(|m| m.id.0).collect::<Vec<_>>(), vec![21, 20, 1]);
}
