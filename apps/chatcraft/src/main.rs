//! The ChatCraft desktop app.
//!
//! `chatcraft` opens the built-in demo workspace (in-process, nothing leaves the machine).
//! `chatcraft --matrix https://matrix.example.org --user alice` signs in to a Matrix homeserver
//! with end-to-end encryption (password from `--password` or `CHATCRAFT_PASSWORD`, needed only
//! the first time on this device).
//! `chatcraft --server ws://host:7900 --handle ada --token TOKEN [--name "Ada Park"]` connects to
//! a `chatcraft-server`. `--dark` starts in dark mode.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use chatcraft_engine::Session;
use chatcraft_engine::proto::{ClientFrame, PROTOCOL_VERSION};
use chatcraft_ui_egui::ChatApp;

struct Args {
    matrix: Option<String>,
    user: String,
    password: String,
    server: Option<String>,
    handle: String,
    name: String,
    token: String,
    dark: bool,
}

const USAGE: &str = "usage: chatcraft [--demo]
                 [--matrix https://homeserver --user NAME [--password PW]]
                 [--server ws://host:port --handle NAME --token TOKEN [--name \"Display Name\"]]
                 [--dark]";

fn args() -> Result<Args, String> {
    let mut a = Args {
        matrix: None,
        user: String::new(),
        password: std::env::var("CHATCRAFT_PASSWORD").unwrap_or_default(),
        server: None,
        handle: String::new(),
        name: String::new(),
        token: std::env::var("CHATCRAFT_TOKEN").unwrap_or_default(),
        dark: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{x} needs a value\n{USAGE}"));
        match x.as_str() {
            "--matrix" => a.matrix = Some(val()?),
            "--user" => a.user = val()?,
            "--password" => a.password = val()?,
            "--server" => a.server = Some(val()?),
            "--handle" => a.handle = val()?,
            "--name" => a.name = val()?,
            "--token" => a.token = val()?,
            "--dark" => a.dark = true,
            "--demo" => a.server = None,
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

struct App(ChatApp);

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
    let session = match (&a.matrix, &a.server) {
        (Some(hs), _) => {
            let store = chatcraft_matrix::default_store_dir(hs, &a.user);
            match chatcraft_matrix::MatrixTransport::connect(chatcraft_matrix::MatrixConfig::new(hs, &a.user, &a.password, store)) {
                Ok(link) => Session::connect(Box::new(link), chatcraft_engine::Loopback::hello(&a.user)),
                Err(e) => {
                    eprintln!("cannot start Matrix: {e}");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
        (None, None) => Session::demo(),
        (None, Some(url)) => match chatcraft_net::NetTransport::connect(url) {
            Ok(link) => {
                let hello =
                    ClientFrame::Hello { version: PROTOCOL_VERSION, handle: a.handle.clone(), display_name: a.name.clone(), token: a.token.clone() };
                Session::connect(Box::new(link), hello)
            }
            Err(e) => {
                eprintln!("cannot start networking: {e}");
                return std::process::ExitCode::FAILURE;
            }
        },
    };
    let offset = chrono::Local::now().offset().local_minus_utc() / 60;
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("ChatCraft").with_inner_size([1360.0, 860.0]).with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    let r = eframe::run_native(
        "ChatCraft",
        opts,
        Box::new(move |cc| {
            let mut app = ChatApp::new(session);
            app.utc_offset_minutes = offset;
            app.set_dark(a.dark);
            let ctx = cc.egui_ctx.clone();
            app.session.set_waker(Box::new(move || ctx.request_repaint()));
            Ok(Box::new(App(app)))
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
