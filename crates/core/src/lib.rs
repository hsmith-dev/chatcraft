//! ChatCraft foundation (L0): typed ids, timestamps, limits and name validation.
//!
//! Nothing here does I/O. Every other crate builds on these types, so they stay small, `Copy`
//! where possible, and total: parsing returns `Option`/`Result`, never panics.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod ids;
pub mod limits;
mod time;
mod validate;

pub use ids::{ChannelId, MessageId, UserId};
pub use time::Ts;
pub use validate::{NameError, normalize_channel_name, normalize_handle};
