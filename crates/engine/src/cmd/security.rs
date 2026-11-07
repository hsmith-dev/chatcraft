//! Encryption and identity verification (Matrix accounts).

use chatcraft_proto::{Op, VerifyAction};
use serde_json::{Value, json};

use super::signed_in;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("security.verify", "Verify identity", "Profile › Verify", verify).params(r#"{"user": "@handle"}"#).when(signed_in),
        CommandSpec::new("security.accept", "Accept verification", "Verification", |s, v| respond(s, v, VerifyAction::Accept))
            .params(r#"{"flow"?: string (default: the active verification)}"#)
            .when(signed_in),
        CommandSpec::new("security.confirm", "The emoji match", "Verification", |s, v| respond(s, v, VerifyAction::Confirm))
            .params(r#"{"flow"?}"#)
            .when(signed_in),
        CommandSpec::new("security.mismatch", "The emoji don't match", "Verification", |s, v| respond(s, v, VerifyAction::Mismatch))
            .params(r#"{"flow"?}"#)
            .when(signed_in),
        CommandSpec::new("security.cancel", "Cancel verification", "Verification", |s, v| respond(s, v, VerifyAction::Cancel))
            .params(r#"{"flow"?}"#)
            .when(signed_in),
        CommandSpec::new("security.setupRecovery", "Set up key backup", "Profile › Encryption", |s, _| s.submit(Op::SetUpRecovery)).when(signed_in),
        CommandSpec::new("security.recover", "Restore from recovery key", "Profile › Encryption", |s, v| {
            s.submit(Op::Recover { key: p::req_str(v, "key")?.to_string() })
        })
        .params(r#"{"key": "recovery key"}"#)
        .when(signed_in),
        CommandSpec::new("security.devices", "Your devices", "Profile menu › Your devices", |s, _| s.submit(Op::Devices)).pure(),
        CommandSpec::new("security.verifyDevice", "Verify device", "Your devices", |s, v| {
            s.submit(Op::VerifyDevice { device: p::req_str(v, "device")?.to_string() })
        })
        .params(r#"{"device": "device id"}"#)
        .when(signed_in),
        CommandSpec::new("security.removeDevice", "Sign out a device", "Your devices", |s, v| {
            s.submit(Op::RemoveDevice { device: p::req_str(v, "device")?.to_string(), password: p::req_str(v, "password")?.to_string() })
        })
        .params(r#"{"device": "device id", "password": string}"#)
        .when(signed_in),
        CommandSpec::new("session.signOut", "Sign out", "Profile menu", |s, _| s.submit(Op::SignOut)).when(signed_in),
        CommandSpec::new("security.status", "Encryption status", "Profile › Security", status).pure(),
    ]
}

fn verify(s: &mut Session, v: &Value) -> CmdResult {
    let user = s.resolve_user(p::req_str(v, "user")?)?;
    if Some(user) == s.me {
        return Err(CmdError::Params("verify another person (verifying your own devices is not supported yet)".into()));
    }
    s.submit(Op::VerifyUser { user })
}

fn respond(s: &mut Session, v: &Value, action: VerifyAction) -> CmdResult {
    let flow = match p::str(v, "flow") {
        Some(f) => f.to_string(),
        None => s.active_verification().map(|x| x.flow.clone()).ok_or_else(|| CmdError::Disabled("no verification in progress".into()))?,
    };
    s.submit(Op::Verification { flow, action })
}

fn status(s: &mut Session, _: &Value) -> CmdResult {
    let verified: Vec<&str> = s.state.users.values().filter(|u| u.verified).map(|u| u.handle.as_str()).collect();
    let encrypted: Vec<String> = s.state.channels.values().filter(|c| c.encrypted).map(|c| s.state.title(c.id, s.me)).collect();
    Ok(json!({ "verifiedPeople": verified, "encryptedConversations": encrypted, "verifications": s.verifications }))
}
