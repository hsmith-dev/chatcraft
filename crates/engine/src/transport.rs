//! The link between a session and a hub.

use std::collections::VecDeque;
use std::time::Duration;

use chatcraft_core::{Ts, UserId};
use chatcraft_hub::Hub;
use chatcraft_proto::{ClientFrame, PROTOCOL_VERSION, ServerFrame};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkStatus {
    Connecting,
    Online,
    /// Disconnected, with the reason (the transport may be reconnecting).
    Offline(String),
}

/// Carries frames to and from a hub. Implementations never block in `send` or `poll`.
pub trait Transport: Send {
    fn send(&mut self, frame: ClientFrame);
    /// Frames that have arrived since the last call.
    fn poll(&mut self) -> Vec<ServerFrame>;
    /// Like `poll`, but waits up to `timeout` for the first frame (headless tools only).
    fn wait(&mut self, _timeout: Duration) -> Vec<ServerFrame> {
        self.poll()
    }
    fn status(&self) -> LinkStatus;
    /// Called from the network thread when frames arrive (the UI requests a repaint).
    fn set_waker(&mut self, _waker: Box<dyn Fn() + Send + Sync>) {}
    /// A short description for the status bar (`demo workspace`, `ws://host:7900`).
    fn describe(&self) -> String;
    /// The replica starts with recent history only; older pages load on demand.
    fn pages_history(&self) -> bool {
        false
    }
}

/// A hub in the same process: the demo workspace, offline use, tests and the browser build.
pub struct Loopback {
    hub: Hub,
    me: Option<UserId>,
    queue: VecDeque<ServerFrame>,
    label: String,
}

impl Loopback {
    pub fn new(mut hub: Hub, label: &str) -> Self {
        // Nothing is persisted in-process.
        let _ = hub.take_unjournaled();
        Loopback { hub, me: None, queue: VecDeque::new(), label: label.into() }
    }

    /// The demo workspace (see `chatcraft_hub::sample`).
    pub fn demo() -> Self {
        let hub = chatcraft_hub::sample::demo(Ts::now()).unwrap_or_else(|_| Hub::create("Demo", Ts::now()));
        Loopback::new(hub, "demo workspace")
    }

    pub fn hub(&self) -> &Hub {
        &self.hub
    }

    /// The hello a session sends to sign in as `handle` on a loopback (no token needed).
    pub fn hello(handle: &str) -> ClientFrame {
        ClientFrame::Hello { version: PROTOCOL_VERSION, handle: handle.into(), display_name: String::new(), token: String::new() }
    }

    fn fan_out(&mut self, me: UserId, out: chatcraft_hub::Outcome) {
        for s in out.events {
            if self.hub.state().event_visible(&s.event, me) {
                self.queue.push_back(ServerFrame::Event(s));
            }
        }
        if out.resync.contains(&me) {
            self.queue.push_back(ServerFrame::Resync { state: Box::new(self.hub.snapshot_for(me)) });
        }
    }
}

impl Transport for Loopback {
    fn send(&mut self, frame: ClientFrame) {
        let now = Ts::now();
        match frame {
            ClientFrame::Hello { handle, display_name, .. } => match self.hub.sign_in(&handle, &display_name, now) {
                Ok((me, _)) => {
                    self.me = Some(me);
                    self.queue.push_back(ServerFrame::Welcome { me, state: Box::new(self.hub.snapshot_for(me)) });
                }
                Err(e) => self.queue.push_back(ServerFrame::Error { message: e.to_string() }),
            },
            ClientFrame::Op { req, op } => {
                let Some(me) = self.me else {
                    self.queue.push_back(ServerFrame::Ack { req, ok: false, result: Value::Null, error: Some("not signed in".into()) });
                    return;
                };
                match self.hub.apply(me, op, now) {
                    Ok(out) => {
                        let result = out.result.clone();
                        self.fan_out(me, out);
                        self.queue.push_back(ServerFrame::Ack { req, ok: true, result, error: None });
                    }
                    Err(e) => self.queue.push_back(ServerFrame::Ack { req, ok: false, result: Value::Null, error: Some(e.to_string()) }),
                }
                // Nothing is persisted in-process.
                let _ = self.hub.take_unjournaled();
            }
            ClientFrame::Ping { n } => self.queue.push_back(ServerFrame::Pong { n }),
        }
    }

    fn poll(&mut self) -> Vec<ServerFrame> {
        self.queue.drain(..).collect()
    }

    fn status(&self) -> LinkStatus {
        if self.me.is_some() { LinkStatus::Online } else { LinkStatus::Connecting }
    }

    fn describe(&self) -> String {
        self.label.clone()
    }
}
