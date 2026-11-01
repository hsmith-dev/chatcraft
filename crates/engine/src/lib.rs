//! ChatCraft engine (L4): the client side of a workspace.
//!
//! **Everything is a command.** Each user-visible action is a [`CommandSpec`] with a stable id
//! (`message.send`, `channel.create`, `nav.nextUnread`), a label, where it lives in the UI, a
//! default shortcut, a params description and a `run` function. The UI, keyboard shortcuts, the
//! quick switcher, the CLI and the MCP server all call [`Session::run`].
//!
//! A [`Session`] keeps a replica of what the signed-in person may see, rebuilt only from the
//! events the hub sends, plus local view state (open conversation, thread, drafts, search). It
//! talks to a hub through a [`Transport`]: [`Loopback`] runs a hub in-process (demo and offline
//! use, tests, the browser build); `chatcraft-net` provides the WebSocket client.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod catalog;
pub mod cmd;
mod session;
mod transport;

use std::collections::HashMap;

use serde_json::Value;

pub use chatcraft_core as core;
pub use chatcraft_hub as hub;
pub use chatcraft_model as model;
pub use chatcraft_proto as proto;
pub use chatcraft_search as search;
pub use chatcraft_text as text;
pub use session::{Alert, Notice, Pane, Session, Sidebar, View};
pub use transport::{LinkStatus, Loopback, Transport};

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum CmdError {
    #[error("unknown command `{0}`")]
    Unknown(String),
    #[error("bad parameters: {0}")]
    Params(String),
    #[error("not available: {0}")]
    Disabled(String),
    #[error("{0}")]
    Failed(String),
}

pub type CmdResult = Result<Value, CmdError>;

/// A registered command.
#[derive(Clone)]
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Where it lives in the UI: `Message › More actions`, `Channel › Settings`, `Sidebar`…
    pub location: &'static str,
    /// Default shortcut in portable form (`Mod+K`, `Alt+Shift+Down`). `Mod` = Ctrl, or ⌘ on macOS.
    pub shortcut: &'static str,
    /// Parameter description for agents (`{"channel": "#name", "text": string}`).
    pub params: &'static str,
    /// Changes the workspace (sends an op), as opposed to reading or navigating.
    pub mutates: bool,
    /// `None` = enabled; `Some(reason)` = disabled.
    pub enabled: fn(&Session) -> Option<&'static str>,
    pub run: fn(&mut Session, &Value) -> CmdResult,
}

fn always(_: &Session) -> Option<&'static str> {
    None
}

impl CommandSpec {
    pub const fn new(id: &'static str, label: &'static str, location: &'static str, run: fn(&mut Session, &Value) -> CmdResult) -> Self {
        CommandSpec { id, label, location, shortcut: "", params: "{}", mutates: true, enabled: always, run }
    }
    pub const fn key(mut self, s: &'static str) -> Self {
        self.shortcut = s;
        self
    }
    pub const fn params(mut self, p: &'static str) -> Self {
        self.params = p;
        self
    }
    /// Reads or navigates; never changes the workspace.
    pub const fn pure(mut self) -> Self {
        self.mutates = false;
        self
    }
    pub const fn when(mut self, f: fn(&Session) -> Option<&'static str>) -> Self {
        self.enabled = f;
        self
    }
}

/// All commands by id.
pub struct Registry {
    specs: Vec<CommandSpec>,
    by_id: HashMap<&'static str, usize>,
}

impl Registry {
    pub fn new(specs: Vec<CommandSpec>) -> Self {
        let by_id = specs.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
        Registry { specs, by_id }
    }
    pub fn get(&self, id: &str) -> Option<&CommandSpec> {
        self.by_id.get(id).and_then(|i| self.specs.get(*i))
    }
    pub fn all(&self) -> &[CommandSpec] {
        &self.specs
    }
    /// The command list as JSON (`commands` / MCP `list_commands`).
    pub fn describe(&self) -> Value {
        Value::Array(
            self.specs
                .iter()
                .map(|s| serde_json::json!({"id": s.id, "label": s.label, "location": s.location, "shortcut": s.shortcut, "params": s.params, "mutates": s.mutates}))
                .collect(),
        )
    }
    /// The command bound to a shortcut, if any (`"Mod+K"`, case-insensitive).
    pub fn by_shortcut(&self, key: &str) -> Option<&CommandSpec> {
        self.specs.iter().find(|s| !s.shortcut.is_empty() && s.shortcut.split(" / ").any(|k| k.eq_ignore_ascii_case(key)))
    }
}

/// Parameter helpers. Params come from people, scripts and agents: everything is optional-typed.
pub mod p {
    use chatcraft_core::MessageId;
    use serde_json::Value;

    use crate::CmdError;

    pub fn str<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
        v.get(k).and_then(Value::as_str)
    }
    pub fn req_str<'a>(v: &'a Value, k: &str) -> Result<&'a str, CmdError> {
        str(v, k).ok_or_else(|| CmdError::Params(format!("`{k}` (string) is required")))
    }
    pub fn bool(v: &Value, k: &str) -> Option<bool> {
        v.get(k).and_then(Value::as_bool)
    }
    pub fn u64(v: &Value, k: &str) -> Option<u64> {
        v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_f64().filter(|f| *f >= 0.0 && f.is_finite()).map(|f| f as u64)))
    }
    /// An id given as a number or as text (`"M12"`).
    pub fn id_text(v: &Value, k: &str) -> Option<String> {
        match v.get(k)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }
    pub fn message(v: &Value, k: &str) -> Option<MessageId> {
        id_text(v, k).and_then(|s| MessageId::parse(&s))
    }
    pub fn req_message(v: &Value, k: &str) -> Result<MessageId, CmdError> {
        message(v, k).ok_or_else(|| CmdError::Params(format!("`{k}` (message id, e.g. 12 or \"M12\") is required")))
    }
    /// A list of strings, or one string, under `k`.
    pub fn list(v: &Value, k: &str) -> Vec<String> {
        match v.get(k) {
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string).or_else(|| x.as_u64().map(|n| n.to_string()))).collect(),
            Some(Value::String(s)) => s.split([',', ' ']).filter(|x| !x.is_empty()).map(str::to_string).collect(),
            Some(Value::Number(n)) => vec![n.to_string()],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
