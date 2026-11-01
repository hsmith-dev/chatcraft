//! The ChatCraft wire protocol (L2).
//!
//! A client opens a connection (WebSocket text frames, or an in-process loopback) and sends
//! [`ClientFrame::Hello`]. The server answers [`ServerFrame::Welcome`] with a snapshot of
//! everything that user may see. From then on the client sends [`Op`]s, each with a request id;
//! the server validates them, appends the resulting events to the workspace log, answers with an
//! [`ServerFrame::Ack`], and fans each event out to every connection allowed to see it. Clients
//! never mutate their replica directly: they apply the events they receive.
//!
//! Every frame is one JSON object with a `type` tag. Frames larger than
//! [`chatcraft_core::limits::MAX_FRAME_BYTES`] are rejected before parsing.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use chatcraft_core::limits::MAX_FRAME_BYTES;
use chatcraft_model::{ChannelId, MessageId, Presence, Sequenced, State, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped on incompatible changes. The server refuses other versions with a clear error.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request to change the workspace. The hub decides whether it is allowed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    PostMessage {
        channel: ChannelId,
        text: String,
        #[serde(default)]
        thread: Option<MessageId>,
        /// A thread reply also sent to the channel.
        #[serde(default)]
        broadcast: bool,
    },
    EditMessage {
        message: MessageId,
        text: String,
    },
    DeleteMessage {
        message: MessageId,
    },
    React {
        message: MessageId,
        emoji: String,
        on: bool,
    },
    Pin {
        message: MessageId,
        pinned: bool,
    },
    CreateChannel {
        name: String,
        #[serde(default)]
        private: bool,
        #[serde(default)]
        topic: String,
        #[serde(default)]
        purpose: String,
        /// Invited at creation.
        #[serde(default)]
        members: Vec<UserId>,
    },
    JoinChannel {
        channel: ChannelId,
    },
    LeaveChannel {
        channel: ChannelId,
    },
    Invite {
        channel: ChannelId,
        users: Vec<UserId>,
    },
    Remove {
        channel: ChannelId,
        user: UserId,
    },
    RenameChannel {
        channel: ChannelId,
        name: String,
    },
    SetTopic {
        channel: ChannelId,
        topic: String,
    },
    SetPurpose {
        channel: ChannelId,
        purpose: String,
    },
    Archive {
        channel: ChannelId,
        archived: bool,
    },
    /// Opens (or finds) the direct message with these people (the sender is added).
    OpenDm {
        users: Vec<UserId>,
    },
    SetProfile {
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        title: Option<String>,
    },
    SetStatus {
        text: String,
        emoji: String,
        /// Minutes until it clears; none keeps it until changed.
        #[serde(default)]
        minutes: Option<u32>,
    },
    SetPresence {
        presence: Presence,
    },
    MarkRead {
        channel: ChannelId,
        upto: MessageId,
    },
    Save {
        message: MessageId,
        saved: bool,
    },
    Mute {
        channel: ChannelId,
        muted: bool,
    },
    Star {
        channel: ChannelId,
        starred: bool,
    },
    RenameWorkspace {
        name: String,
    },
}

/// Client → server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientFrame {
    /// First frame on every connection.
    Hello {
        version: u32,
        /// The handle to sign in as; created on first sign-in when the token allows joining.
        handle: String,
        #[serde(default)]
        display_name: String,
        /// The workspace join token.
        token: String,
    },
    Op {
        req: u64,
        op: Op,
    },
    Ping {
        n: u64,
    },
}

/// Server → client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerFrame {
    Welcome {
        me: UserId,
        state: Box<State>,
    },
    Event(Sequenced),
    /// The client's view changed wholesale (it joined a private conversation): replace the replica.
    Resync {
        state: Box<State>,
    },
    Ack {
        req: u64,
        ok: bool,
        #[serde(default)]
        result: Value,
        #[serde(default)]
        error: Option<String>,
    },
    Error {
        message: String,
    },
    Pong {
        n: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    TooLarge(usize),
    Json(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::TooLarge(n) => write!(f, "frame of {n} bytes exceeds the {MAX_FRAME_BYTES}-byte limit"),
            FrameError::Json(e) => write!(f, "malformed frame: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Encodes a frame as one JSON text.
pub fn encode<T: Serialize>(frame: &T) -> String {
    serde_json::to_string(frame).unwrap_or_else(|e| format!("{{\"type\":\"error\",\"message\":\"encode: {e}\"}}"))
}

/// Decodes one JSON frame, refusing oversized input before parsing it.
pub fn decode<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, FrameError> {
    if text.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(text.len()));
    }
    serde_json::from_str(text).map_err(|e| FrameError::Json(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let f = ClientFrame::Op { req: 7, op: Op::PostMessage { channel: ChannelId(1), text: "hi".into(), thread: None, broadcast: false } };
        let s = encode(&f);
        assert!(s.contains("\"op\":\"postMessage\""), "{s}");
        assert_eq!(decode::<ClientFrame>(&s).unwrap(), f);
        let ack = ServerFrame::Ack { req: 7, ok: true, result: serde_json::json!({"message": 3}), error: None };
        assert_eq!(decode::<ServerFrame>(&encode(&ack)).unwrap(), ack);
    }

    #[test]
    fn hostile_frames_are_errors() {
        assert!(decode::<ClientFrame>("").is_err());
        assert!(decode::<ClientFrame>("{\"type\":\"op\",\"req\":-1}").is_err());
        assert!(matches!(decode::<ClientFrame>(&" ".repeat(MAX_FRAME_BYTES + 1)), Err(FrameError::TooLarge(_))));
        // Optional fields may be omitted.
        let f: ClientFrame = decode(r#"{"type":"op","req":1,"op":{"op":"postMessage","channel":2,"text":"x"}}"#).unwrap();
        assert!(matches!(f, ClientFrame::Op { op: Op::PostMessage { thread: None, .. }, .. }));
    }
}

#[cfg(test)]
mod welcome_tests {
    use super::*;

    #[test]
    fn welcome_with_id_keyed_maps_round_trips() {
        let mut state = State::default();
        state.users.insert(UserId(3), chatcraft_model::User { id: UserId(3), handle: "ada".into(), ..Default::default() });
        state.reads.entry(UserId(3)).or_default().insert(ChannelId(1), MessageId(2));
        let f = ServerFrame::Welcome { me: UserId(3), state: Box::new(state) };
        let back: ServerFrame = decode(&encode(&f)).unwrap();
        assert_eq!(back, f);
    }
}
