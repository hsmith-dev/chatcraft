//! ChatCraft over Matrix (L5, native).
//!
//! [`MatrixTransport`] is a [`chatcraft_engine::Transport`] that speaks the Matrix client-server
//! API through `matrix-rust-sdk`, so ChatCraft works with any Matrix homeserver and interoperates
//! with other Matrix clients. Rooms become conversations, events become ChatCraft events
//! ([`mapper`]), and ChatCraft ops become Matrix requests.
//!
//! **End-to-end encryption.** Olm/Megolm through the SDK (`vodozemac`). Private channels and
//! direct messages are created encrypted; messages are decrypted on this device only. The
//! device's keys live in an encrypted SQLite store (`<store>/store`, passphrase in
//! `<store>/passphrase`, mode 0600) and the signed-in session in `<store>/session.json`, so the
//! same device — and its keys — is used on every launch. Device verification, cross-signing and
//! key backup are tracked in `docs/gaps.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod mapper;
mod worker;

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chatcraft_engine::proto::{ClientFrame, ServerFrame};
use chatcraft_engine::{LinkStatus, Transport};
use tokio::sync::mpsc as tmpsc;

/// How to reach and sign in to a homeserver.
#[derive(Clone, Debug)]
pub struct MatrixConfig {
    /// `https://matrix.example.org` or a server name (`example.org`, discovered via well-known).
    pub homeserver: String,
    /// `alice` or `@alice:example.org`.
    pub user: String,
    /// Used only when there is no saved session for this store.
    pub password: String,
    /// Where this device's session, keys and cache live (one directory per account).
    pub store_dir: PathBuf,
    pub device_name: String,
    /// Accept room invitations automatically (people added to a channel are in it, as in Slack).
    pub auto_join_invites: bool,
}

impl MatrixConfig {
    pub fn new(homeserver: &str, user: &str, password: &str, store_dir: PathBuf) -> MatrixConfig {
        MatrixConfig {
            homeserver: homeserver.into(),
            user: user.into(),
            password: password.into(),
            store_dir,
            device_name: "ChatCraft".into(),
            auto_join_invites: true,
        }
    }
}

/// Where an account's device data lives by default: the platform's per-user data folder,
/// `ChatCraft/matrix/<user>@<homeserver>`.
pub fn default_store_dir(homeserver: &str, user: &str) -> PathBuf {
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/ChatCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("ChatCraft"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(|d| PathBuf::from(d).join("chatcraft"))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/chatcraft")))
    }
    .unwrap_or_else(|| PathBuf::from(".chatcraft"));
    let host = homeserver.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
    let safe: String =
        format!("{}@{host}", user.trim_start_matches('@')).chars().map(|c| if c.is_alphanumeric() || "@.-_".contains(c) { c } else { '_' }).collect();
    base.join("matrix").join(safe)
}

pub(crate) type Waker = Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>;

pub(crate) fn set_status(status: &Mutex<LinkStatus>, s: LinkStatus) {
    if let Ok(mut g) = status.lock() {
        *g = s;
    }
}

pub(crate) fn wake(w: &Waker) {
    if let Ok(g) = w.lock()
        && let Some(f) = g.as_ref()
    {
        f();
    }
}

/// A Matrix account as a ChatCraft transport (runs on its own thread).
pub struct MatrixTransport {
    label: String,
    out: tmpsc::UnboundedSender<ClientFrame>,
    inbox: mpsc::Receiver<ServerFrame>,
    status: Arc<Mutex<LinkStatus>>,
    waker: Waker,
}

impl MatrixTransport {
    /// Starts signing in in the background; the session's Hello frame starts the work.
    pub fn connect(cfg: MatrixConfig) -> Result<MatrixTransport, String> {
        let label = format!("{} on {}", cfg.user, cfg.homeserver);
        let (out, out_rx) = tmpsc::unbounded_channel();
        let (in_tx, inbox) = mpsc::channel();
        let status = Arc::new(Mutex::new(LinkStatus::Connecting));
        let waker: Waker = Arc::new(Mutex::new(None));
        let (st, wk) = (Arc::clone(&status), Arc::clone(&waker));
        std::thread::Builder::new()
            .name("chatcraft-matrix".into())
            .spawn(move || match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(rt) => rt.block_on(worker::run(cfg, out_rx, in_tx, st, wk)),
                Err(e) => set_status(&st, LinkStatus::Offline(format!("runtime: {e}"))),
            })
            .map_err(|e| e.to_string())?;
        Ok(MatrixTransport { label, out, inbox, status, waker })
    }
}

impl Transport for MatrixTransport {
    fn send(&mut self, frame: ClientFrame) {
        if self.out.send(frame).is_err() {
            set_status(&self.status, LinkStatus::Offline("Matrix worker stopped".into()));
        }
    }

    fn poll(&mut self) -> Vec<ServerFrame> {
        self.inbox.try_iter().collect()
    }

    fn wait(&mut self, timeout: Duration) -> Vec<ServerFrame> {
        match self.inbox.recv_timeout(timeout) {
            Ok(first) => std::iter::once(first).chain(self.inbox.try_iter()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn status(&self) -> LinkStatus {
        self.status.lock().map(|g| g.clone()).unwrap_or(LinkStatus::Offline("status unavailable".into()))
    }

    fn set_waker(&mut self, waker: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut g) = self.waker.lock() {
            *g = Some(waker);
        }
    }

    fn describe(&self) -> String {
        self.label.clone()
    }
}
