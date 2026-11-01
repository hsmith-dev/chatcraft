//! The client session.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use chatcraft_core::{ChannelId, MessageId, UserId};
use chatcraft_model::{ChannelKind, ChannelSummary, Event, State};
use chatcraft_proto::{ClientFrame, Op, ServerFrame};
use chatcraft_search::Hit;
use serde::Serialize;
use serde_json::{Value, json};

use crate::transport::{LinkStatus, Loopback, Transport};
use crate::{CmdError, CmdResult, Registry, cmd};

/// What the right-hand pane shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Pane {
    #[default]
    None,
    Thread,
    Details,
    Search,
    Activity,
    Saved,
    Profile,
}

/// Navigation state.
#[derive(Clone, Debug, Default, Serialize)]
pub struct View {
    pub channel: Option<ChannelId>,
    pub thread: Option<MessageId>,
    pub pane: Pane,
    /// The person shown in the profile pane.
    pub profile: Option<UserId>,
    /// Message to scroll to and highlight (from search or a link).
    pub focus: Option<MessageId>,
    /// The first message that was unread when the open conversation was opened (the "New" line).
    pub unread_from: Option<MessageId>,
    #[serde(skip)]
    pub(crate) back: Vec<ChannelId>,
    #[serde(skip)]
    pub(crate) forward: Vec<ChannelId>,
}

/// A transient message for the person (shown as a toast).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

/// Something that deserves a notification: a mention, a direct message, a reply in your thread.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Alert {
    pub channel: ChannelId,
    pub message: MessageId,
    pub title: String,
    pub body: String,
}

/// The sidebar, in display order.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Sidebar {
    pub starred: Vec<ChannelSummary>,
    pub channels: Vec<ChannelSummary>,
    pub dms: Vec<ChannelSummary>,
}

impl Sidebar {
    pub fn order(&self) -> impl Iterator<Item = &ChannelSummary> {
        self.starred.iter().chain(&self.channels).chain(&self.dms)
    }
}

#[derive(Clone, Debug)]
struct Ack {
    ok: bool,
    result: Value,
    error: Option<String>,
}

const MAX_KEPT_ACKS: usize = 256;
const MAX_NOTICES: usize = 20;
const MAX_ALERTS: usize = 100;

/// One signed-in person's view of a workspace.
pub struct Session {
    pub state: State,
    pub me: Option<UserId>,
    pub view: View,
    pub drafts: BTreeMap<ChannelId, String>,
    pub thread_drafts: BTreeMap<MessageId, String>,
    pub search_query: String,
    pub search_hits: Vec<Hit>,
    pub notices: VecDeque<Notice>,
    pub alerts: VecDeque<Alert>,
    /// The window has focus: messages arriving in the open conversation are read.
    pub focused: bool,
    /// A fatal sign-in error from the hub.
    pub error: Option<String>,
    link: Box<dyn Transport>,
    next_req: u64,
    awaited: BTreeSet<u64>,
    acks: BTreeMap<u64, Ack>,
    /// The request `submit` is answering right now (its error goes to the caller, not a notice).
    inline_req: Option<u64>,
    /// Requests whose answer names a conversation to open (create, join, new DM) when it arrives.
    open_on_ack: BTreeSet<u64>,
}

impl Session {
    /// A session over `link`, signing in with `hello`.
    pub fn connect(mut link: Box<dyn Transport>, hello: ClientFrame) -> Session {
        link.send(hello);
        let mut s = Session {
            state: State::default(),
            me: None,
            view: View::default(),
            drafts: BTreeMap::new(),
            thread_drafts: BTreeMap::new(),
            search_query: String::new(),
            search_hits: Vec::new(),
            notices: VecDeque::new(),
            alerts: VecDeque::new(),
            focused: false,
            error: None,
            link,
            next_req: 1,
            awaited: BTreeSet::new(),
            acks: BTreeMap::new(),
            inline_req: None,
            open_on_ack: BTreeSet::new(),
        };
        s.pump();
        s
    }

    /// The demo workspace in-process, signed in as `handle` (default: the demo's own person).
    pub fn demo_as(handle: &str) -> Session {
        Session::connect(Box::new(Loopback::demo()), Loopback::hello(handle))
    }

    pub fn demo() -> Session {
        Session::demo_as(chatcraft_hub::sample::ME)
    }

    pub fn registry() -> &'static Registry {
        cmd::registry()
    }

    pub fn link_status(&self) -> LinkStatus {
        self.link.status()
    }

    pub fn link_label(&self) -> String {
        self.link.describe()
    }

    pub fn set_waker(&mut self, waker: Box<dyn Fn() + Send + Sync>) {
        self.link.set_waker(waker);
    }

    /// Runs a command by id. An escaped panic becomes an error (a safety net, not a licence).
    pub fn run(&mut self, id: &str, params: &Value) -> CmdResult {
        let spec = Self::registry().get(id).ok_or_else(|| CmdError::Unknown(id.to_string()))?;
        if let Some(why) = (spec.enabled)(self) {
            return Err(CmdError::Disabled(why.to_string()));
        }
        let run = spec.run;
        let empty = Value::Object(serde_json::Map::new());
        let params = if params.is_null() { &empty } else { params };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self, params))) {
            Ok(r) => r,
            Err(_) => Err(CmdError::Failed(format!("internal error in `{id}`; nothing was changed locally"))),
        }
    }

    /// Whether a command is enabled right now (for menus and buttons).
    pub fn enabled(&self, id: &str) -> bool {
        Self::registry().get(id).is_some_and(|s| (s.enabled)(self).is_none())
    }

    // ---- the link ---------------------------------------------------------------------------

    /// Sends an op. Returns the hub's result when it answers at once (in-process), else
    /// `{"pending": req}`; [`Session::wait_for`] collects a pending answer.
    pub fn submit(&mut self, op: Op) -> CmdResult {
        if self.me.is_none() {
            return Err(CmdError::Disabled("not signed in yet".into()));
        }
        let req = self.next_req;
        self.next_req = self.next_req.saturating_add(1);
        self.awaited.insert(req);
        self.link.send(ClientFrame::Op { req, op });
        self.inline_req = Some(req);
        self.pump();
        self.inline_req = None;
        match self.take_ack(req) {
            Some(r) => r,
            None => Ok(json!({ "pending": req })),
        }
    }

    /// Like [`Session::submit`], then opens the conversation named by the answer's `channel` —
    /// now if the answer is immediate, otherwise when it arrives.
    pub fn submit_and_open(&mut self, op: Op) -> CmdResult {
        let req = self.next_req;
        let r = self.submit(op)?;
        match r.get("channel").and_then(Value::as_u64) {
            Some(c) => self.open_channel(ChannelId(c)),
            None if r.get("pending").is_some() => {
                self.open_on_ack.insert(req);
            }
            None => {}
        }
        Ok(r)
    }

    fn take_ack(&mut self, req: u64) -> Option<CmdResult> {
        let a = self.acks.remove(&req)?;
        self.awaited.remove(&req);
        Some(if a.ok { Ok(a.result) } else { Err(CmdError::Failed(a.error.unwrap_or_else(|| "request failed".into()))) })
    }

    /// Waits for a pending request's answer (headless tools; never call from the UI thread).
    pub fn wait_for(&mut self, req: u64, timeout: Duration) -> Option<CmdResult> {
        let deadline = web_now_ms().saturating_add(i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX));
        loop {
            if let Some(r) = self.take_ack(req) {
                return Some(r);
            }
            let left = deadline.saturating_sub(web_now_ms());
            if left <= 0 {
                return None;
            }
            let frames = self.link.wait(Duration::from_millis(u64::try_from(left.min(250)).unwrap_or(250)));
            self.handle(frames);
        }
    }

    /// Waits until signed in (headless tools).
    pub fn wait_ready(&mut self, timeout: Duration) -> bool {
        let deadline = web_now_ms().saturating_add(i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX));
        while self.me.is_none() && self.error.is_none() && web_now_ms() < deadline {
            let frames = self.link.wait(Duration::from_millis(100));
            self.handle(frames);
        }
        self.me.is_some()
    }

    /// Waits up to `timeout` for something to arrive, then applies it (headless tools and
    /// tests; never call from the UI thread). Returns whether anything arrived.
    pub fn pump_wait(&mut self, timeout: Duration) -> bool {
        let frames = self.link.wait(timeout);
        let any = !frames.is_empty();
        self.handle(frames);
        any
    }

    /// Applies whatever has arrived. Call once per frame.
    pub fn pump(&mut self) -> bool {
        let frames = self.link.poll();
        let any = !frames.is_empty();
        self.handle(frames);
        any
    }

    fn handle(&mut self, frames: Vec<ServerFrame>) {
        let mut touched_open = false;
        for f in frames {
            match f {
                ServerFrame::Welcome { me, state } => {
                    self.me = Some(me);
                    self.replace_state(*state);
                    self.error = None;
                }
                ServerFrame::Resync { state } => self.replace_state(*state),
                ServerFrame::Event(s) => {
                    touched_open |= self.view.channel.is_some() && s.event.channel() == self.view.channel;
                    self.alert_for(&s.event);
                    self.state.apply(&s);
                    self.after_event(&s.event);
                }
                ServerFrame::Ack { req, ok, result, error } => {
                    if self.open_on_ack.remove(&req)
                        && ok
                        && let Some(c) = result.get("channel").and_then(Value::as_u64)
                    {
                        self.open_channel(ChannelId(c));
                    }
                    if !ok
                        && self.inline_req != Some(req)
                        && let Some(e) = &error
                    {
                        self.notice(e.clone(), true);
                    }
                    if self.awaited.contains(&req) {
                        self.acks.insert(req, Ack { ok, result, error });
                        while self.acks.len() > MAX_KEPT_ACKS {
                            let Some(first) = self.acks.keys().next().copied() else { break };
                            self.acks.remove(&first);
                            self.awaited.remove(&first);
                        }
                    }
                }
                ServerFrame::Error { message } => {
                    if self.me.is_none() {
                        self.error = Some(message.clone());
                    }
                    self.notice(message, true);
                }
                ServerFrame::Pong { .. } => {}
            }
        }
        if touched_open && self.focused {
            self.mark_open_read();
        }
    }

    fn replace_state(&mut self, mut state: State) {
        state.reindex();
        self.state = state;
        let visible = self.view.channel.is_some_and(|c| self.state.channel(c).is_some());
        if !visible {
            // The workspace's default channel, else the first conversation in the sidebar (Matrix
            // has no #general).
            self.view.channel = self.state.workspace.default_channel.filter(|c| self.state.channel(*c).is_some());
            if self.view.channel.is_none() {
                self.view.channel = self.sidebar().order().next().map(|c| c.id);
            }
            self.view.unread_from = self.view.channel.and_then(|c| self.first_unread(c));
        }
    }

    fn after_event(&mut self, e: &Event) {
        let Some(me) = self.me else { return };
        if let Event::MemberLeft { channel, user, .. } = e
            && *user == me
            && !self.state.can_see(me, *channel)
        {
            self.state.retain_visible(me);
            if self.view.channel == Some(*channel) {
                self.view.channel = self.state.workspace.default_channel;
                self.view.thread = None;
            }
        }
    }

    fn alert_for(&mut self, e: &Event) {
        let (Some(me), Event::MessagePosted { message: m }) = (self.me, e) else { return };
        if m.author == me {
            return;
        }
        let Some(c) = self.state.channel(m.channel) else { return };
        let muted = self.state.muted.get(&me).is_some_and(|s| s.contains(&c.id));
        let in_my_thread = m.thread.and_then(|t| self.state.message(t)).is_some_and(|r| r.author == me || r.repliers.contains(&me));
        let direct = c.kind.is_dm() || m.mentions.contains(&me) || (m.mentions_all && !muted) || in_my_thread;
        if !direct {
            return;
        }
        let who = self.state.user(m.author).map(|u| u.name().to_string()).unwrap_or_default();
        let title = if c.kind.is_dm() { who } else { format!("{who} in {}", self.state.title(c.id, Some(me))) };
        self.alerts.push_back(Alert { channel: m.channel, message: m.id, title, body: chatcraft_text::snippet(&m.text, 140) });
        while self.alerts.len() > MAX_ALERTS {
            self.alerts.pop_front();
        }
    }

    pub fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.notices.push_back(Notice { text: text.into(), error });
        while self.notices.len() > MAX_NOTICES {
            self.notices.pop_front();
        }
    }

    /// Marks the open conversation read up to its newest message.
    pub fn mark_open_read(&mut self) {
        let (Some(me), Some(c)) = (self.me, self.view.channel) else { return };
        let Some(latest) = self.state.latest(c) else { return };
        if latest > self.state.last_read(me, c) {
            let _ = self.submit(Op::MarkRead { channel: c, upto: latest });
        }
    }

    // ---- views ------------------------------------------------------------------------------

    /// The sidebar: starred, then channels by name, then direct messages by recent activity.
    pub fn sidebar(&self) -> Sidebar {
        let Some(me) = self.me else { return Sidebar::default() };
        let mut out = Sidebar::default();
        for s in self.state.summaries(me) {
            if s.archived {
                continue;
            }
            if s.starred {
                out.starred.push(s);
            } else if s.kind.is_dm() {
                out.dms.push(s);
            } else {
                out.channels.push(s);
            }
        }
        out.starred.sort_by(|a, b| a.title.cmp(&b.title));
        out.channels.sort_by(|a, b| a.title.cmp(&b.title));
        out.dms.sort_by_key(|c| std::cmp::Reverse(c.last_activity));
        out
    }

    /// The conversation a command should act on: `params.channel`, else the open one.
    pub fn target_channel(&self, params: &Value) -> Result<ChannelId, CmdError> {
        match crate::p::id_text(params, "channel") {
            Some(s) => self.state.resolve_channel(&s, self.me).ok_or_else(|| CmdError::Params(format!("no conversation `{s}`"))),
            None => self.view.channel.ok_or_else(|| CmdError::Params("`channel` is required (no conversation is open)".into())),
        }
    }

    pub fn resolve_user(&self, s: &str) -> Result<UserId, CmdError> {
        if s.trim().trim_start_matches('@') == "me" {
            return self.me.ok_or_else(|| CmdError::Disabled("not signed in".into()));
        }
        self.state.resolve_user(s).ok_or_else(|| CmdError::Params(format!("no person `{s}`")))
    }

    /// Opens a conversation, remembering where we came from.
    pub fn open_channel(&mut self, c: ChannelId) {
        if self.view.channel != Some(c) {
            if let Some(cur) = self.view.channel {
                self.view.back.push(cur);
                if self.view.back.len() > 100 {
                    self.view.back.remove(0);
                }
            }
            self.view.forward.clear();
            self.view.channel = Some(c);
            self.view.unread_from = self.first_unread(c);
            self.view.thread = None;
            if self.view.pane == Pane::Thread {
                self.view.pane = Pane::None;
            }
        }
        self.mark_open_read();
    }

    fn first_unread(&self, c: ChannelId) -> Option<MessageId> {
        let me = self.me?;
        let read = self.state.last_read(me, c);
        self.state.timeline(c).into_iter().find(|m| m.id > read && m.author != me).map(|m| m.id)
    }

    pub(crate) fn go_back(&mut self) -> bool {
        let Some(prev) = self.view.back.pop() else { return false };
        if let Some(cur) = self.view.channel {
            self.view.forward.push(cur);
        }
        self.view.channel = Some(prev);
        self.view.thread = None;
        self.view.unread_from = self.first_unread(prev);
        self.mark_open_read();
        true
    }

    pub(crate) fn go_forward(&mut self) -> bool {
        let Some(next) = self.view.forward.pop() else { return false };
        if let Some(cur) = self.view.channel {
            self.view.back.push(cur);
        }
        self.view.channel = Some(next);
        self.view.thread = None;
        self.view.unread_from = self.first_unread(next);
        self.mark_open_read();
        true
    }

    pub fn can_go_back(&self) -> bool {
        !self.view.back.is_empty()
    }
    pub fn can_go_forward(&self) -> bool {
        !self.view.forward.is_empty()
    }

    /// Your most recent editable message in a conversation (Up arrow in an empty composer).
    pub fn last_own_message(&self, c: ChannelId) -> Option<MessageId> {
        let me = self.me?;
        self.state.timeline(c).iter().rev().find(|m| m.author == me && !m.deleted).map(|m| m.id)
    }

    /// Whether the open conversation is a channel you could join but haven't.
    pub fn previewing(&self) -> bool {
        match (self.me, self.view.channel.and_then(|c| self.state.channel(c))) {
            (Some(me), Some(c)) => c.kind == ChannelKind::Public && !c.members.contains(&me),
            _ => false,
        }
    }

    /// A summary of the session for agents (`session.state`).
    pub fn describe(&self) -> Value {
        let me = self.me.and_then(|m| self.state.user(m));
        json!({
            "workspace": self.state.workspace.name,
            "me": me.map(|u| json!({"id": u.id, "handle": u.handle, "name": u.name()})),
            "link": { "label": self.link_label(), "status": format!("{:?}", self.link_status()) },
            "view": self.view,
            "channel": self.view.channel.map(|c| self.state.title(c, self.me)),
            "seq": self.state.seq,
        })
    }
}

fn web_now_ms() -> i64 {
    chatcraft_core::Ts::now().0
}
