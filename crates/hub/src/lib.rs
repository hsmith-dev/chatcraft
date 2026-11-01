//! The hub (L3): the authority over one workspace.
//!
//! The hub owns the canonical [`State`], validates every [`Op`] against permissions and limits,
//! assigns ids and sequence numbers, and returns the resulting events. It does no I/O and reads
//! no clock (callers pass `now`), so the server, the in-process demo workspace and the tests all
//! run exactly the same code. Persisting events (`chatcraft-store`) and delivering them
//! (`chatcraft-net`) are the caller's job.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod ops;
pub mod sample;

use chatcraft_core::limits::MAX_LABEL;
use chatcraft_core::{ChannelId, MessageId, Ts, UserId, normalize_handle};
use chatcraft_model::{Channel, ChannelKind, Event, Presence, Role, Sequenced, State, User};
use chatcraft_proto::Op;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HubError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("not allowed: {0}")]
    Forbidden(String),
    #[error("invalid: {0}")]
    Invalid(String),
}

/// What an operation produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    pub events: Vec<Sequenced>,
    /// Returned to the requester (`{"message": 12}`).
    pub result: Value,
    /// Users whose view changed wholesale and need a fresh snapshot.
    pub resync: Vec<UserId>,
}

/// The workspace authority.
pub struct Hub {
    state: State,
    next_user: UserId,
    next_channel: ChannelId,
    next_message: MessageId,
    unjournaled: Vec<Sequenced>,
}

/// The id used for things the workspace itself does (creating `#general`).
pub const SYSTEM: UserId = UserId(0);

impl Hub {
    /// A new workspace with `#general` (everyone, can't be left) and `#random`.
    pub fn create(name: &str, now: Ts) -> Hub {
        let mut hub = Hub::replay(Vec::new());
        let name = clean_label(name).unwrap_or_else(|_| "Workspace".into());
        let name = if name.is_empty() { "Workspace".into() } else { name };
        let mut evs = vec![Event::WorkspaceCreated { name, at: now }];
        for (cname, purpose) in [
            ("general", "Announcements and work-wide conversation. Everyone is here."),
            ("random", "Everything that isn't work. Pets, plants, playlists."),
        ] {
            let id = hub.alloc_channel();
            evs.push(Event::ChannelCreated {
                channel: Channel {
                    id,
                    name: cname.into(),
                    kind: ChannelKind::Public,
                    purpose: purpose.into(),
                    created_by: SYSTEM,
                    created: now,
                    ..Channel::default()
                },
            });
            if cname == "general" {
                evs.push(Event::DefaultChannelSet { channel: id });
            }
        }
        hub.commit(evs);
        hub
    }

    /// Rebuilds a hub from its journal.
    pub fn replay(events: Vec<Sequenced>) -> Hub {
        let mut state = State::default();
        for s in &events {
            state.apply(s);
        }
        let (u, c, m) = state.max_ids();
        Hub { state, next_user: u.next(), next_channel: c.next(), next_message: m.next(), unjournaled: Vec::new() }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn seq(&self) -> u64 {
        self.state.seq
    }

    /// Events not yet handed to the journal (the caller persists them before acknowledging).
    pub fn take_unjournaled(&mut self) -> Vec<Sequenced> {
        std::mem::take(&mut self.unjournaled)
    }

    /// What `user` may see, as a snapshot for a Welcome or Resync frame.
    pub fn snapshot_for(&self, user: UserId) -> State {
        self.state.visible_to(user)
    }

    /// Signs in by handle, creating the account on first sign-in (the first person becomes the
    /// owner) and joining the default channels. Marks them active.
    pub fn sign_in(&mut self, handle: &str, display_name: &str, now: Ts) -> Result<(UserId, Outcome), HubError> {
        let handle = normalize_handle(handle).map_err(|e| HubError::Invalid(format!("handle: {e}")))?;
        let display = clean_label(display_name)?;
        if let Some(u) = self.state.user_by_handle(&handle) {
            if u.deactivated {
                return Err(HubError::Forbidden(format!("@{handle} has been deactivated")));
            }
            let id = u.id;
            let evs = if u.presence == Presence::Active { Vec::new() } else { vec![Event::PresenceChanged { user: id, presence: Presence::Active }] };
            return Ok((id, self.outcome(evs, Value::Null)));
        }
        let id = self.alloc_user();
        let role = if self.state.users.values().any(|u| !u.bot) { Role::Member } else { Role::Owner };
        let user = User {
            id,
            display_name: if display.is_empty() { handle.clone() } else { display },
            handle,
            role,
            presence: Presence::Active,
            color: u8::try_from(id.0 % 12).unwrap_or(0),
            joined: now,
            ..User::default()
        };
        let mut evs = vec![Event::UserJoined { user }];
        for c in self.state.channels.values().filter(|c| c.kind == ChannelKind::Public && (c.name == "general" || c.name == "random")) {
            evs.push(Event::MemberJoined { channel: c.id, user: id, by: None });
        }
        Ok((id, self.outcome(evs, Value::Null)))
    }

    /// Adds a bot account (integrations, the demo's build bot).
    pub fn add_bot(&mut self, handle: &str, display_name: &str, now: Ts) -> Result<UserId, HubError> {
        let (id, _) = self.sign_in(handle, display_name, now)?;
        if let Some(u) = self.state.user(id).cloned()
            && !u.bot
        {
            let mut u = u;
            u.bot = true;
            u.role = Role::Member;
            // Re-announce with the bot flag; UserJoined replaces the record.
            self.commit(vec![Event::UserJoined { user: u }]);
        }
        Ok(id)
    }

    /// Marks someone offline (their last connection closed).
    pub fn sign_out(&mut self, user: UserId) -> Outcome {
        let evs = match self.state.user(user) {
            Some(u) if u.presence != Presence::Offline => vec![Event::PresenceChanged { user, presence: Presence::Offline }],
            _ => Vec::new(),
        };
        self.outcome(evs, Value::Null)
    }

    /// Validates and applies one operation by `actor`.
    pub fn apply(&mut self, actor: UserId, op: Op, now: Ts) -> Result<Outcome, HubError> {
        match self.state.user(actor) {
            None => return Err(HubError::NotFound(format!("user {actor}"))),
            Some(u) if u.deactivated => return Err(HubError::Forbidden("this account is deactivated".into())),
            Some(_) => {}
        }
        let (events, result, resync) = ops::plan(self, actor, op, now)?;
        let mut out = self.outcome(events, result);
        out.resync = resync;
        Ok(out)
    }

    fn outcome(&mut self, events: Vec<Event>, result: Value) -> Outcome {
        Outcome { events: self.commit(events), result, resync: Vec::new() }
    }

    fn commit(&mut self, events: Vec<Event>) -> Vec<Sequenced> {
        let mut out = Vec::with_capacity(events.len());
        for event in events {
            let s = Sequenced { seq: self.state.seq.saturating_add(1), event };
            self.state.apply(&s);
            out.push(s);
        }
        self.unjournaled.extend(out.iter().cloned());
        out
    }

    fn alloc_user(&mut self) -> UserId {
        let id = self.next_user;
        self.next_user = id.next();
        id
    }
    fn alloc_channel(&mut self) -> ChannelId {
        let id = self.next_channel;
        self.next_channel = id.next();
        id
    }
    fn alloc_message(&mut self) -> MessageId {
        let id = self.next_message;
        self.next_message = id.next();
        id
    }
}

/// Trims and bounds a short label (names, titles, topics).
pub(crate) fn clean_label(s: &str) -> Result<String, HubError> {
    let s: String = s.trim().chars().filter(|c| !c.is_control()).collect();
    if s.chars().count() > MAX_LABEL {
        return Err(HubError::Invalid(format!("longer than {MAX_LABEL} characters")));
    }
    Ok(s)
}

#[cfg(test)]
mod tests;
