//! Identity verification by emoji comparison, against a real homeserver.
//!
//! Skipped unless `CHATCRAFT_TEST_HOMESERVER` is set and the homeserver allows open registration
//! (`CHATCRAFT_TEST_REGISTER=1`; test homeservers only — see `docs/matrix.md`, "Testing"). Each run
//! registers two fresh accounts, because cross-signing is set up once per account.

use std::path::Path;
use std::time::{Duration, Instant};

use chatcraft_engine::proto::VerifyStage;
use chatcraft_engine::{Loopback, Session};
use chatcraft_matrix::{MatrixConfig, MatrixTransport};
use serde_json::{Value, json};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn register(hs: &str, user: &str, password: &str) {
    let body = json!({ "username": user, "password": password, "auth": { "type": "m.login.dummy" } }).to_string();
    let out = std::process::Command::new("curl")
        .args(["-sf", "-X", "POST", "-H", "Content-Type: application/json", "-d", &body, &format!("{hs}/_matrix/client/v3/register")])
        .output()
        .unwrap();
    assert!(out.status.success(), "registering {user} failed: {}", String::from_utf8_lossy(&out.stdout));
}

fn session(hs: &str, user: &str, password: &str, store: &Path) -> Session {
    let link = MatrixTransport::connect(MatrixConfig::new(hs, user, password, store.to_path_buf())).unwrap();
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

/// Pumps both sessions until `f` holds for them.
fn until(a: &mut Session, b: &mut Session, what: &str, mut f: impl FnMut(&Session, &Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !f(a, b) {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        a.pump_wait(Duration::from_millis(100));
        b.pump_wait(Duration::from_millis(100));
    }
}

fn stage(s: &Session) -> Option<&VerifyStage> {
    s.verifications.last().map(|v| &v.stage)
}

fn emoji(s: &Session) -> Option<Vec<(String, String)>> {
    match stage(s) {
        Some(VerifyStage::Emoji { emoji }) => Some(emoji.clone()),
        _ => None,
    }
}

#[test]
fn people_verify_each_other_by_comparing_emoji() {
    let (Some(hs), Some(_)) = (env("CHATCRAFT_TEST_HOMESERVER"), env("CHATCRAFT_TEST_REGISTER")) else {
        eprintln!("skipped: set CHATCRAFT_TEST_HOMESERVER and CHATCRAFT_TEST_REGISTER=1 (test homeservers with open registration only)");
        return;
    };
    let server = env("CHATCRAFT_TEST_SERVER_NAME").unwrap_or_else(|| "localhost".into());
    let tag = format!("{}{}", std::process::id(), Instant::now().elapsed().as_nanos() % 1000);
    let (a_name, b_name) = (format!("vera{tag}"), format!("walt{tag}"));
    register(&hs, &a_name, "pw-vera-123");
    register(&hs, &b_name, "pw-walt-123");
    let dir = std::env::temp_dir().join(format!("chatcraft-verify-{tag}"));
    let mut vera = session(&hs, &a_name, "pw-vera-123", &dir.join("vera"));
    let mut walt = session(&hs, &b_name, "pw-walt-123", &dir.join("walt"));

    // They talk first (an encrypted DM), so they know each other.
    run(&mut vera, "dm.open", json!({ "users": [format!("@{b_name}:{server}")] }));
    run(&mut vera, "message.send", json!({ "text": "Let's verify each other" }));
    until(&mut vera, &mut walt, "Walt sees Vera", |_, w| w.state.user_by_handle(&a_name).is_some());
    let walt_id = vera.state.user_by_handle(&b_name).unwrap().id;
    assert!(!vera.state.user(walt_id).unwrap().verified);

    // Vera asks; Walt is asked and accepts.
    run(&mut vera, "security.verify", json!({ "user": format!("@{b_name}") }));
    until(&mut vera, &mut walt, "Walt receives the request", |_, w| {
        w.active_verification().is_some_and(|v| v.incoming && v.stage == VerifyStage::Requested)
    });
    run(&mut walt, "security.accept", json!({}));

    // Both screens show the same seven emoji.
    until(&mut vera, &mut walt, "both see emoji", |v, w| emoji(v).is_some() && emoji(w).is_some());
    let (ev, ew) = (emoji(&vera).unwrap(), emoji(&walt).unwrap());
    eprintln!("Vera sees: {}", ev.iter().map(|(s, n)| format!("{s} {n}")).collect::<Vec<_>>().join(", "));
    eprintln!("Walt sees: {}", ew.iter().map(|(s, n)| format!("{s} {n}")).collect::<Vec<_>>().join(", "));
    assert_eq!(ev.len(), 7);
    assert_eq!(ev, ew, "the short authentication strings match");

    // They confirm; each now trusts the other.
    run(&mut vera, "security.confirm", json!({}));
    run(&mut walt, "security.confirm", json!({}));
    until(&mut vera, &mut walt, "both finish", |v, w| stage(v) == Some(&VerifyStage::Done) && stage(w) == Some(&VerifyStage::Done));
    let vera_id = walt.state.user_by_handle(&a_name).unwrap().id;
    until(&mut vera, &mut walt, "both are marked verified", |v, w| {
        v.state.user(walt_id).is_some_and(|u| u.verified) && w.state.user(vera_id).is_some_and(|u| u.verified)
    });

    // A mismatch stops a verification on both sides.
    run(&mut walt, "security.verify", json!({ "user": format!("@{a_name}") }));
    until(&mut vera, &mut walt, "Vera receives the second request", |v, _| v.active_verification().is_some_and(|x| x.incoming));
    run(&mut vera, "security.accept", json!({}));
    until(&mut vera, &mut walt, "both see emoji again", |v, w| emoji(v).is_some() && emoji(w).is_some());
    run(&mut vera, "security.mismatch", json!({}));
    until(&mut vera, &mut walt, "both see it cancelled", |v, w| {
        matches!(stage(v), Some(VerifyStage::Cancelled { .. })) && matches!(stage(w), Some(VerifyStage::Cancelled { .. }))
    });

    // After a restart, trust is remembered.
    drop(vera);
    let vera = session(&hs, &a_name, "", &dir.join("vera"));
    assert!(vera.state.user(walt_id).is_some_and(|u| u.verified), "trust survives a restart");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_device_reads_history_after_restoring_from_backup() {
    let (Some(hs), Some(_)) = (env("CHATCRAFT_TEST_HOMESERVER"), env("CHATCRAFT_TEST_REGISTER")) else {
        eprintln!("skipped: set CHATCRAFT_TEST_HOMESERVER and CHATCRAFT_TEST_REGISTER=1");
        return;
    };
    let server = env("CHATCRAFT_TEST_SERVER_NAME").unwrap_or_else(|| "localhost".into());
    let tag = format!("{}{}", std::process::id(), Instant::now().elapsed().as_nanos() % 1000);
    let (r_name, s_name) = (format!("rina{tag}"), format!("sam{tag}"));
    register(&hs, &r_name, "pw-rina-123");
    register(&hs, &s_name, "pw-sam-123");
    let dir = std::env::temp_dir().join(format!("chatcraft-backup-{tag}"));
    let mut laptop = session(&hs, &r_name, "pw-rina-123", &dir.join("laptop"));
    let mut sam = session(&hs, &s_name, "pw-sam-123", &dir.join("sam"));

    run(&mut laptop, "dm.open", json!({ "users": [format!("@{s_name}:{server}")] }));
    until(&mut laptop, &mut sam, "Sam joins the DM", |_, s| s.state.channels.values().any(|c| c.kind.is_dm()));
    let secret = "the spare key is under the blue pot";
    let c = sam.state.channels.values().find(|c| c.kind.is_dm()).unwrap().id;
    run(&mut sam, "message.send", json!({ "channel": c.to_string(), "text": secret }));
    until(&mut laptop, &mut sam, "the laptop decrypts it", |l, _| l.state.messages.values().any(|m| m.text == secret));

    // Recovery on the laptop: keys are backed up, encrypted, on the server.
    let key = run(&mut laptop, "security.setupRecovery", json!({}))["recoveryKey"].as_str().unwrap().to_string();
    eprintln!("recovery key: {} words", key.split_whitespace().count());
    drop(laptop);

    // A brand-new device can't read the old message…
    let mut phone = session(&hs, &r_name, "pw-rina-123", &dir.join("phone"));
    until(&mut phone, &mut sam, "the phone sees the message as undecryptable", |p, _| p.state.messages.values().any(|m| m.undecryptable));
    assert!(!phone.state.messages.values().any(|m| m.text == secret));
    // …until it restores from the backup.
    let wrong = phone.run("security.recover", &json!({ "key": "EsTc nope nope" }));
    let wrong = match wrong.ok().and_then(|v| v.get("pending").and_then(Value::as_u64)) {
        Some(req) => phone.wait_for(req, Duration::from_secs(60)).unwrap(),
        None => Err(chatcraft_engine::CmdError::Failed("refused".into())),
    };
    assert!(wrong.is_err(), "a wrong recovery key is refused");
    run(&mut phone, "security.recover", json!({ "key": key }));
    until(&mut phone, &mut sam, "the phone reads the old message", |p, _| p.state.messages.values().any(|m| m.text == secret && !m.undecryptable));
    let _ = std::fs::remove_dir_all(&dir);
}
