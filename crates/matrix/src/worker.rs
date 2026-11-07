//! The Matrix worker: sign in, sync, and carry out ops, on the transport's runtime.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chatcraft_core::{ChannelId, MessageId, Ts};
use chatcraft_engine::LinkStatus;
use chatcraft_engine::proto::{ClientFrame, Op, ServerFrame};
use chatcraft_model::Presence;
use chatcraft_text::emoji;
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::room::Receipts;
use matrix_sdk::ruma::api::client::filter::{FilterDefinition, RoomEventFilter, RoomFilter};
use matrix_sdk::ruma::api::client::presence::set_presence;
use matrix_sdk::ruma::api::client::room::{Visibility, create_room};
use matrix_sdk::ruma::events::InitialStateEvent;
use matrix_sdk::ruma::events::room::encryption::RoomEncryptionEventContent;
use matrix_sdk::ruma::events::room::join_rules::JoinRule;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::{EventId, OwnedUserId, RoomId, UInt, UserId};
use matrix_sdk::sync::SyncResponse;
use matrix_sdk::{Client, Room, RoomMemberships};
use serde_json::{Value, json};
use tokio::sync::mpsc as tmpsc;

use crate::mapper::{Mapper, RoomInfo, format_status};
use crate::{MatrixConfig, Waker, set_status, wake};

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Reads a secret file, or creates it with a random value (mode 0600 on Unix).
fn secret_file(path: &std::path::Path) -> R<String> {
    if let Ok(s) = std::fs::read_to_string(path)
        && !s.trim().is_empty()
    {
        return Ok(s.trim().to_string());
    }
    let mut b = [0u8; 32];
    // OS randomness only: this secret protects the device's encryption keys at rest.
    getrandom::fill(&mut b).map_err(|e| format!("no secure randomness available: {e}"))?;
    let s: String = b.iter().map(|x| format!("{x:02x}")).collect();
    write_private(path, &s)?;
    Ok(s)
}

fn write_private(path: &std::path::Path, contents: &str) -> R<()> {
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Signs in, reusing this store's saved device when there is one (so its keys stay valid).
async fn sign_in(cfg: &MatrixConfig) -> R<Client> {
    std::fs::create_dir_all(&cfg.store_dir).map_err(|e| format!("{}: {e}", cfg.store_dir.display()))?;
    let pass = secret_file(&cfg.store_dir.join("passphrase"))?;
    let client = Client::builder()
        .server_name_or_homeserver_url(&cfg.homeserver)
        .sqlite_store(cfg.store_dir.join("store"), Some(&pass))
        .build()
        .await
        .map_err(err)?;
    let session_path = cfg.store_dir.join("session.json");
    if let Ok(text) = std::fs::read_to_string(&session_path)
        && let Ok(session) = serde_json::from_str::<MatrixSession>(&text)
    {
        match client.restore_session(session).await {
            Ok(()) => return Ok(client),
            Err(e) => log::warn!("saved session rejected ({e}); signing in again"),
        }
    }
    if cfg.password.is_empty() {
        return Err("no saved session for this account; a password is needed".into());
    }
    client.matrix_auth().login_username(&cfg.user, &cfg.password).initial_device_display_name(&cfg.device_name).await.map_err(err)?;
    if let Some(session) = client.matrix_auth().session() {
        write_private(&session_path, &serde_json::to_string(&session).map_err(err)?)?;
    }
    Ok(client)
}

/// The SDK's view of a room, as the mapper wants it.
async fn room_info(room: &Room) -> RoomInfo {
    let name = match room.name().filter(|n| !n.trim().is_empty()) {
        Some(n) => n,
        None => room.display_name().await.map(|d| d.to_string()).unwrap_or_default(),
    };
    let members = room
        .members(RoomMemberships::JOIN | RoomMemberships::INVITE)
        .await
        .map(|ms| ms.iter().map(|m| (m.user_id().to_string(), m.display_name().map(str::to_string))).collect())
        .unwrap_or_default();
    RoomInfo {
        room_id: room.room_id().to_string(),
        name,
        topic: room.topic().unwrap_or_default(),
        public: matches!(room.join_rule(), Some(JoinRule::Public)),
        direct: room.is_direct().await.unwrap_or(false),
        encrypted: room.encryption_state().is_encrypted(),
        favourite: room.is_favourite(),
        members,
    }
}

fn sync_settings() -> SyncSettings {
    let mut timeline = RoomEventFilter::default();
    timeline.limit = Some(UInt::from(100u32));
    let mut room = RoomFilter::default();
    room.timeline = timeline;
    let mut filter = FilterDefinition::default();
    filter.room = room;
    SyncSettings::default().filter(filter.into()).timeout(Duration::from_secs(30))
}

struct Worker {
    client: Client,
    map: Mapper,
    me: OwnedUserId,
    cfg: MatrixConfig,
    /// Problems the person should hear about (failed joins…), sent as error frames.
    problems: Vec<String>,
    /// Rooms this device marked as DMs, before `m.direct` comes back through sync.
    direct: std::collections::HashSet<String>,
}

/// How many recent events of each conversation the welcome snapshot carries.
const HISTORY: u32 = 100;

impl Worker {
    async fn info(&self, room: &Room) -> RoomInfo {
        let mut info = room_info(room).await;
        info.direct |= self.direct.contains(&info.room_id);
        info
    }

    /// The welcome snapshot: every joined room from the SDK's store, with its recent history
    /// fetched (and decrypted) explicitly, so it doesn't depend on what the first sync returned.
    async fn snapshot(&mut self) {
        for room in self.client.joined_rooms() {
            let info = self.info(&room).await;
            self.map.upsert_room(&info);
            let mut opts = matrix_sdk::room::MessagesOptions::backward();
            opts.limit = UInt::from(HISTORY);
            match room.messages(opts).await {
                Ok(page) => {
                    for ev in page.chunk.iter().rev() {
                        self.timeline_event(room.room_id().as_str(), ev);
                    }
                }
                Err(e) => log::warn!("history of {}: {e}", room.room_id()),
            }
        }
    }

    fn timeline_event(&mut self, room_id: &str, ev: &matrix_sdk::deserialized_responses::TimelineEvent) {
        let Ok(json) = serde_json::from_str::<Value>(ev.raw().json().get()) else { return };
        let utd = json.get("type").and_then(Value::as_str) == Some("m.room.encrypted");
        self.map.timeline(room_id, &json, ev.encryption_info().is_some() || utd, utd);
    }

    async fn absorb(&mut self, r: &SyncResponse) {
        self.accept_invites(r).await;
        self.absorb_rooms(r).await;
    }

    /// Accepts every waiting invitation in the SDK's store (not only the ones this sync
    /// mentions: the store skips invites it already knew). An invite marked `is_direct` is
    /// recorded as a DM.
    async fn accept_invites(&mut self, r: &SyncResponse) {
        if !self.cfg.auto_join_invites {
            return;
        }
        let me = self.me.as_str();
        let flagged: std::collections::HashSet<String> = r
            .rooms
            .invited
            .iter()
            .filter(|(_, invite)| {
                invite.invite_state.events.iter().filter_map(|e| serde_json::from_str::<Value>(e.json().get()).ok()).any(|e| {
                    e.get("type").and_then(Value::as_str) == Some("m.room.member")
                        && e.get("state_key").and_then(Value::as_str) == Some(me)
                        && e.get("content").and_then(|c| c.get("is_direct")).and_then(Value::as_bool) == Some(true)
                })
            })
            .map(|(rid, _)| rid.to_string())
            .collect();
        for room in self.client.invited_rooms() {
            let rid = room.room_id().to_owned();
            let direct = flagged.contains(rid.as_str()) || room.is_direct().await.unwrap_or(false);
            match self.client.join_room_by_id(&rid).await {
                Ok(joined) if direct => {
                    self.direct.insert(rid.to_string());
                    if let Err(e) = joined.set_is_direct(true).await {
                        log::warn!("marking {rid} as a DM: {e}");
                    }
                }
                Ok(_) => {}
                Err(e) => self.problems.push(format!("couldn't accept the invitation to {rid}: {e}")),
            }
        }
    }

    async fn absorb_rooms(&mut self, r: &SyncResponse) {
        for (rid, up) in &r.rooms.joined {
            if let Some(room) = self.client.get_room(rid) {
                let info = self.info(&room).await;
                self.map.upsert_room(&info);
            }
            for ev in &up.timeline.events {
                self.timeline_event(rid.as_str(), ev);
            }
            for ad in &up.account_data {
                let Ok(json) = serde_json::from_str::<Value>(ad.json().get()) else { continue };
                if json.get("type").and_then(Value::as_str) == Some("m.fully_read")
                    && let Some(e) = json.get("content").and_then(|c| c.get("event_id")).and_then(Value::as_str)
                {
                    self.map.fully_read(rid.as_str(), e);
                }
            }
        }
        for rid in r.rooms.left.keys() {
            self.map.left_room(rid.as_str());
        }
        for p in &r.presence {
            let Ok(json) = serde_json::from_str::<Value>(p.json().get()) else { continue };
            let (Some(who), Some(c)) = (json.get("sender").and_then(Value::as_str), json.get("content")) else { continue };
            self.map.presence(who, c.get("presence").and_then(Value::as_str).unwrap_or("offline"), c.get("status_msg").and_then(Value::as_str));
        }
    }

    fn room(&self, c: ChannelId) -> R<Room> {
        let rid = self.map.room_id(c).ok_or_else(|| format!("no conversation {c}"))?;
        let rid = RoomId::parse(rid).map_err(err)?;
        self.client.get_room(&rid).ok_or_else(|| format!("not in conversation {c}"))
    }

    fn event_id(&self, m: MessageId) -> R<matrix_sdk::ruma::OwnedEventId> {
        let e = self.map.event_id(m).ok_or_else(|| format!("no message {m}"))?;
        EventId::parse(e).map_err(err)
    }

    fn mxid(&self, u: chatcraft_core::UserId) -> R<OwnedUserId> {
        let s = self.map.mxid(u).ok_or_else(|| format!("no person {u}"))?;
        UserId::parse(s).map_err(err)
    }

    /// Sends a message-like event and feeds it back through the mapper at once (local echo).
    async fn send(&mut self, room: &Room, kind: &str, content: Value) -> R<String> {
        let res = room.send_raw(kind, content.clone()).await.map_err(err)?;
        let event_id = res.response.event_id.to_string();
        let echo = json!({ "type": kind, "event_id": event_id, "sender": self.me.as_str(), "origin_server_ts": Ts::now().0, "content": content });
        self.map.timeline(room.room_id().as_str(), &echo, room.encryption_state().is_encrypted(), false);
        Ok(event_id)
    }

    async fn redact(&mut self, room: &Room, target: &str) -> R<()> {
        let eid = EventId::parse(target).map_err(err)?;
        let res = room.redact(&eid, None, None).await.map_err(err)?;
        let echo = json!({ "type": "m.room.redaction", "event_id": res.event_id.to_string(), "sender": self.me.as_str(), "redacts": target, "content": { "redacts": target } });
        self.map.timeline(room.room_id().as_str(), &echo, false, false);
        Ok(())
    }

    /// Message content: Markdown → HTML for other clients, plus intentional mentions.
    fn message_content(&self, text: &str) -> R<Value> {
        let mut content = serde_json::to_value(RoomMessageEventContent::text_markdown(text)).map_err(err)?;
        let ids: Vec<String> = chatcraft_text::mentions(text)
            .iter()
            .filter_map(|h| self.map.resolve_mention(h))
            .filter_map(|u| self.map.mxid(u))
            .map(str::to_string)
            .collect();
        let room = chatcraft_text::broadcasts(text);
        if let Some(o) = content.as_object_mut()
            && (!ids.is_empty() || room)
        {
            o.insert("m.mentions".into(), json!({ "user_ids": ids, "room": room }));
        }
        Ok(content)
    }

    async fn create_room(&mut self, name: Option<String>, topic: String, private: bool, direct: bool, invite: Vec<OwnedUserId>) -> R<ChannelId> {
        let mut req = create_room::v3::Request::new();
        req.name = name.clone();
        req.topic = (!topic.is_empty()).then(|| topic.clone());
        req.invite = invite.clone();
        req.is_direct = direct;
        req.preset = Some(if direct {
            create_room::v3::RoomPreset::TrustedPrivateChat
        } else if private {
            create_room::v3::RoomPreset::PrivateChat
        } else {
            create_room::v3::RoomPreset::PublicChat
        });
        if !private && !direct {
            req.visibility = Visibility::Public;
        }
        let encrypted = private || direct;
        if encrypted {
            req.initial_state = vec![InitialStateEvent::with_empty_state_key(RoomEncryptionEventContent::with_recommended_defaults()).to_raw_any()];
        }
        let room = self.client.create_room(req).await.map_err(err)?;
        if direct {
            self.direct.insert(room.room_id().to_string());
            // Matrix keeps DMs in each person's `m.direct` list; record it for us.
            if let Err(e) = room.set_is_direct(true).await {
                log::warn!("marking {} as a DM: {e}", room.room_id());
            }
        }
        let mut members = vec![(self.me.to_string(), None)];
        members.extend(invite.iter().map(|u| (u.to_string(), None)));
        let info = RoomInfo {
            room_id: room.room_id().to_string(),
            name: name.unwrap_or_default(),
            topic,
            public: !encrypted,
            direct,
            encrypted,
            favourite: false,
            members,
        };
        Ok(self.map.upsert_room(&info))
    }

    #[allow(clippy::cognitive_complexity)]
    async fn op(&mut self, op: Op) -> R<Value> {
        match op {
            Op::PostMessage { channel, text, thread, .. } => {
                let room = self.room(channel)?;
                let mut content = self.message_content(&text)?;
                if let Some(root) = thread {
                    let root = self.event_id(root)?.to_string();
                    if let Some(o) = content.as_object_mut() {
                        o.insert(
                            "m.relates_to".into(),
                            json!({ "rel_type": "m.thread", "event_id": root, "is_falling_back": true, "m.in_reply_to": { "event_id": root } }),
                        );
                    }
                }
                let eid = self.send(&room, "m.room.message", content).await?;
                Ok(json!({ "message": self.map.message_for(&eid), "channel": channel }))
            }
            Op::EditMessage { message, text } => {
                let target = self.event_id(message)?.to_string();
                let room = self.room(self.map.state().message(message).map(|m| m.channel).unwrap_or_default())?;
                let new_content = self.message_content(&text)?;
                let content = json!({ "msgtype": "m.text", "body": format!("* {text}"), "m.new_content": new_content, "m.relates_to": { "rel_type": "m.replace", "event_id": target } });
                self.send(&room, "m.room.message", content).await?;
                Ok(Value::Null)
            }
            Op::DeleteMessage { message } => {
                let target = self.event_id(message)?.to_string();
                let room = self.room(self.map.state().message(message).map(|m| m.channel).unwrap_or_default())?;
                self.redact(&room, &target).await?;
                Ok(Value::Null)
            }
            Op::React { message, emoji: name, on } => {
                let target = self.event_id(message)?.to_string();
                let room = self.room(self.map.state().message(message).map(|m| m.channel).unwrap_or_default())?;
                let name = name.trim_matches(':').to_string();
                let me = self.map.me().unwrap_or_default();
                if on {
                    let key = emoji::lookup(&name).unwrap_or(&name).to_string();
                    let content = json!({ "m.relates_to": { "rel_type": "m.annotation", "event_id": target, "key": key } });
                    self.send(&room, "m.reaction", content).await?;
                } else if let Some(r) = self.map.reaction_event(message, &name, me).map(str::to_string) {
                    self.redact(&room, &r).await?;
                }
                Ok(Value::Null)
            }
            Op::CreateChannel { name, private, topic, purpose, members, external } => {
                let mut invite = members.iter().map(|u| self.mxid(*u)).collect::<R<Vec<_>>>()?;
                invite.extend(parse_mxids(&external)?);
                let topic = if topic.is_empty() { purpose } else { topic };
                let c = self.create_room(Some(name), topic, private, false, invite).await?;
                Ok(json!({ "channel": c }))
            }
            Op::OpenDm { users, external } => {
                let me = self.map.me().unwrap_or_default();
                let others: Vec<_> = users.into_iter().filter(|u| *u != me).collect();
                let existing = self.map.state().dm_with(&std::iter::once(me).chain(others.iter().copied()).collect());
                if let (Some(c), true) = (existing, external.is_empty()) {
                    return Ok(json!({ "channel": c }));
                }
                let mut invite = others.iter().map(|u| self.mxid(*u)).collect::<R<Vec<_>>>()?;
                invite.extend(parse_mxids(&external)?);
                let c = self.create_room(None, String::new(), true, true, invite).await?;
                Ok(json!({ "channel": c }))
            }
            Op::JoinChannel { channel } => {
                let rid = self.map.room_id(channel).ok_or_else(|| format!("no conversation {channel}"))?;
                let rid = RoomId::parse(rid).map_err(err)?;
                let room = self.client.join_room_by_id(&rid).await.map_err(err)?;
                let info = self.info(&room).await;
                Ok(json!({ "channel": self.map.upsert_room(&info) }))
            }
            Op::LeaveChannel { channel } => {
                let room = self.room(channel)?;
                room.leave().await.map_err(err)?;
                self.map.left_room(room.room_id().as_str());
                Ok(Value::Null)
            }
            Op::Invite { channel, users, external } => {
                let room = self.room(channel)?;
                let mut ids = users.iter().map(|u| self.mxid(*u)).collect::<R<Vec<_>>>()?;
                ids.extend(parse_mxids(&external)?);
                for u in ids {
                    room.invite_user_by_id(&u).await.map_err(err)?;
                }
                Ok(Value::Null)
            }
            Op::Remove { channel, user } => {
                self.room(channel)?.kick_user(&self.mxid(user)?, None).await.map_err(err)?;
                Ok(Value::Null)
            }
            Op::RenameChannel { channel, name } => {
                self.room(channel)?.set_name(name).await.map_err(err)?;
                Ok(Value::Null)
            }
            Op::SetTopic { channel, topic } | Op::SetPurpose { channel, purpose: topic } => {
                self.room(channel)?.set_room_topic(&topic).await.map_err(err)?;
                Ok(Value::Null)
            }
            Op::MarkRead { channel, upto } => {
                let room = self.room(channel)?;
                let eid = self.event_id(upto)?;
                room.send_multiple_receipts(Receipts::new().fully_read_marker(eid.clone()).public_read_receipt(eid.clone())).await.map_err(err)?;
                self.map.fully_read(room.room_id().as_str(), eid.as_str());
                Ok(Value::Null)
            }
            Op::Star { channel, starred } => {
                self.room(channel)?.set_is_favourite(starred, None).await.map_err(err)?;
                self.map.starred(channel, starred);
                Ok(Value::Null)
            }
            Op::SetProfile { display_name, .. } => {
                if let Some(n) = display_name {
                    self.client.account().set_display_name(Some(&n)).await.map_err(err)?;
                }
                Ok(Value::Null)
            }
            Op::SetPresence { presence } => {
                let state = match presence {
                    Presence::Active => PresenceState::Online,
                    Presence::Away | Presence::Dnd => PresenceState::Unavailable,
                    Presence::Offline => PresenceState::Offline,
                };
                self.client.send(set_presence::v3::Request::new(self.me.clone(), state)).await.map_err(err)?;
                Ok(Value::Null)
            }
            Op::SetStatus { text, emoji: e, .. } => {
                let mut req = set_presence::v3::Request::new(self.me.clone(), PresenceState::Online);
                req.status_msg = Some(format_status(&text, &e));
                self.client.send(req).await.map_err(err)?;
                Ok(Value::Null)
            }
            Op::Archive { .. } => Err("archiving isn't available on Matrix yet".into()),
            Op::Pin { .. } => Err("pinning isn't available on Matrix yet".into()),
            Op::Save { .. } => Err("saved items aren't available on Matrix yet".into()),
            Op::Mute { .. } => Err("muting isn't available on Matrix yet".into()),
            Op::RenameWorkspace { .. } => Err("a Matrix homeserver has no workspace name to change".into()),
        }
    }
}

fn parse_mxids(list: &[String]) -> R<Vec<OwnedUserId>> {
    list.iter().map(|s| UserId::parse(s.as_str()).map_err(|e| format!("`{s}` is not a Matrix ID: {e}"))).collect()
}

fn send_frames(in_tx: &mpsc::Sender<ServerFrame>, waker: &Waker, frames: Vec<ServerFrame>) -> bool {
    for f in frames {
        if in_tx.send(f).is_err() {
            return false;
        }
    }
    wake(waker);
    true
}

pub(crate) async fn run(
    cfg: MatrixConfig,
    mut out_rx: tmpsc::UnboundedReceiver<ClientFrame>,
    in_tx: mpsc::Sender<ServerFrame>,
    status: Arc<Mutex<LinkStatus>>,
    waker: Waker,
) {
    // The session's Hello starts the sign-in.
    loop {
        match out_rx.recv().await {
            Some(ClientFrame::Hello { .. }) => break,
            Some(_) => {}
            None => return,
        }
    }
    let client = match sign_in(&cfg).await {
        Ok(c) => c,
        Err(e) => {
            set_status(&status, LinkStatus::Offline(format!("sign-in failed: {e}")));
            let _ = send_frames(&in_tx, &waker, vec![ServerFrame::Error { message: format!("Matrix sign-in failed: {e}") }]);
            return;
        }
    };
    let Some(me) = client.user_id().map(ToOwned::to_owned) else { return };
    let mut w =
        Worker { map: Mapper::new(me.as_str()), client: client.clone(), me, cfg, problems: Vec::new(), direct: std::collections::HashSet::new() };

    // First sync from scratch (not from a saved token) so the snapshot carries recent history,
    // then the welcome; later syncs continue from this one's token.
    let first = sync_settings().timeout(Duration::from_secs(0)).token(matrix_sdk::config::SyncToken::NoToken).full_state(true);
    match client.sync_once(first).await {
        Ok(r) => {
            // Homeservers may answer a repeated initial sync from a cache (Synapse does, for a
            // couple of minutes), so catch up with one immediate incremental sync before the
            // snapshot.
            let catch_up = client.sync_once(sync_settings().timeout(Duration::from_secs(0))).await.ok();
            // Accept waiting invitations first, so they are in the snapshot.
            w.accept_invites(&r).await;
            if let Some(c) = &catch_up {
                w.accept_invites(c).await;
            }
            w.snapshot().await;
            w.absorb_rooms(&r).await;
            if let Some(c) = &catch_up {
                w.absorb_rooms(c).await;
            }
        }
        Err(e) => {
            set_status(&status, LinkStatus::Offline(format!("sync failed: {e}")));
            let _ = send_frames(&in_tx, &waker, vec![ServerFrame::Error { message: format!("Matrix sync failed: {e}") }]);
            return;
        }
    }
    let _ = w.map.take();
    let me_id = w.map.me().unwrap_or_default();
    set_status(&status, LinkStatus::Online);
    let mut hello = vec![ServerFrame::Welcome { me: me_id, state: Box::new(w.map.state().clone()) }];
    hello.extend(w.problems.drain(..).map(|message| ServerFrame::Error { message }));
    if !send_frames(&in_tx, &waker, hello) {
        return;
    }

    // Syncs run on their own task; their results come back here, where the mapper lives.
    let (sync_tx, mut sync_rx) = tmpsc::unbounded_channel::<R<SyncResponse>>();
    let sync_client = client.clone();
    let status2 = Arc::clone(&status);
    let waker2 = Arc::clone(&waker);
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            match sync_client.sync_once(sync_settings()).await {
                Ok(r) => {
                    backoff = Duration::from_secs(1);
                    set_status(&status2, LinkStatus::Online);
                    if sync_tx.send(Ok(r)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    set_status(&status2, LinkStatus::Offline(format!("{e}; retrying")));
                    wake(&waker2);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        }
    });

    loop {
        tokio::select! {
            f = out_rx.recv() => {
                let frames = match f {
                    Some(ClientFrame::Op { req, op }) => {
                        let r = w.op(op).await;
                        let mut frames: Vec<ServerFrame> = w.map.take().into_iter().map(ServerFrame::Event).collect();
                        frames.push(match r {
                            Ok(result) => ServerFrame::Ack { req, ok: true, result, error: None },
                            Err(e) => ServerFrame::Ack { req, ok: false, result: Value::Null, error: Some(e) },
                        });
                        frames
                    }
                    Some(ClientFrame::Ping { n }) => vec![ServerFrame::Pong { n }],
                    Some(ClientFrame::Hello { .. }) => Vec::new(),
                    None => return,
                };
                if !send_frames(&in_tx, &waker, frames) {
                    return;
                }
            }
            r = sync_rx.recv() => {
                let Some(Ok(r)) = r else { continue };
                w.absorb(&r).await;
                let mut frames: Vec<ServerFrame> = w.map.take().into_iter().map(ServerFrame::Event).collect();
                frames.extend(w.problems.drain(..).map(|message| ServerFrame::Error { message }));
                if !send_frames(&in_tx, &waker, frames) {
                    return;
                }
            }
        }
    }
}
