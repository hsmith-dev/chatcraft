//! `chatcraft-server`: hosts one workspace over WebSocket.
//!
//! ```text
//! chatcraft-server [--bind 127.0.0.1:7900] [--data ./chatcraft-data | --memory]
//!                  [--name "Workspace"] [--demo] [--token TOKEN]
//! ```
//! The join token is read from `--token`, `CHATCRAFT_TOKEN`, or `<data>/token` (created with a
//! random value on first run). Anyone with it can sign in with any unused handle.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::PathBuf;
use std::process::ExitCode;

use chatcraft_net::ServerOptions;
use chatcraft_net::server::{Server, random_token};

const USAGE: &str = "usage: chatcraft-server [--bind ADDR] [--data DIR | --memory] [--name NAME] [--demo] [--token TOKEN]";

fn run() -> Result<(), String> {
    let mut bind = "127.0.0.1:7900".to_string();
    let mut data: Option<PathBuf> = Some(PathBuf::from("chatcraft-data"));
    let mut name = "My Workspace".to_string();
    let mut demo = false;
    let mut token = std::env::var("CHATCRAFT_TOKEN").ok().filter(|t| !t.is_empty());
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{a} needs a value\n{USAGE}"));
        match a.as_str() {
            "--bind" => bind = val()?,
            "--data" => data = Some(PathBuf::from(val()?)),
            "--memory" => data = None,
            "--name" => name = val()?,
            "--demo" => demo = true,
            "--token" => token = Some(val()?),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    // The token lives in `<data>/token` so a restart keeps it; `--token` replaces it.
    let token = match &data {
        Some(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = dir.join("token");
            let stored = std::fs::read_to_string(&path).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            let t = token.or(stored).unwrap_or_else(random_token);
            std::fs::write(&path, &t).map_err(|e| format!("{}: {e}", path.display()))?;
            t
        }
        None => token.unwrap_or_else(random_token),
    };
    let bind = bind.parse().map_err(|e| format!("--bind {bind}: {e}"))?;
    let server = Server::open(ServerOptions { bind, data_dir: data.clone(), token: token.clone(), workspace_name: name, demo })?;
    for w in &server.warnings {
        eprintln!("warning: {w}");
    }
    let ws = server.workspace_name();
    let store = data.map(|d| d.display().to_string()).unwrap_or_else(|| "memory (nothing is saved)".into());
    server.run(|addr| {
        eprintln!("ChatCraft server: workspace \"{ws}\" on ws://{addr}");
        eprintln!("  data:  {store}");
        eprintln!("  token: {token}");
        eprintln!("  join:  chatcraft --server ws://{addr} --handle YOURNAME --token {token}");
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("chatcraft-server: {e}");
            ExitCode::FAILURE
        }
    }
}
