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

use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::{EventId, OwnedUserId, RoomId, UInt, UserId};
use matrix_sdk::sync::SyncResponse;
use matrix_sdk::{Client, Room, RoomMemberships};
use serde_json::{Value, json};
use tokio::sync::mpsc as tmpsc;

use crate::mapper::{Mapper, RoomInfo, format_status};
use crate::secrets::Secrets;
use crate::verify::{Change, Verifications};
use crate::{MatrixConfig, Waker, set_status, wake};

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A random secret (64 hex characters) from the OS CSPRNG.
fn random_secret() -> R<String> {
    let mut b = [0u8; 32];
    // OS randomness only: this secret protects the device's encryption keys at rest.
    getrandom::fill(&mut b).map_err(|e| format!("no secure randomness available: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// The keychain account for a config: `user@homeserver`.
pub(crate) fn account(cfg: &MatrixConfig) -> String {
    format!(
        "{}@{}",
        cfg.user.trim_start_matches('@'),
        cfg.homeserver.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/')
    )
}

/// Returns the client and whether this was a fresh password sign-in (a new device).
async fn sign_in(cfg: &MatrixConfig) -> R<(Client, bool)> {
    std::fs::create_dir_all(&cfg.store_dir).map_err(|e| format!("{}: {e}", cfg.store_dir.display()))?;
    let mut secrets = Secrets::new(cfg.secrets, &cfg.store_dir, &account(cfg));
    let pass = match secrets.get("passphrase") {
        Some(p) => p,
        None => {
            let p = random_secret()?;
            secrets.set("passphrase", &p)?;
            p
        }
    };
    let client = Client::builder()
        .server_name_or_homeserver_url(&cfg.homeserver)
        .sqlite_store(cfg.store_dir.join("store"), Some(&pass))
        // Once recovery is set up, room keys are backed up as they arrive, and a key missing on
        // this device is looked up in the backup.
        .with_encryption_settings(matrix_sdk::encryption::EncryptionSettings {
            auto_enable_cross_signing: false,
            backup_download_strategy: matrix_sdk::encryption::BackupDownloadStrategy::AfterDecryptionFailure,
            auto_enable_backups: true,
        })
        .build()
        .await
        .map_err(err)?;
    if let Some(text) = secrets.get("session")
        && let Ok(session) = serde_json::from_str::<MatrixSession>(&text)
    {
        match client.restore_session(session).await {
            Ok(()) => return Ok((client, false)),
            Err(e) => log::warn!("saved session rejected ({e}); signing in again"),
        }
    }
    if cfg.password.is_empty() {
        return Err("no saved session for this account; a password is needed".into());
    }
    client.matrix_auth().login_username(&cfg.user, &cfg.password).initial_device_display_name(&cfg.device_name).await.map_err(err)?;
    if let Some(session) = client.matrix_auth().session() {
        secrets.set("session", &serde_json::to_string(&session).map_err(err)?)?;
    }
    if let Some(why) = &secrets.fallback {
        log::warn!("OS keychain unavailable ({why}); secrets kept in private files in {}", cfg.store_dir.display());
    }
    Ok((client, true))
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
        pinned: room.pinned_event_ids().unwrap_or_default().iter().map(ToString::to_string).collect(),
        muted: false,
        archived: state_content(room, ARCHIVED).await.and_then(|c| c.get("archived").and_then(Value::as_bool)).unwrap_or(false),
    }
}

/// The room state ChatCraft marks archived rooms with: `{"archived", "events_default_before"}`.
const ARCHIVED: &str = "dev.chatcraft.archived";

/// A room state event's content as JSON (empty state key).
async fn state_content(room: &Room, event_type: &str) -> Option<Value> {
    let raw = room.get_state_event(event_type.into(), "").await.ok()??;
    let text = match &raw {
        matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Sync(r) => r.json().get().to_string(),
        matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Stripped(r) => r.json().get().to_string(),
    };
    serde_json::from_str::<Value>(&text).ok()?.get("content").cloned()
}

/// The account data type ChatCraft keeps saved items in.
const SAVED: &str = "dev.chatcraft.saved";

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
    verify: Verifications,
    /// Frames other than events waiting to go out (verification updates).
    outbox: Vec<ServerFrame>,
    /// The person signed out: stop, and forget this device.
    signed_out: bool,
    /// Saved items as stored in account data: the truth, including items outside loaded history.
    saved: Vec<(String, String)>,
    /// Per room: the pagination token for the next older page (`None`: the beginning is loaded).
    older: std::collections::HashMap<String, Option<String>>,
}

/// How many recent events of each conversation the welcome snapshot carries.
const HISTORY: u32 = 100;

impl Worker {
    /// The first sync and the welcome snapshot.
    async fn start(&mut self, fresh: bool) -> R<()> {
        // Continue from this device's stored sync token (a fresh device has none and gets a
        // full sync). The snapshot below reads rooms from the store and fetches history itself,
        // so it doesn't depend on what this sync returns. (Starting from scratch on every launch
        // made identical requests that homeservers answer from a cache — Synapse does for a couple
        // of minutes — and served stale account data such as push rules.)
        let r = self.client.sync_once(sync_settings().timeout(Duration::from_secs(0))).await.map_err(err)?;
        // Accept waiting invitations first, so they are in the snapshot.
        self.accept_invites(&r).await;
        self.snapshot().await;
        self.absorb_rooms(&r).await;
        // A new device of an account without cross-signing sets it up, so people can verify it.
        if fresh && let Err(e) = crate::verify::bootstrap_cross_signing(&self.client, &self.cfg.user, &self.cfg.password).await {
            self.problems.push(format!("couldn't set up cross-signing: {e}"));
        }
        self.refresh_trust().await;
        self.load_saved().await;
        Ok(())
    }

    async fn info(&self, room: &Room) -> RoomInfo {
        let mut info = room_info(room).await;
        info.direct |= self.direct.contains(&info.room_id);
        info.muted = matches!(
            self.client.notification_settings().await.get_user_defined_room_notification_mode(room.room_id()).await,
            Some(matrix_sdk::notification_settings::RoomNotificationMode::Mute)
        );
        info
    }

    /// The saved list from account data (`[{"room", "event"}]`).
    async fn load_saved(&mut self) {
        let Ok(Some(raw)) = self.client.account().account_data_raw(SAVED.into()).await else { return };
        let Ok(v) = serde_json::from_str::<Value>(raw.json().get()) else { return };
        self.saved = v
            .get("events")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter().filter_map(|x| Some((x.get("room")?.as_str()?.to_string(), x.get("event")?.as_str()?.to_string()))).take(1000).collect()
            })
            .unwrap_or_default();
        self.map.set_saved_list(&self.saved);
    }

    /// The welcome snapshot: every joined room from the SDK's store, with its recent history
    /// fetched (and decrypted) explicitly, so it doesn't depend on what the first sync returned.
    async fn snapshot(&mut self) {
        for room in self.client.joined_rooms() {
            // Refresh each member list once per launch: names and memberships the store holds
            // may be stale (the incremental first sync doesn't carry full room state).
            room.mark_members_missing();
            if let Err(e) = room.sync_members().await {
                log::warn!("members of {}: {e}", room.room_id());
            }
            let info = self.info(&room).await;
            self.map.upsert_room(&info);
            let mut opts = matrix_sdk::room::MessagesOptions::backward();
            opts.limit = UInt::from(HISTORY);
            match room.messages(opts).await {
                Ok(page) => {
                    for ev in page.chunk.iter().rev() {
                        self.timeline_event(room.room_id().as_str(), ev);
                    }
                    // Everyone's stored read receipt, now that the messages they point at are known.
                    let members: Vec<OwnedUserId> =
                        room.members(RoomMemberships::JOIN).await.map(|ms| ms.iter().map(|m| m.user_id().to_owned()).collect()).unwrap_or_default();
                    for u in members.iter().filter(|u| **u != self.me).take(200) {
                        use matrix_sdk::ruma::events::receipt::{ReceiptThread, ReceiptType};
                        if let Ok(Some((event_id, _))) = room.load_user_receipt(ReceiptType::Read, &ReceiptThread::Unthreaded, u).await {
                            self.map.set_receipt(room.room_id().as_str(), event_id.as_str(), u.as_str());
                        }
                    }
                    // Where the next older page starts (none: this is the room's beginning).
                    let more = if page.chunk.is_empty() { None } else { page.end.clone() };
                    self.older.insert(room.room_id().to_string(), more);
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

    /// A verification request someone sent us (in a room or to this device). `own_ok`: it came
    /// to this device directly, so it may be from another of our own devices.
    async fn incoming_request(&mut self, sender: &str, flow: &str, own_ok: bool) {
        if self.verify.knows(flow) || (sender == self.me.as_str() && !own_ok) {
            return;
        }
        let Ok(uid) = UserId::parse(sender) else { return };
        if let Some(req) = self.client.encryption().get_verification_request(&uid, flow).await {
            let user = self.map.user(sender, None);
            if let Some(u) = self.verify.track(req, user) {
                self.outbox.push(ServerFrame::Verification(u));
            }
        }
    }

    /// Whether the viewer has verified each person they can see.
    async fn refresh_trust(&mut self) {
        let people: Vec<String> = self.map.state().users.keys().filter_map(|u| self.map.mxid(*u)).map(str::to_string).collect();
        for mxid in people {
            let Ok(uid) = UserId::parse(mxid.as_str()) else { continue };
            if uid == self.me {
                continue;
            }
            if let Ok(Some(identity)) = self.client.encryption().get_user_identity(&uid).await {
                self.map.set_verified(&mxid, identity.is_verified());
            }
        }
    }

    /// Tries again to decrypt messages shown as undecryptable (after a key arrived or the backup
    /// was restored). `download`: fetch each room's keys from the backup first.
    async fn retry_undecryptable(&mut self, download: bool) {
        let utd: Vec<(String, String)> = self
            .map
            .state()
            .messages
            .values()
            .filter(|m| m.undecryptable && !m.deleted)
            .rev()
            .take(200)
            .filter_map(|m| Some((self.map.room_id(m.channel)?.to_string(), self.map.event_id(m.id)?.to_string())))
            .collect();
        if utd.is_empty() {
            return;
        }
        if download {
            let rooms: std::collections::BTreeSet<&String> = utd.iter().map(|(r, _)| r).collect();
            for r in rooms {
                if let Ok(rid) = RoomId::parse(r.as_str())
                    && let Err(e) = self.client.encryption().backups().download_room_keys_for_room(&rid).await
                {
                    log::warn!("backup keys for {r}: {e}");
                }
            }
        }
        for (rid, eid) in utd {
            let (Ok(r), Ok(e)) = (RoomId::parse(rid.as_str()), EventId::parse(eid.as_str())) else { continue };
            let Some(room) = self.client.get_room(&r) else { continue };
            if let Ok(ev) = room.event(&e, None).await {
                self.timeline_event(&rid, &ev);
            }
        }
    }

    async fn on_verification(&mut self, change: Change) {
        let out = self.verify.on_change(change).await;
        if let Some((mxid, v)) = out.verified {
            self.map.set_verified(&mxid, v);
        }
        if let Some(u) = out.update {
            self.outbox.push(ServerFrame::Verification(u));
        }
    }

    async fn absorb_rooms(&mut self, r: &SyncResponse) {
        let mut requests: Vec<(String, String, bool)> = Vec::new();
        for ev in &r.to_device {
            let Ok(json) = serde_json::from_str::<Value>(ev.as_raw().json().get()) else { continue };
            if json.get("type").and_then(Value::as_str) == Some("m.key.verification.request")
                && let (Some(s), Some(f)) =
                    (json.get("sender").and_then(Value::as_str), json.get("content").and_then(|c| c.get("transaction_id")).and_then(Value::as_str))
            {
                requests.push((s.to_string(), f.to_string(), true));
            }
        }
        for up in r.rooms.joined.values() {
            for ev in &up.timeline.events {
                let Ok(json) = serde_json::from_str::<Value>(ev.raw().json().get()) else { continue };
                let msgtype = json.get("content").and_then(|c| c.get("msgtype")).and_then(Value::as_str);
                if msgtype == Some("m.key.verification.request")
                    && let (Some(s), Some(e)) = (json.get("sender").and_then(Value::as_str), json.get("event_id").and_then(Value::as_str))
                {
                    requests.push((s.to_string(), e.to_string(), false));
                }
            }
        }
        for (sender, flow, own_ok) in requests {
            self.incoming_request(&sender, &flow, own_ok).await;
        }
        for (rid, up) in &r.rooms.joined {
            if let Some(room) = self.client.get_room(rid) {
                let info = self.info(&room).await;
                self.map.upsert_room(&info);
            }
            for ev in &up.timeline.events {
                self.timeline_event(rid.as_str(), ev);
            }
            for ev in &up.ephemeral {
                let Ok(json) = serde_json::from_str::<Value>(ev.json().get()) else { continue };
                if json.get("type").and_then(Value::as_str) == Some("m.receipt")
                    && let Some(content) = json.get("content").and_then(Value::as_object)
                {
                    // {event_id: {"m.read": {user_id: {ts}}}}
                    for (event_id, kinds) in content.iter().take(200) {
                        for user in kinds.get("m.read").and_then(Value::as_object).into_iter().flat_map(|m| m.keys()).take(200) {
                            self.map.set_receipt(rid.as_str(), event_id, user);
                        }
                    }
                }
                if json.get("type").and_then(Value::as_str) == Some("m.typing") {
                    let ids: Vec<String> = json
                        .get("content")
                        .and_then(|c| c.get("user_ids"))
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).take(50).map(str::to_string).collect())
                        .unwrap_or_default();
                    self.map.set_typing(rid.as_str(), &ids);
                }
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
        let saved_changed = r
            .account_data
            .iter()
            .any(|e| serde_json::from_str::<Value>(e.json().get()).is_ok_and(|v| v.get("type").and_then(Value::as_str) == Some(SAVED)));
        if saved_changed {
            self.load_saved().await;
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

    /// Message content: the markup as `body`, HTML rendered by ChatCraft's own parser as
    /// `formatted_body` (so `*x*` means bold everywhere), plus intentional mentions.
    fn message_content(&self, text: &str) -> R<Value> {
        let mxid_of = |h: &str| self.map.resolve_mention(h).and_then(|u| self.map.mxid(u)).map(str::to_string);
        let html = crate::html::from_markup(text, &mxid_of);
        let mut content = json!({ "msgtype": "m.text", "body": text });
        if html != crate::html::escape_text(text)
            && let Some(o) = content.as_object_mut()
        {
            o.insert("format".into(), json!("org.matrix.custom.html"));
            o.insert("formatted_body".into(), json!(html));
        }
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
        // Public channels get an address (`#name:server`) so they can be shared and joined even
        // where the server doesn't list them in its directory.
        if !private && !direct {
            req.room_alias_name = name.as_deref().and_then(|n| chatcraft_core::normalize_channel_name(n).ok());
        }
        let alias = req.room_alias_name.clone();
        let room = match self.client.create_room(req.clone()).await {
            Ok(r) => r,
            // The address is taken: create the channel without one.
            Err(e) if req.room_alias_name.is_some() && e.to_string().contains("M_ROOM_IN_USE") => {
                req.room_alias_name = None;
                self.client.create_room(req).await.map_err(err)?
            }
            Err(e) => return Err(err(e)),
        };
        if !private && !direct {
            let listed = self
                .client
                .send(matrix_sdk::ruma::api::client::directory::get_room_visibility::v3::Request::new(room.room_id().to_owned()))
                .await
                .map(|r| r.visibility == Visibility::Public)
                .unwrap_or(false);
            if !listed {
                let server = self.me.server_name().to_string();
                let address = room.canonical_alias().map(|a| a.to_string()).or_else(|| alias.as_ref().map(|a| format!("#{a}:{server}")));
                let address = address.map(|a| format!(" People can join with its address, {a}.")).unwrap_or_default();
                self.problems.push(format!("Your homeserver doesn't let you list rooms in its directory, so the channel isn't listed.{address}"));
            }
        }
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
            members,
            ..RoomInfo::default()
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
            Op::VerifyUser { user } => {
                let mxid = self.mxid(user)?;
                let enc = self.client.encryption();
                let identity = match enc.get_user_identity(&mxid).await.map_err(err)? {
                    Some(i) => Some(i),
                    None => enc.request_user_identity(&mxid).await.map_err(err)?,
                };
                let identity = identity.ok_or_else(|| format!("{mxid} hasn't set up an encryption identity (cross-signing) yet"))?;
                let request = identity.request_verification().await.map_err(err)?;
                let flow = request.flow_id().to_string();
                if let Some(u) = self.verify.track(request, user) {
                    self.outbox.push(ServerFrame::Verification(u));
                }
                Ok(json!({ "flow": flow }))
            }
            Op::Verification { flow, action } => {
                if let Some(u) = self.verify.act(&flow, action).await? {
                    self.outbox.push(ServerFrame::Verification(u));
                }
                Ok(Value::Null)
            }
            Op::SetUpRecovery => {
                let key = self.client.encryption().recovery().enable().wait_for_backups_to_upload().await.map_err(err)?;
                Ok(json!({ "recoveryKey": key }))
            }
            Op::Recover { key } => {
                self.client.encryption().recovery().recover(key.trim()).await.map_err(|e| format!("that recovery key didn't work: {e}"))?;
                self.retry_undecryptable(true).await;
                self.refresh_trust().await;
                let left = self.map.state().messages.values().filter(|m| m.undecryptable && !m.deleted).count();
                Ok(json!({ "restored": true, "stillUndecryptable": left }))
            }
            Op::SignOut => {
                // Best effort on the server; forgetting the device locally always happens.
                if let Err(e) = self.client.matrix_auth().logout().await {
                    log::warn!("server sign-out: {e}");
                }
                self.signed_out = true;
                Ok(json!({ "signedOut": true }))
            }
            Op::Pin { message, pinned } => {
                let eid = self.event_id(message)?;
                let room = self.room(self.map.state().message(message).map(|m| m.channel).unwrap_or_default())?;
                if pinned {
                    room.pin_event(&eid).await.map_err(err)?;
                } else {
                    room.unpin_event(&eid).await.map_err(err)?;
                }
                self.map.set_pinned(message, pinned);
                Ok(Value::Null)
            }
            Op::Mute { channel, muted } => {
                let room = self.room(channel)?;
                let settings = self.client.notification_settings().await;
                if muted {
                    settings
                        .set_room_notification_mode(room.room_id(), matrix_sdk::notification_settings::RoomNotificationMode::Mute)
                        .await
                        .map_err(err)?;
                } else {
                    settings.delete_user_defined_room_rules(room.room_id()).await.map_err(err)?;
                }
                self.map.set_muted(channel, muted);
                Ok(Value::Null)
            }
            Op::Save { message, saved } => {
                let eid = self.event_id(message)?.to_string();
                let rid = self.map.room_id(self.map.state().message(message).map(|m| m.channel).unwrap_or_default()).unwrap_or_default().to_string();
                let mut list = self.saved.clone();
                list.retain(|(_, e)| *e != eid);
                if saved {
                    list.push((rid, eid));
                }
                let content = json!({ "events": list.iter().map(|(r, e)| json!({ "room": r, "event": e })).collect::<Vec<_>>() });
                let raw = matrix_sdk::ruma::serde::Raw::from_json_string(content.to_string()).map_err(err)?;
                self.client.account().set_account_data_raw(SAVED.into(), raw).await.map_err(err)?;
                self.saved = list;
                self.map.set_saved_list(&self.saved);
                Ok(Value::Null)
            }
            Op::Directory { query } => {
                // Signed in and filtered on the server (many homeservers list rooms only to members).
                let mut req = matrix_sdk::ruma::api::client::directory::get_public_rooms_filtered::v3::Request::new();
                req.limit = Some(UInt::from(100u32));
                let mut filter = matrix_sdk::ruma::directory::Filter::new();
                filter.generic_search_term = Some(query.trim().trim_start_matches('#').to_string()).filter(|q| !q.is_empty());
                req.filter = filter;
                let res = self.client.send(req).await.map_err(err)?;
                let q = query.trim().trim_start_matches('#').to_lowercase();
                let rooms: Vec<Value> = res
                    .chunk
                    .iter()
                    .filter(|r| {
                        q.is_empty()
                            || r.name.as_deref().is_some_and(|n| n.to_lowercase().contains(&q))
                            || r.topic.as_deref().is_some_and(|t| t.to_lowercase().contains(&q))
                            || r.canonical_alias.as_ref().is_some_and(|a| a.as_str().to_lowercase().contains(&q))
                    })
                    .map(|r| {
                        json!({
                            "room": r.room_id.to_string(),
                            "alias": r.canonical_alias.as_ref().map(ToString::to_string),
                            "name": r.name.clone().unwrap_or_else(|| r.room_id.to_string()),
                            "topic": r.topic.clone().unwrap_or_default(),
                            "members": u64::from(r.num_joined_members),
                            "joined": self.map.known_channel(r.room_id.as_str()).is_some_and(|c| self.map.state().is_member(self.map.me().unwrap_or_default(), c)),
                        })
                    })
                    .collect();
                Ok(json!({ "rooms": rooms }))
            }
            Op::JoinExternal { room } => {
                let id = matrix_sdk::ruma::RoomOrAliasId::parse(room.trim()).map_err(|e| format!("`{room}` isn't a room id or alias: {e}"))?;
                let joined = self.client.join_room_by_id_or_alias(&id, &[]).await.map_err(err)?;
                let info = self.info(&joined).await;
                Ok(json!({ "channel": self.map.upsert_room(&info) }))
            }
            Op::LoadOlder { channel } => {
                let room = self.room(channel)?;
                let rid = room.room_id().to_string();
                let Some(Some(token)) = self.older.get(&rid).cloned() else {
                    return Ok(json!({ "loaded": 0, "more": false }));
                };
                let mut opts = matrix_sdk::room::MessagesOptions::backward().from(Some(token.as_str()));
                opts.limit = UInt::from(HISTORY);
                let page = room.messages(opts).await.map_err(err)?;
                for ev in page.chunk.iter().rev() {
                    self.timeline_event(&rid, ev);
                }
                let loaded = page.chunk.len();
                let next = if page.chunk.is_empty() { None } else { page.end.clone() };
                let more = next.is_some();
                self.older.insert(rid, next);
                Ok(json!({ "loaded": loaded, "more": more }))
            }
            Op::Typing { channel, typing } => {
                self.room(channel)?.typing_notice(typing).await.map_err(err)?;
                Ok(Value::Null)
            }
            Op::Devices => {
                let list = self.client.devices().await.map_err(err)?;
                // Fetch our device keys fresh, so devices that signed in recently have their trust state.
                let _ = self.client.encryption().request_user_identity(&self.me).await;
                let this = self.client.device_id().map(ToString::to_string).unwrap_or_default();
                let enc = self.client.encryption();
                let mut out = Vec::new();
                for d in &list.devices {
                    let verified = enc.get_device(&self.me, &d.device_id).await.ok().flatten().is_some_and(|x| x.is_verified());
                    out.push(json!({
                        "id": d.device_id.to_string(),
                        "name": d.display_name.clone().unwrap_or_else(|| d.device_id.to_string()),
                        "lastSeen": d.last_seen_ts.map(|t| u64::from(t.0)),
                        "lastSeenIp": d.last_seen_ip,
                        "thisDevice": d.device_id.as_str() == this,
                        "verified": verified,
                    }));
                }
                // This device first, then the most recently active.
                out.sort_by_key(|d| {
                    (std::cmp::Reverse(d["thisDevice"].as_bool().unwrap_or(false)), std::cmp::Reverse(d["lastSeen"].as_u64().unwrap_or(0)))
                });
                Ok(json!({ "devices": out }))
            }
            Op::VerifyDevice { device } => {
                if self.client.device_id().is_some_and(|d| d.as_str() == device) {
                    return Err("that's this device".into());
                }
                let id: matrix_sdk::ruma::OwnedDeviceId = device.as_str().into();
                let enc = self.client.encryption();
                let dev = match enc.get_device(&self.me, &id).await.map_err(err)? {
                    Some(d) => d,
                    None => {
                        // A device that signed in after our last key query: ask the server.
                        let _ = enc.request_user_identity(&self.me).await;
                        enc.get_device(&self.me, &id).await.map_err(err)?.ok_or_else(|| format!("device `{device}` has no encryption keys yet"))?
                    }
                };
                let request = dev.request_verification().await.map_err(err)?;
                let flow = request.flow_id().to_string();
                let me = self.map.me().unwrap_or_default();
                if let Some(u) = self.verify.track(request, me) {
                    self.outbox.push(ServerFrame::Verification(u));
                }
                Ok(json!({ "flow": flow }))
            }
            Op::RemoveDevice { device, password } => {
                if self.client.device_id().is_some_and(|d| d.as_str() == device) {
                    return Err("that's this device: use Sign out".into());
                }
                let ids = [matrix_sdk::ruma::OwnedDeviceId::from(device.as_str())];
                // The homeserver asks for the password (user-interactive auth) before removing a device.
                if let Err(e) = self.client.delete_devices(&ids, None).await {
                    use matrix_sdk::ruma::api::client::uiaa;
                    let Some(info) = e.as_uiaa_response() else { return Err(err(e)) };
                    let mut pw =
                        uiaa::Password::new(uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(self.me.localpart().to_string())), password);
                    pw.session = info.session.clone();
                    self.client
                        .delete_devices(&ids, Some(uiaa::AuthData::Password(pw)))
                        .await
                        .map_err(|e| format!("couldn't sign that device out: {e}"))?;
                }
                Ok(json!({ "removed": device }))
            }
            Op::Archive { channel, archived } => {
                // Archived = read-only for everyone (the level needed to post is raised to admin)
                // plus a marker that remembers the old level, so unarchiving restores it exactly.
                let room = self.room(channel)?;
                let mut levels = state_content(&room, "m.room.power_levels").await.unwrap_or_else(|| json!({}));
                let marker = state_content(&room, ARCHIVED).await;
                let send = |room: &Room, ty: &'static str, content: Value| {
                    let room = room.clone();
                    async move {
                        room.send_state_event_raw(ty, "", content)
                            .await
                            .map_err(|e| format!("couldn't change the channel ({e}); archiving needs admin rights in the room"))
                    }
                };
                if archived {
                    let before = levels.get("events_default").cloned().unwrap_or(json!(0));
                    send(&room, ARCHIVED, json!({ "archived": true, "events_default_before": before })).await?;
                    if let Some(o) = levels.as_object_mut() {
                        o.insert("events_default".into(), json!(100));
                    }
                    send(&room, "m.room.power_levels", levels).await?;
                } else {
                    let before = marker.as_ref().and_then(|m| m.get("events_default_before").cloned()).unwrap_or(json!(0));
                    if let Some(o) = levels.as_object_mut() {
                        o.insert("events_default".into(), before);
                    }
                    send(&room, "m.room.power_levels", levels).await?;
                    send(&room, ARCHIVED, json!({ "archived": false })).await?;
                }
                self.map.set_archived(channel, archived);
                Ok(Value::Null)
            }
            Op::RenameWorkspace { .. } => Err("a Matrix homeserver has no workspace name to change".into()),
        }
    }
}

/// Removes everything this device kept: secrets (keychain or files) and the local key store.
async fn forget_device(cfg: &MatrixConfig) {
    let mut secrets = Secrets::new(cfg.secrets, &cfg.store_dir, &account(cfg));
    secrets.delete("session");
    secrets.delete("passphrase");
    if let Err(e) = std::fs::remove_dir_all(&cfg.store_dir) {
        log::warn!("removing {}: {e}", cfg.store_dir.display());
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
    let (client, fresh) = match sign_in(&cfg).await {
        Ok(c) => c,
        Err(e) => {
            set_status(&status, LinkStatus::Offline(format!("sign-in failed: {e}")));
            let _ = send_frames(&in_tx, &waker, vec![ServerFrame::Error { message: format!("Matrix sign-in failed: {e}") }]);
            return;
        }
    };
    let Some(me) = client.user_id().map(ToOwned::to_owned) else { return };
    let (vtx, mut vrx) = tmpsc::unbounded_channel::<Change>();
    let mut w = Worker {
        map: Mapper::new(me.as_str()),
        client: client.clone(),
        me,
        cfg,
        problems: Vec::new(),
        direct: std::collections::HashSet::new(),
        verify: Verifications::new(vtx),
        outbox: Vec::new(),
        signed_out: false,
        saved: Vec::new(),
        older: std::collections::HashMap::new(),
    };

    if let Err(e) = w.start(fresh).await {
        set_status(&status, LinkStatus::Offline(format!("sync failed: {e}")));
        let _ = send_frames(&in_tx, &waker, vec![ServerFrame::Error { message: format!("Matrix sync failed: {e}") }]);
        return;
    }
    let _ = w.map.take();
    let me_id = w.map.me().unwrap_or_default();
    set_status(&status, LinkStatus::Online);
    let mut hello = vec![ServerFrame::Welcome { me: me_id, state: Box::new(w.map.state().clone()) }];
    hello.append(&mut w.outbox);
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
                        frames.append(&mut w.outbox);
                        frames.extend(w.problems.drain(..).map(|message| ServerFrame::Error { message }));
                        frames.push(match r {
                            Ok(result) => ServerFrame::Ack { req, ok: true, result, error: None },
                            Err(e) => ServerFrame::Ack { req, ok: false, result: Value::Null, error: Some(e) },
                        });
                        if w.signed_out {
                            let _ = send_frames(&in_tx, &waker, frames);
                            forget_device(&w.cfg).await;
                            set_status(&status, LinkStatus::Offline("signed out".into()));
                            wake(&waker);
                            return;
                        }
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
            v = vrx.recv() => {
                let Some(v) = v else { continue };
                w.on_verification(v).await;
                let mut frames: Vec<ServerFrame> = w.map.take().into_iter().map(ServerFrame::Event).collect();
                frames.append(&mut w.outbox);
                if !send_frames(&in_tx, &waker, frames) {
                    return;
                }
            }
            r = sync_rx.recv() => {
                let Some(Ok(r)) = r else { continue };
                w.absorb(&r).await;
                // Room keys arrive as to-device messages: retry what couldn't be read.
                if !r.to_device.is_empty() {
                    w.retry_undecryptable(false).await;
                }
                let mut frames: Vec<ServerFrame> = w.map.take().into_iter().map(ServerFrame::Event).collect();
                frames.append(&mut w.outbox);
                frames.extend(w.problems.drain(..).map(|message| ServerFrame::Error { message }));
                if !send_frames(&in_tx, &waker, frames) {
                    return;
                }
            }
        }
    }
}
