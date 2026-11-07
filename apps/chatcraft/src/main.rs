//! The ChatCraft desktop app.
//!
//! With no arguments it signs back in to the remembered Matrix account (its device and keys are
//! in the OS keychain and the data folder), or shows the sign-in screen. Options:
//! `--demo` (the built-in demo workspace, nothing leaves the machine);
//! `--matrix https://matrix.example.org --user alice [--password PW]` (or `CHATCRAFT_PASSWORD`);
//! `--server ws://host:port --handle NAME --token TOKEN` (the small built-in ChatCraft server);
//! `--dark`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::PathBuf;

use chatcraft_engine::proto::{ClientFrame, PROTOCOL_VERSION};
use chatcraft_engine::{Loopback, Session};
use chatcraft_ui_egui::{Credentials, Shell, SsoPoll, Start};

const USAGE: &str = "usage: chatcraft [--demo]
                 [--matrix https://homeserver --user NAME [--password PW]]
                 [--server ws://host:port --handle NAME --token TOKEN [--name \"Display Name\"]]
                 [--dark]";

#[derive(Default)]
struct Args {
    demo: bool,
    matrix: Option<String>,
    user: String,
    password: String,
    server: Option<String>,
    handle: String,
    name: String,
    token: String,
    dark: bool,
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        password: std::env::var("CHATCRAFT_PASSWORD").unwrap_or_default(),
        token: std::env::var("CHATCRAFT_TOKEN").unwrap_or_default(),
        ..Args::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{x} needs a value\n{USAGE}"));
        match x.as_str() {
            "--demo" => a.demo = true,
            "--matrix" => a.matrix = Some(val()?),
            "--user" => a.user = val()?,
            "--password" => a.password = val()?,
            "--server" => a.server = Some(val()?),
            "--handle" => a.handle = val()?,
            "--name" => a.name = val()?,
            "--token" => a.token = val()?,
            "--dark" => a.dark = true,
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    if a.matrix.is_some() && a.user.is_empty() {
        return Err(format!("--matrix needs --user\n{USAGE}"));
    }
    if a.server.is_some() && a.handle.is_empty() {
        return Err(format!("--server needs --handle\n{USAGE}"));
    }
    Ok(a)
}

/// The remembered account: homeserver and user name only (secrets live in the keychain).
fn account_file() -> PathBuf {
    chatcraft_matrix::data_dir().join("account.json")
}

fn remembered() -> Option<Credentials> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(account_file()).ok()?).ok()?;
    let homeserver = v.get("homeserver")?.as_str()?.to_string();
    let user = v.get("user")?.as_str()?.to_string();
    Some(Credentials { homeserver, user, ..Credentials::default() })
}

fn remember(c: &Credentials) {
    let path = account_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serde_json::json!({ "homeserver": c.homeserver, "user": c.user }).to_string());
}

fn forget() {
    let _ = std::fs::remove_file(account_file());
}

fn connect_matrix(c: &Credentials) -> Result<Session, String> {
    let store = chatcraft_matrix::default_store_dir(&c.homeserver, &c.user);
    let mut cfg = chatcraft_matrix::MatrixConfig::new(&c.homeserver, &c.user, &c.password, store);
    cfg.login_token = c.login_token.clone();
    let link = chatcraft_matrix::MatrixTransport::connect(cfg)?;
    Ok(Session::connect(Box::new(link), Loopback::hello(&c.user)))
}

/// Single sign-on in the system browser, back to a one-off listener on this computer.
fn start_sso(c: &Credentials) -> Result<Box<dyn FnMut() -> SsoPoll>, String> {
    use chatcraft_matrix::sso::{SsoEvent, SsoSignIn};
    let mut sso = SsoSignIn::start(&c.homeserver)?;
    Ok(Box::new(move || match sso.poll() {
        None => SsoPoll::Waiting,
        Some(SsoEvent::Open(url)) => SsoPoll::Open(url),
        Some(SsoEvent::Token(t)) => SsoPoll::Token(t),
        Some(SsoEvent::Failed(e)) => SsoPoll::Failed(e),
    }))
}

struct App(Shell);

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

fn main() -> std::process::ExitCode {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let start = if a.demo {
        Start::Session(Box::new(Session::demo()))
    } else if let Some(hs) = &a.matrix {
        Start::Connect(Credentials { homeserver: hs.clone(), user: a.user.clone(), password: a.password.clone(), ..Credentials::default() })
    } else if let Some(url) = &a.server {
        match chatcraft_net::NetTransport::connect(url) {
            Ok(link) => {
                let hello =
                    ClientFrame::Hello { version: PROTOCOL_VERSION, handle: a.handle.clone(), display_name: a.name.clone(), token: a.token.clone() };
                Start::Session(Box::new(Session::connect(Box::new(link), hello)))
            }
            Err(e) => {
                eprintln!("cannot start networking: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        match remembered() {
            Some(c) => Start::Connect(c),
            None => Start::SignIn(Credentials::default()),
        }
    };
    let offset = chrono::Local::now().offset().local_minus_utc() / 60;
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("ChatCraft").with_inner_size([1360.0, 860.0]).with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    let dark = a.dark;
    let r = eframe::run_native(
        "ChatCraft",
        opts,
        Box::new(move |cc| {
            let mut shell = Shell::new(start, Box::new(connect_matrix));
            shell.sso = Some(Box::new(start_sso));
            shell.on_signed_in = Some(Box::new(remember));
            shell.on_signed_out = Some(Box::new(forget));
            shell.dark = dark;
            shell.utc_offset_minutes = offset;
            if let Some(app) = shell.app() {
                app.set_dark(dark);
                let ctx = cc.egui_ctx.clone();
                app.session.set_waker(Box::new(move || ctx.request_repaint()));
            }
            Ok(Box::new(App(shell)))
        }),
    );
    match r {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("chatcraft: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
