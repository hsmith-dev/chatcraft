//! The ChatCraft data model (L1).
//!
//! The workspace is **event-sourced**: [`State`] is what you get by applying a sequence of
//! [`Event`]s with [`State::apply`]. The hub validates requests and turns them into events; the
//! server journals them; every client replays the same events into its own replica. `apply` is
//! total — an event naming something that doesn't exist is ignored — so a corrupt or reordered log
//! can degrade a replica but never crash it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod entities;
mod event;
mod state;

pub use chatcraft_core::{ChannelId, MessageId, Ts, UserId};
pub use entities::{Channel, ChannelKind, Message, Presence, Role, Status, User, Workspace};
pub use event::{Event, Sequenced};
pub use state::{ChannelSummary, State};

#[cfg(test)]
mod tests;
