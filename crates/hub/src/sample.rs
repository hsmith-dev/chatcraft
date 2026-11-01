//! The demo workspace: an original, fictional team ("Kestrel Labs") with a few days of
//! conversation, used by `--demo`, the web build, screenshots and tests. All names and text are
//! our own.

use chatcraft_core::{ChannelId, MessageId, Ts, UserId};
use chatcraft_model::{Presence, State};
use chatcraft_proto::Op;

use crate::{Hub, HubError};

/// The person you are in the demo.
pub const ME: &str = "morgan";

struct B {
    hub: Hub,
    t: Ts,
}

impl B {
    fn user(&mut self, handle: &str, name: &str, title: &str) -> Result<UserId, HubError> {
        let (id, _) = self.hub.sign_in(handle, name, self.t)?;
        self.hub.apply(id, Op::SetProfile { display_name: None, title: Some(title.into()) }, self.t)?;
        Ok(id)
    }
    fn at(&mut self, minutes: i64) {
        self.t = self.t.plus_millis(minutes * 60_000);
    }
    fn op(&mut self, who: UserId, op: Op) -> Result<serde_json::Value, HubError> {
        Ok(self.hub.apply(who, op, self.t)?.result)
    }
    fn say(&mut self, who: UserId, ch: ChannelId, text: &str) -> Result<MessageId, HubError> {
        self.at(3);
        let r = self.op(who, Op::PostMessage { channel: ch, text: text.into(), thread: None, broadcast: false })?;
        Ok(r.get("message").and_then(serde_json::Value::as_u64).map(MessageId).unwrap_or_default())
    }
    fn reply(&mut self, who: UserId, ch: ChannelId, root: MessageId, text: &str) -> Result<MessageId, HubError> {
        self.at(2);
        let r = self.op(who, Op::PostMessage { channel: ch, text: text.into(), thread: Some(root), broadcast: false })?;
        Ok(r.get("message").and_then(serde_json::Value::as_u64).map(MessageId).unwrap_or_default())
    }
    fn react(&mut self, who: UserId, m: MessageId, emoji: &str) -> Result<(), HubError> {
        self.op(who, Op::React { message: m, emoji: emoji.into(), on: true }).map(drop)
    }
    fn channel(&mut self, who: UserId, name: &str, private: bool, topic: &str, purpose: &str, members: &[UserId]) -> Result<ChannelId, HubError> {
        let r =
            self.op(who, Op::CreateChannel { name: name.into(), private, topic: topic.into(), purpose: purpose.into(), members: members.to_vec() })?;
        Ok(r.get("channel").and_then(serde_json::Value::as_u64).map(ChannelId).unwrap_or_default())
    }
    fn dm(&mut self, who: UserId, with: &[UserId]) -> Result<ChannelId, HubError> {
        let r = self.op(who, Op::OpenDm { users: with.to_vec() })?;
        Ok(r.get("channel").and_then(serde_json::Value::as_u64).map(ChannelId).unwrap_or_default())
    }
    fn read(&mut self, who: UserId, ch: ChannelId, upto: MessageId) -> Result<(), HubError> {
        self.op(who, Op::MarkRead { channel: ch, upto }).map(drop)
    }
}

fn find(s: &State, name: &str) -> ChannelId {
    s.channel_by_name(name).map(|c| c.id).unwrap_or_default()
}

/// Builds the demo workspace; the newest message lands a few minutes before `now`.
pub fn demo(now: Ts) -> Result<Hub, HubError> {
    let start = now.plus_millis(-(26 * 60 + 40) * 60_000);
    let mut b = B { hub: Hub::create("Kestrel Labs", start), t: start };
    let morgan = b.user(ME, "Morgan Lee", "Product lead")?;
    let ada = b.user("ada", "Ada Park", "Product designer")?;
    let ravi = b.user("ravi", "Ravi Menon", "Backend engineer")?;
    let sofia = b.user("sofia", "Sofia Alvarez", "Engineering manager")?;
    let kenji = b.user("kenji", "Kenji Watanabe", "Mobile engineer")?;
    let noor = b.user("noor", "Noor Haddad", "Marketing lead")?;
    let bot = b.hub.add_bot("buildbot", "Build Bot", b.t)?;
    let general = find(b.hub.state(), "general");
    let random = find(b.hub.state(), "random");

    let design = b.channel(
        ada,
        "design",
        false,
        "Beacon 2.0 visual refresh · crits Tue/Thu",
        "Design reviews, mocks and the component library.",
        &[morgan, kenji, sofia],
    )?;
    let eng = b.channel(
        sofia,
        "engineering",
        false,
        "Release train: Thursdays · on-call: @ravi",
        "Everything about building and shipping Beacon.",
        &[morgan, ravi, kenji, ada],
    )?;
    let launch = b.channel(morgan, "launch-plan", true, "Beacon 2.0 launch — Nov 12", "Private planning for the 2.0 launch.", &[sofia, noor, ada])?;
    let ship = b.channel(ravi, "ship-it", false, "Deploy notifications", "Build Bot posts every deploy here.", &[morgan, sofia, kenji, bot])?;

    // Yesterday.
    let w = b.say(sofia, general, "Welcome to the new workspace, everyone! :wave: Channels for each team are in the sidebar — say hi in #random.")?;
    b.react(morgan, w, "wave")?;
    b.react(ada, w, "wave")?;
    b.react(kenji, w, "tada")?;
    b.say(noor, random, "Office plant update: the fiddle-leaf fig has *three* new leaves :seedling:")?;
    let k = b.say(kenji, random, "Mine is just surviving. Recommendations for a low-light desk plant?")?;
    b.reply(noor, random, k, "Pothos. Unkillable, and it trails nicely off a shelf.")?;
    b.reply(ada, random, k, "+1 for pothos. Or a ZZ plant if you forget to water for weeks :sweat_smile:")?;

    b.at(60);
    let crit = b.say(ada, design, "Posted the first pass of the new onboarding flow. Looking for feedback on the empty states especially.\n- Welcome screen\n- Team invite step\n- First-channel suggestions")?;
    b.reply(kenji, design, crit, "The invite step feels long on a small phone — could the copy-link option come first?")?;
    b.reply(morgan, design, crit, "Agree with Kenji. Also love the illustrations on the empty states :heart_eyes:")?;
    b.reply(ada, design, crit, "Good call, moving copy-link up. Thanks both!")?;
    b.react(sofia, crit, "eyes")?;

    let rel = b.say(sofia, eng, "Release train for *Thursday*: please have PRs reviewed by Wednesday noon. Checklist is pinned.")?;
    b.op(sofia, Op::Pin { message: rel, pinned: true })?;
    b.react(ravi, rel, "white_check_mark")?;
    b.react(kenji, rel, "white_check_mark")?;
    let mig = b.say(ravi, eng, "Heads up: the message-index migration runs tonight. Expect search to be a little slow for ~20 minutes.")?;
    b.reply(sofia, eng, mig, "Thanks for the heads-up. Is there a rollback plan if it runs long?")?;
    b.reply(ravi, eng, mig, "Yes — the old index stays live until the new one passes checks, then we flip a flag.")?;

    let l1 = b.say(
        morgan,
        launch,
        "Draft launch timeline is up. Key dates:\n1. Nov 5 — feature freeze\n2. Nov 9 — press preview\n3. Nov 12 — public launch :rocket:",
    )?;
    b.react(noor, l1, "rocket")?;
    b.react(sofia, l1, "+1")?;
    b.reply(noor, launch, l1, "I'll have the blog post draft ready by Nov 3 so legal has time to review.")?;

    let dm_ada = b.dm(ada, &[morgan])?;
    b.say(morgan, dm_ada, "Loved the onboarding crit today. Want to pair on the tour copy tomorrow?")?;
    b.say(ada, dm_ada, "Yes please! Morning works best for me.")?;
    let dm_ravi = b.dm(morgan, &[ravi])?;
    b.say(ravi, dm_ravi, "Do you have a minute to look at the notification settings spec before I start?")?;
    b.say(morgan, dm_ravi, "Just read it — looks great. One question about quiet hours, left a comment.")?;
    let grp = b.dm(noor, &[morgan, sofia])?;
    b.say(noor, grp, "Quick sync on launch comms Monday at 10?")?;
    b.say(sofia, grp, "Works for me.")?;

    // Morgan has read everything up to here.
    for ch in [general, random, design, eng, launch, ship, dm_ada, dm_ravi, grp] {
        let latest = b.hub.state().latest(ch).unwrap_or_default();
        b.read(morgan, ch, latest)?;
    }

    // This morning (unread for Morgan).
    b.at(16 * 60);
    let d1 = b.say(bot, ship, "`beacon-web` *v2.0.0-rc.3* deployed to staging :white_check_mark: (build 1842, 6m 12s)")?;
    b.react(ravi, d1, "rocket")?;
    b.read(morgan, ship, d1)?;
    b.say(ada, design, "New icon set is in the library :sparkles: — every icon is drawn in code, so they scale and theme perfectly.")?;
    b.say(kenji, design, "These look crisp on the tablet build. The thread icon reads much better now.")?;
    let q = b.say(kenji, eng, "@morgan quick question: should the read-receipt setting default to on or off for new workspaces?")?;
    b.reply(ravi, eng, q, "Off by default would match our privacy principles.")?;
    b.say(sofia, eng, "Reminder that the retro is at 3pm today. Bring one thing that went well :bulb:")?;
    b.say(ada, dm_ada, "Here's the doc for tomorrow: <https://example.org/beacon/tour-copy|Tour copy draft>. See you at 9!")?;
    b.say(noor, launch, "Press list is final — 24 outlets. @here please flag anyone missing by Friday.")?;
    b.say(noor, general, "Lunch-and-learn on Thursday: \"Writing for humans\" in the big room. Snacks provided :pizza:")?;

    // Presence and statuses.
    b.op(ravi, Op::SetPresence { presence: Presence::Away })?;
    b.op(kenji, Op::SetStatus { text: "Flying to Osaka".into(), emoji: "airplane".into(), minutes: None })?;
    b.op(kenji, Op::SetPresence { presence: Presence::Offline })?;
    b.op(noor, Op::SetStatus { text: "Heads down on launch copy".into(), emoji: "memo".into(), minutes: Some(120) })?;
    b.op(noor, Op::SetPresence { presence: Presence::Dnd })?;
    b.op(sofia, Op::SetStatus { text: "In meetings until 2".into(), emoji: "calendar".into(), minutes: Some(180) })?;
    let _ = b.hub.sign_out(bot);
    Ok(b.hub)
}
