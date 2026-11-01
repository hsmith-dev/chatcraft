//! The WebSocket client transport.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chatcraft_engine::{LinkStatus, Transport};
use chatcraft_proto::{ClientFrame, ServerFrame, decode, encode};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc as tmpsc;
use tokio_tungstenite::tungstenite::Message;

type Waker = Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>;

/// A transport to `ws://host:port`, on its own thread.
pub struct NetTransport {
    url: String,
    out: tmpsc::UnboundedSender<ClientFrame>,
    inbox: mpsc::Receiver<ServerFrame>,
    status: Arc<Mutex<LinkStatus>>,
    waker: Waker,
}

fn set(status: &Mutex<LinkStatus>, s: LinkStatus) {
    if let Ok(mut g) = status.lock() {
        *g = s;
    }
}

fn wake(w: &Waker) {
    if let Ok(g) = w.lock()
        && let Some(f) = g.as_ref()
    {
        f();
    }
}

impl NetTransport {
    /// Starts connecting in the background. The first frame sent must be the Hello; it is sent
    /// again after every reconnect.
    pub fn connect(url: &str) -> Result<NetTransport, String> {
        let url = normalize(url);
        let (out, out_rx) = tmpsc::unbounded_channel();
        let (in_tx, inbox) = mpsc::channel();
        let status = Arc::new(Mutex::new(LinkStatus::Connecting));
        let waker: Waker = Arc::new(Mutex::new(None));
        let (u, st, wk) = (url.clone(), Arc::clone(&status), Arc::clone(&waker));
        std::thread::Builder::new()
            .name("chatcraft-net".into())
            .spawn(move || match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt.block_on(run(u, out_rx, in_tx, st, wk)),
                Err(e) => set(&st, LinkStatus::Offline(format!("runtime: {e}"))),
            })
            .map_err(|e| e.to_string())?;
        Ok(NetTransport { url, out, inbox, status, waker })
    }
}

/// `host:port` → `ws://host:port`.
fn normalize(url: &str) -> String {
    let u = url.trim();
    if u.starts_with("ws://") || u.starts_with("wss://") { u.to_string() } else { format!("ws://{u}") }
}

async fn run(
    url: String,
    mut out_rx: tmpsc::UnboundedReceiver<ClientFrame>,
    in_tx: mpsc::Sender<ServerFrame>,
    status: Arc<Mutex<LinkStatus>>,
    waker: Waker,
) {
    let mut hello: Option<String> = None;
    let mut queued: VecDeque<String> = VecDeque::new();
    let mut backoff = Duration::from_millis(500);
    loop {
        while hello.is_none() {
            match out_rx.recv().await {
                Some(f @ ClientFrame::Hello { .. }) => hello = Some(encode(&f)),
                Some(f) => queued.push_back(encode(&f)),
                None => return,
            }
        }
        set(&status, LinkStatus::Connecting);
        wake(&waker);
        match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok((ws, _)) => {
                backoff = Duration::from_millis(500);
                let (mut w, mut r) = ws.split();
                let mut ok = w.send(Message::Text(hello.clone().unwrap_or_default().into())).await.is_ok();
                while ok && let Some(q) = queued.pop_front() {
                    ok = w.send(Message::Text(q.into())).await.is_ok();
                }
                if ok {
                    set(&status, LinkStatus::Online);
                    wake(&waker);
                }
                while ok {
                    tokio::select! {
                        f = out_rx.recv() => match f {
                            Some(f) => {
                                let text = encode(&f);
                                if matches!(f, ClientFrame::Hello { .. }) {
                                    hello = Some(text.clone());
                                }
                                if w.send(Message::Text(text.clone().into())).await.is_err() {
                                    queued.push_back(text);
                                    ok = false;
                                }
                            }
                            None => return,
                        },
                        m = r.next() => match m {
                            Some(Ok(Message::Text(t))) => {
                                match decode::<ServerFrame>(t.as_str()) {
                                    Ok(f) => {
                                        if in_tx.send(f).is_err() {
                                            return;
                                        }
                                    }
                                    // Never drop a frame silently: the session shows it.
                                    Err(e) => {
                                        log::warn!("{url}: {e}");
                                        let _ = in_tx.send(ServerFrame::Error { message: format!("unreadable frame from the server: {e}") });
                                    }
                                }
                                wake(&waker);
                            }
                            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => ok = false,
                            Some(Ok(_)) => {}
                        },
                    }
                }
                set(&status, LinkStatus::Offline("connection lost; reconnecting".into()));
            }
            Err(e) => set(&status, LinkStatus::Offline(format!("{e}; retrying"))),
        }
        wake(&waker);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

impl Transport for NetTransport {
    fn send(&mut self, frame: ClientFrame) {
        if self.out.send(frame).is_err() {
            set(&self.status, LinkStatus::Offline("network thread stopped".into()));
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
        self.url.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chatcraft_engine::Session;
    use chatcraft_proto::PROTOCOL_VERSION;
    use serde_json::json;

    use super::*;
    use crate::server::{Server, ServerOptions};

    fn hello(handle: &str, token: &str) -> ClientFrame {
        ClientFrame::Hello { version: PROTOCOL_VERSION, handle: handle.into(), display_name: String::new(), token: token.into() }
    }

    /// Two people on a real server over real sockets: messages, private channels, wrong tokens.
    #[test]
    fn end_to_end_over_websocket() {
        let server = Server::open(ServerOptions {
            bind: ([127, 0, 0, 1], 0).into(),
            data_dir: None,
            token: "tok".into(),
            workspace_name: "E2E".into(),
            demo: false,
        })
        .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = server.run(move |addr| {
                let _ = tx.send(addr);
            });
        });
        let addr = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let url = format!("ws://{addr}");
        let t = Duration::from_secs(10);

        let mut ada = Session::connect(Box::new(NetTransport::connect(&url).unwrap()), hello("ada", "tok"));
        assert!(ada.wait_ready(t), "{:?} {:?}", ada.link_status(), ada.error);
        let mut bo = Session::connect(Box::new(NetTransport::connect(&url).unwrap()), hello("bo", "tok"));
        assert!(bo.wait_ready(t));

        let pending = ada.run("message.send", &json!({"channel": "#general", "text": "hi @bo"})).unwrap();
        let req = pending["pending"].as_u64().unwrap();
        let r = ada.wait_for(req, t).unwrap().unwrap();
        let m = chatcraft_core::MessageId(r["message"].as_u64().unwrap());
        // Bo receives it.
        let deadline = std::time::Instant::now() + t;
        while bo.state.message(m).is_none() && std::time::Instant::now() < deadline {
            bo.pump();
            std::thread::sleep(Duration::from_millis(20));
        }
        let general = bo.state.workspace.default_channel.unwrap();
        assert_eq!(bo.state.unread(bo.me.unwrap(), general), (1, 1));
        assert!(bo.alerts.iter().any(|a| a.message == m));

        // A private channel Bo isn't in never reaches him.
        let pending = ada.run("channel.create", &json!({"name": "secret", "private": true})).unwrap();
        let r = ada.wait_for(pending["pending"].as_u64().unwrap(), t).unwrap().unwrap();
        let secret = chatcraft_core::ChannelId(r["channel"].as_u64().unwrap());
        // The requester's replica already has it when the answer arrives (events precede the Ack).
        assert!(ada.state.channel(secret).is_some(), "own events arrive before the ack");
        assert_eq!(ada.state.resolve_channel("#secret", ada.me), Some(secret));
        std::thread::sleep(Duration::from_millis(200));
        bo.pump();
        assert!(bo.state.channel(secret).is_none());

        // A wrong token is refused.
        let mut eve = Session::connect(Box::new(NetTransport::connect(&url).unwrap()), hello("eve", "nope"));
        assert!(!eve.wait_ready(Duration::from_secs(3)));
        assert!(eve.error.as_deref().unwrap_or_default().contains("token"));
    }
}
