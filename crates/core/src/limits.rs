//! Hard limits. Everything that arrives from a client, a file or an agent is hostile: these caps
//! bound allocations and keep one request from stalling the hub.

/// Longest message body, in bytes (Slack caps around 40k characters; we cap bytes).
pub const MAX_MESSAGE_BYTES: usize = 40_000;
/// Longest channel name, in characters (lowercase, `a-z 0-9 - _`).
pub const MAX_CHANNEL_NAME: usize = 80;
/// Longest user handle, in characters.
pub const MAX_HANDLE: usize = 40;
/// Longest display name, title, topic or purpose, in characters.
pub const MAX_LABEL: usize = 250;
/// Longest status text, in characters.
pub const MAX_STATUS: usize = 100;
/// Most distinct reactions on one message.
pub const MAX_REACTIONS_PER_MESSAGE: usize = 50;
/// Longest emoji shortcode, in bytes (`:shortcode:` without colons).
pub const MAX_EMOJI_NAME: usize = 64;
/// Most members a group direct message may have (including the opener).
pub const MAX_GROUP_DM: usize = 9;
/// Most results a search returns.
pub const MAX_SEARCH_RESULTS: usize = 200;
/// Largest wire frame accepted, in bytes.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
