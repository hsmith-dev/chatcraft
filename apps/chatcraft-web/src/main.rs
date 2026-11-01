//! ChatCraft in the browser: the same `Shell` and `ChatApp` as the desktop, through eframe's web
//! runner, signing in to a Matrix homeserver from the page (IndexedDB store, end-to-end
//! encryption in the browser) or opening the demo workspace.
//!
//! - `?demo` opens the demo workspace.
//! - `#homeserver=matrix.example.org&user=alice` prefills the sign-in form (never a password).
//! - Single sign-on leaves the page for the homeserver's sign-in page, which comes back with
//!   `?loginToken=…`; the account being signed in to waits in `sessionStorage` meanwhile.
//! - A signed-in account is remembered in this browser (`localStorage`: homeserver and user name;
//!   the device's session and store passphrase are kept there too, as other web clients do).
//!
//! Build with `trunk build --release` from this directory (output in `dist/web`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web {
    use chatcraft_engine::{Loopback, Session};
    use std::cell::RefCell;
    use std::rc::Rc;

    use chatcraft_ui_egui::{Credentials, Shell, SsoPoll, Start};
    use wasm_bindgen::JsCast;

    const ACCOUNT: &str = "chatcraft:account";
    /// The account a single sign-on is for, while the page is away at the identity provider.
    const SSO_PENDING: &str = "chatcraft:sso";

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
        account_from(&storage()?.get_item(ACCOUNT).ok()??)
    }

    fn account_from(json: &str) -> Option<Credentials> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        Some(Credentials { homeserver: v.get("homeserver")?.as_str()?.into(), user: v.get("user")?.as_str()?.into(), ..Credentials::default() })
    }

    fn account_json(c: &Credentials) -> String {
        serde_json::json!({ "homeserver": c.homeserver, "user": c.user }).to_string()
    }

    fn session_storage() -> Option<web_sys::Storage> {
        web_sys::window()?.session_storage().ok()?
    }

    /// This page's address without its query or fragment (where single sign-on comes back to).
    fn page_address() -> Option<String> {
        let l = web_sys::window()?.location();
        Some(format!("{}{}", l.origin().ok()?, l.pathname().ok()?))
    }

    /// Back from single sign-on: the token, and the account it is for. The token is removed from
    /// the address bar (and so from history) straight away.
    fn returning_from_sso(search: &str) -> Option<Credentials> {
        let token = chatcraft_matrix::sso::login_token_from_query(search)?;
        if let (Some(w), Some(addr)) = (web_sys::window(), page_address())
            && let Ok(h) = w.history()
        {
            let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&addr));
        }
        let store = session_storage()?;
        let pending = store.get_item(SSO_PENDING).ok()??;
        let _ = store.remove_item(SSO_PENDING);
        Some(Credentials { login_token: token, ..account_from(&pending)? })
    }

    /// Single sign-on in this tab: fetch the homeserver's SSO page, remember which account it is
    /// for, and go there.
    fn start_sso(c: &Credentials) -> Result<Box<dyn FnMut() -> SsoPoll>, String> {
        let back = page_address().ok_or("no page address")?;
        session_storage()
            .and_then(|s| s.set_item(SSO_PENDING, &account_json(c)).ok())
            .ok_or("this browser doesn't allow session storage, which single sign-on needs")?;
        let result: Rc<RefCell<Option<Result<String, String>>>> = Rc::new(RefCell::new(None));
        let (slot, hs) = (Rc::clone(&result), c.homeserver.trim().to_string());
        wasm_bindgen_futures::spawn_local(async move {
            let r = chatcraft_matrix::sso::sso_login_url(&hs, &back).await;
            *slot.borrow_mut() = Some(r);
        });
        let mut gone = false;
        Ok(Box::new(move || match result.borrow().as_ref() {
            None => SsoPoll::Waiting,
            Some(Ok(url)) => {
                if !gone {
                    gone = true;
                    if let Some(w) = web_sys::window() {
                        let _ = w.location().set_href(url);
                    }
                }
                SsoPoll::Waiting
            }
            Some(Err(e)) => {
                if let Some(s) = session_storage() {
                    let _ = s.remove_item(SSO_PENDING);
                }
                SsoPoll::Failed(e.clone())
            }
        }))
    }

    fn remember(c: &Credentials) {
        if let Some(s) = storage() {
            let _ = s.set_item(ACCOUNT, &account_json(c));
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
        if let Some(c) = returning_from_sso(&search) {
            return Start::Connect(c);
        }
        if let Some(c) = remembered() {
            return Start::Connect(c);
        }
        let hash = location.as_ref().and_then(|l| l.hash().ok()).unwrap_or_default();
        let params = web_sys::UrlSearchParams::new_with_str(hash.trim_start_matches('#')).ok();
        let get = |k: &str| params.as_ref().and_then(|p| p.get(k)).unwrap_or_default();
        Start::SignIn(Credentials { homeserver: get("homeserver"), user: get("user"), ..Credentials::default() })
    }

    fn connect(c: &Credentials) -> Result<Session, String> {
        let mut cfg = chatcraft_matrix::MatrixConfig::new(&c.homeserver, &c.user, &c.password, std::path::PathBuf::new());
        cfg.login_token = c.login_token.clone();
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
                        shell.sso = Some(Box::new(start_sso));
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
