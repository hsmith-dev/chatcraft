//! `chatcraft-cli`: every ChatCraft command, headless.
//!
//! ```text
//! chatcraft-cli commands                       list every command (JSON)
//! chatcraft-cli parity [--markdown]            Slack/Teams parity summary
//! chatcraft-cli run [CONN] --cmd 'id={json}'…  run commands in order, print each result
//! chatcraft-cli mcp [CONN]                     MCP server on stdio
//!
//! CONN: --demo [--as HANDLE]  (default: the in-process demo workspace)
//!       --server ws://HOST:PORT --as HANDLE --token TOKEN
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;
use std::time::Duration;

use chatcraft_engine::proto::{ClientFrame, PROTOCOL_VERSION};
use chatcraft_engine::{Session, catalog};
use serde_json::Value;

const USAGE: &str = "usage: chatcraft-cli <commands | parity [--markdown] | run [CONN] --cmd 'id={json}'... | mcp [CONN]>
CONN: --demo [--as HANDLE] | --server ws://HOST:PORT --as HANDLE --token TOKEN";

struct Conn {
    server: Option<String>,
    handle: String,
    token: String,
    cmds: Vec<String>,
}

fn parse(rest: &[String]) -> Result<Conn, String> {
    let mut c = Conn { server: None, handle: String::new(), token: std::env::var("CHATCRAFT_TOKEN").unwrap_or_default(), cmds: Vec::new() };
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--demo" => c.server = None,
            "--server" => c.server = Some(val()?),
            "--as" => c.handle = val()?,
            "--token" => c.token = val()?,
            "--cmd" => c.cmds.push(val()?),
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    Ok(c)
}

fn session(c: &Conn) -> Result<Session, String> {
    match &c.server {
        None => Ok(if c.handle.is_empty() { Session::demo() } else { Session::demo_as(&c.handle) }),
        Some(url) => {
            if c.handle.is_empty() {
                return Err("--server needs --as HANDLE".into());
            }
            let link = chatcraft_net::NetTransport::connect(url)?;
            let hello =
                ClientFrame::Hello { version: PROTOCOL_VERSION, handle: c.handle.clone(), display_name: String::new(), token: c.token.clone() };
            let mut s = Session::connect(Box::new(link), hello);
            if !s.wait_ready(Duration::from_secs(10)) {
                return Err(s.error.clone().unwrap_or_else(|| format!("could not sign in to {url}")));
            }
            Ok(s)
        }
    }
}

fn run_cmds(c: &Conn) -> Result<(), String> {
    let mut s = session(c)?;
    let mut failed = false;
    for raw in &c.cmds {
        let (id, params) = raw.split_once('=').unwrap_or((raw.as_str(), "{}"));
        let params: Value = serde_json::from_str(params).map_err(|e| format!("{id}: params are not JSON: {e}"))?;
        let r = match s.run(id, &params) {
            Ok(v) if v.get("pending").is_some() => {
                let req = v.get("pending").and_then(Value::as_u64).unwrap_or(0);
                s.wait_for(req, Duration::from_secs(15))
                    .unwrap_or_else(|| Err(chatcraft_engine::CmdError::Failed("no answer from the server".into())))
            }
            other => other,
        };
        match r {
            Ok(v) => println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "command": id, "ok": true, "result": v })).unwrap_or_default()),
            Err(e) => {
                failed = true;
                println!("{}", serde_json::json!({ "command": id, "ok": false, "error": e.to_string() }));
            }
        }
    }
    if failed { Err("a command failed".into()) } else { Ok(()) }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let r = match args.first().map(String::as_str) {
        Some("commands") => {
            println!("{}", serde_json::to_string_pretty(&Session::registry().describe()).unwrap_or_default());
            Ok(())
        }
        Some("parity") => {
            if rest.iter().any(|a| a == "--markdown") {
                print!("{}", catalog::markdown());
            } else {
                println!("{}", serde_json::to_string_pretty(&catalog::summary_json()).unwrap_or_default());
            }
            Ok(())
        }
        Some("run") => parse(rest).and_then(|c| run_cmds(&c)),
        Some("mcp") => parse(rest).and_then(|c| session(&c)).and_then(|s| chatcraft_mcp::serve_stdio(s).map_err(|e| e.to_string())),
        Some("-h" | "--help" | "help") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n{USAGE}")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("chatcraft-cli: {e}");
            ExitCode::FAILURE
        }
    }
}
