use chatcraft_model::{ChannelKind, Event};

use super::*;

fn t(n: i64) -> Ts {
    Ts(1_800_000_000_000 + n * 1000)
}

fn setup() -> (Hub, UserId, UserId, UserId, ChannelId) {
    let mut h = Hub::create("Test", t(0));
    let (a, _) = h.sign_in("ada", "Ada", t(1)).unwrap();
    let (b, _) = h.sign_in("bo", "Bo", t(2)).unwrap();
    let (c, _) = h.sign_in("cy", "Cy", t(3)).unwrap();
    let general = h.state().workspace.default_channel.unwrap();
    (h, a, b, c, general)
}

fn post(h: &mut Hub, who: UserId, ch: ChannelId, text: &str) -> MessageId {
    let r = h.apply(who, Op::PostMessage { channel: ch, text: text.into(), thread: None, broadcast: false }, t(10)).unwrap();
    MessageId(r.result["message"].as_u64().unwrap())
}

#[test]
fn first_user_owns_and_everyone_joins_defaults() {
    let (h, a, b, _, general) = setup();
    assert_eq!(h.state().user(a).unwrap().role, Role::Owner);
    assert_eq!(h.state().user(b).unwrap().role, Role::Member);
    assert!(h.state().is_member(b, general));
    assert!(h.state().channel_by_name("random").unwrap().members.contains(&b));
}

#[test]
fn posting_resolves_mentions_and_marks_read() {
    let (mut h, a, b, _, general) = setup();
    let m = post(&mut h, a, general, "hey @bo and @nobody");
    let msg = h.state().message(m).unwrap();
    assert_eq!(msg.mentions.iter().copied().collect::<Vec<_>>(), vec![b]);
    assert_eq!(h.state().last_read(a, general), m);
    assert_eq!(h.state().unread(b, general), (1, 1));
}

#[test]
fn private_channels_hide_and_invites_resync() {
    let (mut h, a, b, c, _) = setup();
    let r = h
        .apply(
            a,
            Op::CreateChannel {
                name: "Secret Plans".into(),
                private: true,
                topic: String::new(),
                purpose: String::new(),
                members: vec![b],
                external: vec![],
            },
            t(5),
        )
        .unwrap();
    let ch = ChannelId(r.result["channel"].as_u64().unwrap());
    assert_eq!(h.state().channel(ch).unwrap().name, "secret-plans");
    // Cy can't see, join or post, and gets "not found" rather than "forbidden".
    assert!(matches!(h.apply(c, Op::JoinChannel { channel: ch }, t(6)), Err(HubError::NotFound(_))));
    assert!(matches!(
        h.apply(c, Op::PostMessage { channel: ch, text: "x".into(), thread: None, broadcast: false }, t(6)),
        Err(HubError::NotFound(_))
    ));
    let out = h.apply(a, Op::Invite { channel: ch, users: vec![c], external: vec![] }, t(7)).unwrap();
    assert_eq!(out.resync, vec![c]);
    assert!(h.state().can_see(c, ch));
}

#[test]
fn only_authors_edit_and_admins_delete() {
    let (mut h, a, b, _, general) = setup();
    let m = post(&mut h, b, general, "typo here");
    assert!(matches!(h.apply(a, Op::EditMessage { message: m, text: "x".into() }, t(11)), Err(HubError::Forbidden(_))));
    h.apply(b, Op::EditMessage { message: m, text: "fixed".into() }, t(11)).unwrap();
    assert_eq!(h.state().message(m).unwrap().text, "fixed");
    // Ada is the owner, so she may delete Bo's message.
    h.apply(a, Op::DeleteMessage { message: m }, t(12)).unwrap();
    assert!(h.state().message(m).unwrap().deleted);
    assert!(h.apply(b, Op::EditMessage { message: m, text: "again".into() }, t(13)).is_err());
}

#[test]
fn threads_reactions_and_idempotence() {
    let (mut h, a, b, _, general) = setup();
    let root = post(&mut h, a, general, "root");
    let r1 = h.apply(b, Op::PostMessage { channel: general, text: "reply".into(), thread: Some(root), broadcast: false }, t(20)).unwrap();
    let reply = MessageId(r1.result["message"].as_u64().unwrap());
    // Replying to a reply lands in the same thread.
    h.apply(a, Op::PostMessage { channel: general, text: "nested".into(), thread: Some(reply), broadcast: false }, t(21)).unwrap();
    assert_eq!(h.state().message(root).unwrap().reply_count, 2);
    let on = |h: &mut Hub, e: &str, on: bool| h.apply(b, Op::React { message: root, emoji: e.into(), on }, t(22)).unwrap().events.len();
    assert_eq!(on(&mut h, ":tada:", true), 1);
    assert_eq!(on(&mut h, "tada", true), 0, "reacting twice is a no-op");
    assert_eq!(on(&mut h, "🎉", false), 1, "glyphs map to shortcodes");
    assert!(h.apply(b, Op::React { message: root, emoji: "not valid!".into(), on: true }, t(22)).is_err());
}

#[test]
fn dms_are_found_again() {
    let (mut h, a, b, c, _) = setup();
    let one = h.apply(a, Op::OpenDm { users: vec![b], external: vec![] }, t(5)).unwrap();
    let again = h.apply(b, Op::OpenDm { users: vec![a], external: vec![] }, t(6)).unwrap();
    assert_eq!(one.result, again.result);
    assert!(again.events.is_empty());
    let grp = h.apply(a, Op::OpenDm { users: vec![b, c], external: vec![] }, t(7)).unwrap();
    let id = ChannelId(grp.result["channel"].as_u64().unwrap());
    assert_eq!(h.state().channel(id).unwrap().kind, ChannelKind::Group);
    assert!(h.apply(a, Op::LeaveChannel { channel: id }, t(8)).is_err());
}

#[test]
fn default_channel_rules() {
    let (mut h, _, b, _, general) = setup();
    assert!(matches!(h.apply(b, Op::LeaveChannel { channel: general }, t(5)), Err(HubError::Forbidden(_))));
    assert!(h.apply(b, Op::Archive { channel: general, archived: true }, t(5)).is_err());
    assert!(
        h.apply(
            b,
            Op::CreateChannel {
                name: "general".into(),
                private: false,
                topic: String::new(),
                purpose: String::new(),
                members: vec![],
                external: vec![]
            },
            t(5)
        )
        .is_err()
    );
}

#[test]
fn replay_reproduces_state_and_continues_ids() {
    let (mut h, a, _, _, general) = setup();
    post(&mut h, a, general, "one");
    let log = {
        let mut all = Vec::new();
        // Everything committed so far is unjournaled in a fresh hub.
        all.extend(h.take_unjournaled());
        all
    };
    let mut again = Hub::replay(log);
    assert_eq!(again.state().messages.len(), h.state().messages.len());
    assert_eq!(again.seq(), h.seq());
    let m = post(&mut again, a, general, "two");
    assert!(h.state().message(m).is_none(), "ids continue after the replayed ones");
}

#[test]
fn demo_workspace_builds_with_unreads_for_me() {
    let h = sample::demo(t(100_000)).unwrap();
    let me = h.state().user_by_handle(sample::ME).unwrap().id;
    let sums = h.state().summaries(me);
    assert!(sums.iter().any(|s| s.unread > 0));
    assert!(sums.iter().any(|s| s.mentions > 0));
    assert!(sums.iter().any(|s| s.kind == ChannelKind::Private));
    assert!(sums.iter().any(|s| s.kind == ChannelKind::Group));
    assert!(h.state().messages.len() > 25);
}

#[test]
fn hostile_ops_never_panic() {
    let (mut h, a, ..) = setup();
    let big = "x".repeat(100_000);
    let ops = vec![
        Op::PostMessage { channel: ChannelId(u64::MAX), text: big.clone(), thread: Some(MessageId(u64::MAX)), broadcast: true },
        Op::PostMessage { channel: ChannelId(1), text: big.clone(), thread: None, broadcast: false },
        Op::PostMessage { channel: ChannelId(1), text: "   ".into(), thread: None, broadcast: false },
        Op::EditMessage { message: MessageId(0), text: String::new() },
        Op::React { message: MessageId(1), emoji: big.clone(), on: true },
        Op::CreateChannel {
            name: big.clone(),
            private: true,
            topic: big.clone(),
            purpose: String::new(),
            members: vec![UserId(999)],
            external: vec![],
        },
        Op::CreateChannel { name: "#".into(), private: false, topic: String::new(), purpose: String::new(), members: vec![], external: vec![] },
        Op::OpenDm { users: (0..100).map(UserId).collect(), external: vec![] },
        Op::SetStatus { text: big.clone(), emoji: "!!".into(), minutes: Some(u32::MAX) },
        Op::SetProfile { display_name: Some(String::new()), title: Some(big.clone()) },
        Op::MarkRead { channel: ChannelId(1), upto: MessageId(u64::MAX) },
        Op::RenameChannel { channel: ChannelId(1), name: "general".into() },
        Op::Invite { channel: ChannelId(1), users: vec![UserId(0)], external: vec![] },
        Op::Remove { channel: ChannelId(1), user: UserId(u64::MAX) },
        Op::RenameWorkspace { name: big },
    ];
    for op in ops {
        let _ = h.apply(a, op, t(30));
    }
    assert!(h.apply(UserId(424_242), Op::SetPresence { presence: chatcraft_model::Presence::Away }, t(31)).is_err());
    // A status with a huge duration saturates instead of overflowing.
    h.apply(a, Op::SetStatus { text: "ok".into(), emoji: "x".into(), minutes: Some(u32::MAX) }, Ts(i64::MAX - 5)).unwrap();
    assert!(matches!(h.state().user(a).unwrap().status.until, Some(Ts(i64::MAX))));
    let _ = Event::WorkspaceRenamed { name: String::new() };
}
