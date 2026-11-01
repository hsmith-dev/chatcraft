//! End-to-end encryption against a real homeserver.
//!
//! Skipped unless `CHATCRAFT_TEST_HOMESERVER` is set. With a local Synapse (see
//! `docs/matrix.md`, "Testing"):
//!
//! ```sh
//! CHATCRAFT_TEST_HOMESERVER=http://localhost:8008 CHATCRAFT_TEST_SERVER_NAME=localhost \
//! CHATCRAFT_TEST_ALICE=alice:pw-alice-123 CHATCRAFT_TEST_BOB=bob:pw-bob-123 \
//!   cargo test -p chatcraft-matrix --test e2ee -- --nocapture
//! ```

use std::path::Path;
use std::time::{Duration, Instant};

use chatcraft_engine::{Loopback, Session};
use chatcraft_matrix::{MatrixConfig, MatrixTransport};
use serde_json::{Value, json};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn session(hs: &str, cred: &str, store: &Path) -> Session {
    let (user, pw) = cred.split_once(':').unwrap_or((cred, ""));
    let link = MatrixTransport::connect(MatrixConfig::new(hs, user, pw, store.to_path_buf())).unwrap();
    let mut s = Session::connect(Box::new(link), Loopback::hello(user));
    assert!(s.wait_ready(Duration::from_secs(60)), "{user} could not sign in: {:?}", s.error);
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    let r = s.run(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"));
    match r.get("pending").and_then(Value::as_u64) {
        Some(req) => s.wait_for(req, Duration::from_secs(60)).unwrap_or_else(|| panic!("{id}: no answer")).unwrap_or_else(|e| panic!("{id}: {e}")),
        None => r,
    }
}

fn until(s: &mut Session, what: &str, mut f: impl FnMut(&Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !f(s) {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        s.pump_wait(Duration::from_millis(250));
    }
}

/// The homeserver's copy of every room `token` is in, as raw JSON text.
fn server_view(hs: &str, token: &str) -> String {
    let get = |path: &str| -> String {
        let out = std::process::Command::new("curl")
            .args(["-sf", "-H", &format!("Authorization: Bearer {token}"), &format!("{hs}{path}")])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let rooms: Value = serde_json::from_str(&get("/_matrix/client/v3/joined_rooms")).unwrap();
    rooms["joined_rooms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| get(&format!("/_matrix/client/v3/rooms/{}/messages?dir=b&limit=100", r.as_str().unwrap())))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn private_channels_are_end_to_end_encrypted() {
    let (Some(hs), Some(alice_cred), Some(bob_cred)) = (env("CHATCRAFT_TEST_HOMESERVER"), env("CHATCRAFT_TEST_ALICE"), env("CHATCRAFT_TEST_BOB"))
    else {
        eprintln!("skipped: set CHATCRAFT_TEST_HOMESERVER, CHATCRAFT_TEST_ALICE and CHATCRAFT_TEST_BOB");
        return;
    };
    let server = env("CHATCRAFT_TEST_SERVER_NAME").unwrap_or_else(|| "localhost".into());
    let dir = std::env::temp_dir().join(format!("chatcraft-e2ee-{}", std::process::id()));
    let mut alice = session(&hs, &alice_cred, &dir.join("alice"));
    let mut bob = session(&hs, &bob_cred, &dir.join("bob"));
    let bob_mxid = format!("@{}:{server}", bob_cred.split(':').next().unwrap_or("bob"));

    // A private channel is created encrypted, with Bob invited by Matrix ID.
    let name = format!("secret-{}", std::process::id());
    let r = run(&mut alice, "channel.create", json!({ "name": name, "private": true, "members": [bob_mxid] }));
    let c = chatcraft_engine::model::ChannelId(r["channel"].as_u64().unwrap());
    assert!(alice.state.channel(c).unwrap().encrypted, "private channels are encrypted");
    until(&mut bob, "Bob joins the channel", |s| s.state.channel_by_name(&name).is_some());
    until(&mut alice, "Alice sees Bob in the channel", |s| s.state.channel(c).is_some_and(|ch| ch.members.len() == 2));

    // Alice sends; Bob decrypts.
    let secret = "the launch code is 4242 :rocket:";
    run(&mut alice, "message.send", json!({ "channel": c.to_string(), "text": secret }));
    until(&mut bob, "Bob decrypts the message", |s| s.state.messages.values().any(|m| m.text == secret && m.encrypted && !m.undecryptable));
    eprintln!("Bob decrypted: {secret}");
    let bc = bob.state.channel_by_name(&name).unwrap().id;
    let root = bob.state.timeline(bc).iter().find(|m| m.text == secret).unwrap().id;

    // Reactions, threads and edits travel encrypted too.
    run(&mut bob, "message.react", json!({ "message": root.0, "emoji": "tada", "on": true }));
    run(&mut bob, "message.reply", json!({ "message": root.0, "text": "copy that" }));
    until(&mut alice, "Alice sees Bob's reaction and reply", |s| {
        s.state.timeline(c).iter().any(|m| m.text == secret && !m.reactions.is_empty() && m.reply_count == 1)
    });
    let mine = alice.state.timeline(c).iter().find(|m| m.text == secret).unwrap().id;
    run(&mut alice, "message.edit", json!({ "message": mine.0, "text": "the launch code is 4243" }));
    until(&mut bob, "Bob sees the edit", |s| s.state.message(root).is_some_and(|m| m.text == "the launch code is 4243" && m.edited.is_some()));

    // The homeserver only ever held ciphertext.
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("alice/session.json")).unwrap()).unwrap();
    let token = saved["tokens"]["access_token"].as_str().or_else(|| saved["access_token"].as_str()).unwrap();
    let view = server_view(&hs, token);
    eprintln!("server view: {} bytes, {} encrypted events", view.len(), view.matches("m.room.encrypted").count());
    assert!(view.contains("m.room.encrypted"), "the server stored encrypted events");
    for plain in ["launch code", "copy that", "4243"] {
        assert!(!view.contains(plain), "the server must never see `{plain}`");
    }

    // Bob restarts: the same device comes back from its store (no password) and can still read.
    drop(bob);
    let mut bob = session(&hs, &format!("{}:", bob_cred.split(':').next().unwrap_or("bob")), &dir.join("bob"));
    until(&mut bob, "Bob reads history after restarting", |s| s.state.messages.values().any(|m| m.text == "the launch code is 4243"));
    eprintln!("after restart Bob still reads {} decrypted messages", bob.state.messages.values().filter(|m| m.encrypted && !m.undecryptable).count());

    let _ = std::fs::remove_dir_all(&dir);
}
