//! ChatCraft networking (L5, native only).
//!
//! - [`server`]: the workspace server. One [`chatcraft_hub::Hub`] behind a mutex, its events
//!   journaled with `chatcraft-store` before each request is acknowledged, fanned out to every
//!   WebSocket connection that may see them.
//! - [`client`]: [`NetTransport`], a [`chatcraft_engine::Transport`] over WebSocket that runs on
//!   its own thread, reconnects with backoff, and signs in again after every reconnect (the
//!   server answers with a fresh snapshot, so the replica heals itself).
//!
//! Transport security: the server speaks plain `ws://` and binds to localhost by default. Put
//! it behind a TLS-terminating reverse proxy (`wss://`) to expose it; native TLS is a tracked gap.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod client;
pub mod server;

pub use client::NetTransport;
pub use server::{Server, ServerOptions};
