//! The workspace server.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use chatcraft_core::limits::MAX_FRAME_BYTES;
use chatcraft_core::{Ts, UserId};
use chatcraft_hub::{Hub, Outcome};
use chatcraft_model::Sequenced;
use chatcraft_proto::{ClientFrame, PROTOCOL_VERSION, ServerFrame, decode, encode};
use chatcraft_store::{FileJournal, Journal, NullJournal};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub struct ServerOptions {
    pub bind: SocketAddr,
    /// Where the journal lives; `None` keeps everything in memory.
    pub data_dir: Option<PathBuf>,
    /// The join token clients present in Hello.
    pub token: String,
    /// Name for a new workspace.
    pub workspace_name: String,
    /// Seed a new workspace with the demo team.
    pub demo: bool,
}

#[derive(Clone, Debug)]
enum Fanout {
    Events(Arc<Vec<Sequenced>>),
    Resync(UserId),
}

struct Shared {
    hub: Mutex<Hub>,
    journal: Mutex<Box<dyn Journal>>,
    tx: broadcast::Sender<Fanout>,
    online: Mutex<HashMap<UserId, usize>>,
    token: String,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock means a panic elsewhere; the data is still the best we have.
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Shared {
    /// Journals what the hub produced and fans it out. Returns an error if the journal failed
    /// (the events are live in memory but not durable: the operator must look).
    fn publish(&self, hub: &mut Hub, out: &Outcome) -> Result<(), String> {
        let pending = hub.take_unjournaled();
        let r = if pending.is_empty() { Ok(()) } else { lock(&self.journal).append(&pending).map_err(|e| e.to_string()) };
        if let Err(e) = &r {
            log::error!("journal append failed: {e}");
        }
        if !out.events.is_empty() {
            let _ = self.tx.send(Fanout::Events(Arc::new(out.events.clone())));
        }
        for u in &out.resync {
            let _ = self.tx.send(Fanout::Resync(*u));
        }
        r
    }
}

/// A running (or ready-to-run) server.
pub struct Server {
    shared: Arc<Shared>,
    pub warnings: Vec<String>,
    bind: SocketAddr,
}

/// Compares secrets without an early exit on the first differing byte.
fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A random URL-safe token.
pub fn random_token() -> String {
    let mut b = [0u8; 18];
    if getrandom::fill(&mut b).is_err() {
        // No OS randomness: fall back to the clock (still unique per run; operators can set one).
        let t = Ts::now().0.to_le_bytes();
        for (i, x) in b.iter_mut().enumerate() {
            *x = t.get(i % 8).copied().unwrap_or(0) ^ (i as u8).wrapping_mul(97);
        }
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Server {
    /// Opens (or creates) the workspace.
    pub fn open(opts: ServerOptions) -> Result<Server, String> {
        let now = Ts::now();
        let (hub, journal, warnings): (Hub, Box<dyn Journal>, Vec<String>) = match &opts.data_dir {
            Some(dir) => {
                let (mut j, loaded) = FileJournal::open(dir).map_err(|e| e.to_string())?;
                let mut hub = if loaded.events.is_empty() {
                    if opts.demo { chatcraft_hub::sample::demo(now).map_err(|e| e.to_string())? } else { Hub::create(&opts.workspace_name, now) }
                } else {
                    Hub::replay(loaded.events)
                };
                // A new workspace journals its whole founding history; a replayed one has none.
                let founding = hub.take_unjournaled();
                if !founding.is_empty() {
                    j.append(&founding).map_err(|e| e.to_string())?;
                }
                (hub, Box::new(j), loaded.warnings)
            }
            None => {
                let mut hub =
                    if opts.demo { chatcraft_hub::sample::demo(now).map_err(|e| e.to_string())? } else { Hub::create(&opts.workspace_name, now) };
                let _ = hub.take_unjournaled();
                (hub, Box::new(NullJournal), Vec::new())
            }
        };
        let (tx, _) = broadcast::channel(1024);
        let shared = Shared { hub: Mutex::new(hub), journal: Mutex::new(journal), tx, online: Mutex::new(HashMap::new()), token: opts.token };
        Ok(Server { shared: Arc::new(shared), warnings, bind: opts.bind })
    }

    pub fn workspace_name(&self) -> String {
        lock(&self.shared.hub).state().workspace.name.clone()
    }

    /// Serves until the process ends. Calls `ready` with the bound address.
    pub async fn serve(self, ready: impl FnOnce(SocketAddr)) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.bind).await?;
        ready(listener.local_addr()?);
        loop {
            let (stream, peer) = listener.accept().await?;
            let shared = Arc::clone(&self.shared);
            tokio::spawn(async move {
                if let Err(e) = connection(shared, stream).await {
                    log::info!("{peer}: {e}");
                }
            });
        }
    }

    /// Runs the server on a new multi-threaded runtime (blocking).
    pub fn run(self, ready: impl FnOnce(SocketAddr)) -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(self.serve(ready)).map_err(|e| e.to_string())
    }
}

type Ws = tokio_tungstenite::WebSocketStream<TcpStream>;

async fn send(ws: &mut Ws, f: &ServerFrame) -> Result<(), String> {
    ws.send(Message::Text(encode(f).into())).await.map_err(|e| e.to_string())
}

async fn connection(shared: Arc<Shared>, stream: TcpStream) -> Result<(), String> {
    let config = WebSocketConfig::default().max_message_size(Some(MAX_FRAME_BYTES)).max_frame_size(Some(MAX_FRAME_BYTES));
    let mut ws = tokio_tungstenite::accept_async_with_config(stream, Some(config)).await.map_err(|e| e.to_string())?;

    // 1. Hello.
    let first = tokio::time::timeout(std::time::Duration::from_secs(15), ws.next()).await.map_err(|_| "no hello within 15s".to_string())?;
    let hello = match first {
        Some(Ok(Message::Text(t))) => decode::<ClientFrame>(t.as_str()).map_err(|e| e.to_string())?,
        _ => return Err("expected a hello frame".into()),
    };
    let ClientFrame::Hello { version, handle, display_name, token } = hello else {
        send(&mut ws, &ServerFrame::Error { message: "the first frame must be hello".into() }).await?;
        return Err("first frame was not hello".into());
    };
    if version != PROTOCOL_VERSION {
        send(&mut ws, &ServerFrame::Error { message: format!("protocol {version} is not supported (server speaks {PROTOCOL_VERSION})") }).await?;
        return Err("version mismatch".into());
    }
    if !same_secret(&token, &shared.token) {
        send(&mut ws, &ServerFrame::Error { message: "the workspace token is wrong".into() }).await?;
        return Err("bad token".into());
    }
    // Subscribe before taking the snapshot so nothing falls between them.
    let mut rx = shared.tx.subscribe();
    let signed = {
        let mut hub = lock(&shared.hub);
        hub.sign_in(&handle, &display_name, Ts::now()).map(|(me, out)| {
            let _ = shared.publish(&mut hub, &out);
            (me, hub.seq(), hub.snapshot_for(me))
        })
    };
    let (me, welcome_seq, snapshot) = match signed {
        Ok(x) => x,
        Err(e) => {
            send(&mut ws, &ServerFrame::Error { message: e.to_string() }).await?;
            return Err(e.to_string());
        }
    };
    *lock(&shared.online).entry(me).or_default() += 1;
    let r = session(&shared, &mut ws, &mut rx, me, welcome_seq, snapshot).await;

    // Last connection gone: offline.
    let last = {
        let mut online = lock(&shared.online);
        let n = online.entry(me).or_default();
        *n = n.saturating_sub(1);
        *n == 0
    };
    if last {
        let mut hub = lock(&shared.hub);
        let out = hub.sign_out(me);
        let _ = shared.publish(&mut hub, &out);
    }
    r
}

async fn session(
    shared: &Shared,
    ws: &mut Ws,
    rx: &mut broadcast::Receiver<Fanout>,
    me: UserId,
    mut last_seq: u64,
    snapshot: chatcraft_model::State,
) -> Result<(), String> {
    send(ws, &ServerFrame::Welcome { me, state: Box::new(snapshot) }).await?;
    loop {
        tokio::select! {
            msg = ws.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.to_string()),
                };
                // The requester's own events go out before its Ack, so its replica is current
                // when the answer arrives; the broadcast copies are then skipped by `last_seq`.
                let mut own: Vec<ServerFrame> = Vec::new();
                let reply = match decode::<ClientFrame>(text.as_str()) {
                    Ok(ClientFrame::Op { req, op }) => {
                        let mut hub = lock(&shared.hub);
                        match hub.apply(me, op, Ts::now()) {
                            Ok(out) => {
                                let state = hub.state();
                                own.extend(out.events.iter().filter(|s| s.seq > last_seq && state.event_visible(&s.event, me)).cloned().map(ServerFrame::Event));
                                if out.resync.contains(&me) {
                                    own.push(ServerFrame::Resync { state: Box::new(hub.snapshot_for(me)) });
                                }
                                last_seq = last_seq.max(out.events.last().map_or(0, |s| s.seq));
                                match shared.publish(&mut hub, &out) {
                                    Ok(()) => ServerFrame::Ack { req, ok: true, result: out.result, error: None },
                                    Err(e) => ServerFrame::Ack { req, ok: false, result: Value::Null, error: Some(format!("saved in memory but not on disk: {e}")) },
                                }
                            }
                            Err(e) => ServerFrame::Ack { req, ok: false, result: Value::Null, error: Some(e.to_string()) },
                        }
                    }
                    Ok(ClientFrame::Ping { n }) => ServerFrame::Pong { n },
                    Ok(ClientFrame::Hello { .. }) => ServerFrame::Error { message: "already signed in".into() },
                    Err(e) => ServerFrame::Error { message: e.to_string() },
                };
                for f in &own {
                    send(ws, f).await?;
                }
                send(ws, &reply).await?;
            }
            item = rx.recv() => {
                let frames: Vec<ServerFrame> = match item {
                    Ok(Fanout::Events(evs)) => {
                        let hub = lock(&shared.hub);
                        let state = hub.state();
                        evs.iter()
                            .filter(|s| s.seq > last_seq && state.event_visible(&s.event, me))
                            .cloned()
                            .map(ServerFrame::Event)
                            .collect()
                    }
                    Ok(Fanout::Resync(u)) if u == me => vec![ServerFrame::Resync { state: Box::new(lock(&shared.hub).snapshot_for(me)) }],
                    Ok(Fanout::Resync(_)) => Vec::new(),
                    // Fell behind: start over from a snapshot.
                    Err(broadcast::error::RecvError::Lagged(_)) => vec![ServerFrame::Resync { state: Box::new(lock(&shared.hub).snapshot_for(me)) }],
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                };
                for f in frames {
                    if let ServerFrame::Event(s) = &f {
                        last_seq = last_seq.max(s.seq);
                    }
                    send(ws, &f).await?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_compare() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
        assert_eq!(random_token().len(), 36);
        assert_ne!(random_token(), random_token());
    }

    #[test]
    fn a_new_workspace_is_journaled_whole_and_replays() {
        let dir = std::env::temp_dir().join(format!("chatcraft-net-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let opts = |demo| ServerOptions {
            bind: ([127, 0, 0, 1], 0).into(),
            data_dir: Some(dir.clone()),
            token: "t".into(),
            workspace_name: "W".into(),
            demo,
        };
        let first = Server::open(opts(true)).unwrap();
        let (n, seq) = {
            let h = lock(&first.shared.hub);
            (h.state().messages.len(), h.seq())
        };
        drop(first);
        let again = Server::open(opts(false)).unwrap();
        let h = lock(&again.shared.hub);
        assert_eq!(h.state().messages.len(), n);
        assert_eq!(h.seq(), seq);
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
        drop(h);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
