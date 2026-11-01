//! ChatCraft in the browser: the same `Shell` and `ChatApp` as the desktop, through eframe's web
//! runner, signing in to a Matrix homeserver from the page (IndexedDB store, end-to-end
//! encryption in the browser) or opening the demo workspace.
//!
//! - `?demo` opens the demo workspace.
//! - `#homeserver=matrix.example.org&user=alice` prefills the sign-in form (never a password).
//! - A signed-in account is remembered in this browser (`localStorage`: homeserver and user name;
//!   the device's session and store passphrase are kept there too, as other web clients do).
//!
//! Build with `trunk build --release` from this directory (output in `dist/web`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web {
    use chatcraft_engine::{Loopback, Session};
    use chatcraft_ui_egui::{Credentials, Shell, Start};
    use wasm_bindgen::JsCast;

    const ACCOUNT: &str = "chatcraft:account";

    struct App(Shell);

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.ui(ui);
        }
    }

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    fn remembered() -> Option<Credentials> {
        let v: serde_json::Value = serde_json::from_str(&storage()?.get_item(ACCOUNT).ok()??).ok()?;
        Some(Credentials { homeserver: v.get("homeserver")?.as_str()?.into(), user: v.get("user")?.as_str()?.into(), password: String::new() })
    }

    fn remember(c: &Credentials) {
        if let Some(s) = storage() {
            let _ = s.set_item(ACCOUNT, &serde_json::json!({ "homeserver": c.homeserver, "user": c.user }).to_string());
        }
    }

    fn forget() {
        if let Some(s) = storage() {
            let _ = s.remove_item(ACCOUNT);
        }
    }

    /// `?demo`, and `#homeserver=…&user=…` for a prefilled form.
    fn start_from_url() -> Start {
        let location = web_sys::window().map(|w| w.location());
        let search = location.as_ref().and_then(|l| l.search().ok()).unwrap_or_default();
        if search.trim_start_matches('?').split('&').any(|p| p == "demo") {
            return Start::Session(Box::new(Session::demo()));
        }
        if let Some(c) = remembered() {
            return Start::Connect(c);
        }
        let hash = location.as_ref().and_then(|l| l.hash().ok()).unwrap_or_default();
        let params = web_sys::UrlSearchParams::new_with_str(hash.trim_start_matches('#')).ok();
        let get = |k: &str| params.as_ref().and_then(|p| p.get(k)).unwrap_or_default();
        Start::SignIn(Credentials { homeserver: get("homeserver"), user: get("user"), password: String::new() })
    }

    fn connect(c: &Credentials) -> Result<Session, String> {
        let cfg = chatcraft_matrix::MatrixConfig::new(&c.homeserver, &c.user, &c.password, std::path::PathBuf::new());
        let link = chatcraft_matrix::MatrixTransport::connect(cfg)?;
        Ok(Session::connect(Box::new(link), Loopback::hello(&c.user)))
    }

    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            let Some(canvas) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("chatcraft_canvas"))
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                log::error!("no #chatcraft_canvas");
                return;
            };
            // getTimezoneOffset is minutes *behind* UTC.
            let offset = -(js_sys::Date::new_0().get_timezone_offset() as i32);
            let r = eframe::WebRunner::new()
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(move |_cc| {
                        let mut shell = Shell::new(start_from_url(), Box::new(connect));
                        shell.on_signed_in = Some(Box::new(remember));
                        shell.on_signed_out = Some(Box::new(forget));
                        shell.utc_offset_minutes = offset;
                        Ok(Box::new(App(shell)))
                    }),
                )
                .await;
            if let Err(e) = r {
                log::error!("ChatCraft failed to start: {e:?}");
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("chatcraft-web only runs in the browser: build it with `trunk build --release` in apps/chatcraft-web");
}
