//! The window around a session: sign in, connect, chat, sign out.
//!
//! The UI crate doesn't know how to reach a homeserver (it also runs in the browser); the app
//! binary passes a [`Connect`] function that turns [`Credentials`] into a [`Session`].

use chatcraft_engine::{LinkStatus, Session};
use egui::{FontId, Frame, Margin, RichText, Ui, vec2};

use crate::{ChatApp, Tokens, icons};

/// What the sign-in screen collects. The password is only needed the first time on a device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credentials {
    pub homeserver: String,
    pub user: String,
    pub password: String,
}

/// Starts a session for credentials (it may still be signing in when it returns).
pub type Connect = Box<dyn FnMut(&Credentials) -> Result<Session, String>>;

/// How the window starts.
pub enum Start {
    /// The sign-in form, optionally prefilled.
    SignIn(Credentials),
    /// Signing in with these credentials right away (a remembered account: no password).
    Connect(Credentials),
    /// An existing session (the demo workspace, or one made by the caller).
    Session(Box<Session>),
}

struct Form {
    creds: Credentials,
    error: Option<String>,
}

enum Screen {
    SignIn(Form),
    Connecting { session: Box<Session>, creds: Credentials },
    Chat { app: Box<ChatApp>, creds: Option<Credentials> },
}

/// The ChatCraft window.
pub struct Shell {
    screen: Screen,
    connect: Connect,
    /// Called after a successful sign-in (to remember the account; never the password).
    pub on_signed_in: Option<Box<dyn FnMut(&Credentials)>>,
    /// Called after signing out (to forget the account).
    pub on_signed_out: Option<Box<dyn FnMut()>>,
    pub dark: bool,
    pub utc_offset_minutes: i32,
}

impl Shell {
    pub fn new(start: Start, connect: Connect) -> Shell {
        let mut shell = Shell {
            screen: Screen::SignIn(Form { creds: Credentials::default(), error: None }),
            connect,
            on_signed_in: None,
            on_signed_out: None,
            dark: false,
            utc_offset_minutes: 0,
        };
        shell.screen = match start {
            Start::SignIn(creds) => Screen::SignIn(Form { creds, error: None }),
            Start::Connect(creds) => shell.connecting(creds),
            Start::Session(s) => Screen::Chat { app: Box::new(ChatApp::new(*s)), creds: None },
        };
        shell
    }

    fn connecting(&mut self, creds: Credentials) -> Screen {
        match (self.connect)(&creds) {
            Ok(session) => Screen::Connecting { session: Box::new(session), creds },
            Err(e) => Screen::SignIn(Form { creds, error: Some(e) }),
        }
    }

    /// The chat window, once signed in.
    pub fn app(&mut self) -> Option<&mut ChatApp> {
        match &mut self.screen {
            Screen::Chat { app, .. } => Some(app),
            _ => None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let tokens = if self.dark { Tokens::dark() } else { Tokens::light() };
        let mut next: Option<Screen> = None;
        match &mut self.screen {
            Screen::SignIn(form) => {
                tokens.apply(&ctx);
                if let Some(action) = sign_in_form(ui, &tokens, form) {
                    next = Some(match action {
                        FormAction::SignIn => {
                            let creds = form.creds.clone();
                            self.connecting_screen(creds)
                        }
                        FormAction::Demo => Screen::Chat { app: Box::new(ChatApp::new(Session::demo())), creds: None },
                    });
                }
            }
            Screen::Connecting { session, creds } => {
                tokens.apply(&ctx);
                session.pump();
                if session.me.is_some() {
                    let mut creds = creds.clone();
                    creds.password.clear();
                    if let Some(f) = self.on_signed_in.as_mut() {
                        f(&creds);
                    }
                    let mut s = std::mem::replace(session, Box::new(Session::demo()));
                    let c = ctx.clone();
                    s.set_waker(Box::new(move || c.request_repaint()));
                    next = Some(Screen::Chat { app: Box::new(ChatApp::new(*s)), creds: Some(creds) });
                } else if let Some(e) = session.error.clone().or_else(|| match session.link_status() {
                    LinkStatus::Offline(why) if why.starts_with("sign-in failed") => Some(why),
                    _ => None,
                }) {
                    let mut creds = creds.clone();
                    creds.password.clear();
                    next = Some(Screen::SignIn(Form { creds, error: Some(e) }));
                } else {
                    connecting_ui(ui, &tokens, &creds.user, &creds.homeserver);
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
            Screen::Chat { app, creds } => {
                app.utc_offset_minutes = self.utc_offset_minutes;
                app.ui(ui);
                if app.st.signed_out {
                    if let Some(f) = self.on_signed_out.as_mut() {
                        f();
                    }
                    let mut c = creds.clone().unwrap_or_default();
                    c.password.clear();
                    next = Some(Screen::SignIn(Form { creds: c, error: None }));
                }
            }
        }
        if let Some(n) = next {
            self.screen = n;
            ctx.request_repaint();
        }
    }

    fn connecting_screen(&mut self, creds: Credentials) -> Screen {
        self.connecting(creds)
    }
}

enum FormAction {
    SignIn,
    Demo,
}

fn card(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui)) {
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, t.side_bg);
    let w = 420.0_f32.min(full.width() - 32.0);
    let rect = egui::Rect::from_center_size(full.center(), vec2(w, 10.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(egui::pos2(rect.left(), full.top() + (full.height() * 0.16).max(24.0)), vec2(w, full.height()))),
    );
    Frame::new().fill(t.main_bg).corner_radius(14).inner_margin(Margin::same(28)).show(&mut child, |ui| {
        ui.set_width(w - 56.0);
        add(ui);
    });
}

fn heading(ui: &mut Ui, t: &Tokens) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 9.0, t.side_active_bg);
        icons::paint(ui.painter(), r.shrink(8.0), icons::Icon::Chat, t.side_active_fg);
        ui.label(RichText::new("ChatCraft").font(FontId::proportional(26.0)).strong().color(t.strong));
    });
    ui.add_space(14.0);
}

fn sign_in_form(ui: &mut Ui, t: &Tokens, form: &mut Form) -> Option<FormAction> {
    let mut action = None;
    card(ui, t, |ui| {
        heading(ui, t);
        ui.label(RichText::new("Sign in with your Matrix account").strong().color(t.strong));
        ui.label(RichText::new("Any Matrix homeserver works. Private conversations are end-to-end encrypted.").color(t.muted));
        ui.add_space(12.0);
        // Each field is labelled for screen readers by its visible label.
        let field = |ui: &mut Ui, label: &str, value: &mut String, hint: &str, password: bool| {
            let l = ui.label(RichText::new(label).small().color(t.muted));
            let frame = Frame::new()
                .fill(t.composer_bg)
                .stroke(egui::Stroke::new(1.0, t.composer_border))
                .corner_radius(7)
                .inner_margin(Margin::symmetric(10, 7));
            ui.add(egui::TextEdit::singleline(value).hint_text(hint).password(password).desired_width(f32::INFINITY).frame(frame)).labelled_by(l.id)
        };
        if form.creds.homeserver.is_empty() {
            form.creds.homeserver = "matrix.org".into();
        }
        field(ui, "Homeserver", &mut form.creds.homeserver, "matrix.example.org", false);
        ui.add_space(6.0);
        let user = field(ui, "Username", &mut form.creds.user, "alice or @alice:example.org", false);
        if form.creds.user.is_empty() && !user.has_focus() {
            user.request_focus();
        }
        ui.add_space(6.0);
        let pw = field(ui, "Password", &mut form.creds.password, "", true);
        let enter = pw.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(e) = &form.error {
            ui.add_space(8.0);
            ui.label(RichText::new(e.as_str()).color(t.danger));
        }
        ui.add_space(14.0);
        let ready = !form.creds.homeserver.trim().is_empty() && !form.creds.user.trim().is_empty() && !form.creds.password.is_empty();
        let b = egui::Button::new(RichText::new("Sign in").strong().color(t.accent_fg))
            .fill(if ready { t.accent } else { t.faint })
            .corner_radius(7)
            .min_size(vec2(ui.available_width(), 38.0));
        if (ui.add_enabled(ready, b).clicked() || (enter && ready)) && action.is_none() {
            form.error = None;
            action = Some(FormAction::SignIn);
        }
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            if ui.link("Try the demo workspace instead").on_hover_text("A sample team that runs entirely on this computer").clicked() {
                action = Some(FormAction::Demo);
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new("No account? Create one on your homeserver's website, or ask whoever runs it.").small().color(t.muted));
    });
    action
}

fn connecting_ui(ui: &mut Ui, t: &Tokens, user: &str, homeserver: &str) {
    card(ui, t, |ui| {
        heading(ui, t);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(format!("Signing in as {user} on {homeserver}…")).color(t.text));
        });
        ui.add_space(6.0);
        ui.label(RichText::new("The first sign-in on a device sets up its encryption keys.").small().color(t.muted));
    });
}
