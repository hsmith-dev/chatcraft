//! ChatCraft's egui front end (L6). Swappable: nothing below depends on it.
//!
//! The UI is thin. Panels read the engine's [`Session`] and push [`Act`]s; after drawing,
//! [`ChatApp::ui`] applies them, almost always as `session.run(command id, params)`, so a click,
//! a shortcut, the CLI and an agent all take the same path. UI-only commands (`ui.*`: dialogs,
//! theme) are listed in [`UI_COMMANDS`]. Colours come from [`theme::Tokens`]; icons are drawn in
//! code ([`icons`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod avatar;
mod chrome;
mod dialogs;
pub mod icons;
mod keys;
mod markup;
mod pane;
mod sas;
mod shell;
mod sidebar;
pub mod theme;
mod timeline;

use chatcraft_engine::Session;
use chatcraft_engine::model::{ChannelId, MessageId, Ts, UserId};
use egui::{Color32, Frame, Margin, Stroke, Ui};
use serde_json::{Value, json};

pub use shell::{Connect, Credentials, Shell, Start};
pub use theme::{Mode, Tokens};

/// UI-only commands: `(id, label, shortcut)`.
pub const UI_COMMANDS: &[(&str, &str, &str)] = &[
    ("ui.quickSwitcher", "Jump to a conversation or person", "Mod+K"),
    ("ui.newChannel", "Create a channel", "Mod+Shift+N"),
    ("ui.browseChannels", "Browse channels", "Mod+Shift+O"),
    ("ui.newMessage", "New direct message", "Mod+N"),
    ("ui.setStatus", "Set a status", "Mod+Shift+Y"),
    ("ui.people", "People directory", "Mod+Shift+E"),
    ("ui.toggleTheme", "Switch light / dark", "Mod+Shift+L"),
    ("ui.shortcuts", "Keyboard shortcuts", "Mod+/"),
    ("ui.focusSearch", "Search", "Mod+F"),
    ("ui.recovery", "Encryption and key backup", "Mod+Shift+R"),
    ("ui.signOut", "Sign out", ""),
];

/// What the emoji picker inserts into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickFor {
    React(MessageId),
    Composer(ChannelId),
    Thread(MessageId),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Dialog {
    QuickSwitcher {
        query: String,
        selected: usize,
    },
    CreateChannel {
        name: String,
        purpose: String,
        private: bool,
        error: Option<String>,
    },
    NewMessage {
        query: String,
        picked: Vec<UserId>,
    },
    Browse {
        query: String,
        /// The homeserver's public room directory (Matrix): results, the request in flight, the
        /// query last asked, and whether this account has a directory at all.
        directory: Option<Vec<Value>>,
        waiting: Option<u64>,
        asked: Option<String>,
        unavailable: bool,
    },
    People {
        query: String,
    },
    Status {
        text: String,
        emoji: String,
        minutes: Option<u32>,
    },
    Topic {
        channel: ChannelId,
        text: String,
    },
    Shortcuts,
    /// An identity verification (by flow id).
    Verify {
        flow: String,
    },
    /// Key backup: set up (shows the new recovery key) or restore with one.
    Recovery {
        key: Option<String>,
        input: String,
        waiting: Option<(u64, RecoveryStep)>,
        message: Option<(String, bool)>,
    },
    SignOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecoveryStep {
    SetUp,
    Restore,
}

/// Things panels ask for while drawing; applied once the frame is drawn.
#[derive(Clone, Debug)]
pub(crate) enum Act {
    Run(&'static str, Value),
    Open(ChannelId),
    OpenAt(ChannelId, MessageId),
    Thread(MessageId),
    Profile(UserId),
    ProfileHandle(String),
    ChannelName(String),
    Edit(MessageId, String),
    SaveEdit(MessageId, String),
    CancelEdit,
    Picker(PickFor, egui::Pos2),
    Menu(MessageId, egui::Pos2),
    CloseMenus,
    Dialog(Dialog),
    CloseDialog,
    Draft(ChannelId, String),
    ThreadDraft(MessageId, String),
    Send(ChannelId),
    Reply(MessageId),
    Search(String),
    Copy(String),
    Ui(&'static str),
}

#[derive(Default)]
pub(crate) struct UiState {
    pub dialog: Option<Dialog>,
    pub editing: Option<(MessageId, String)>,
    pub picker: Option<(PickFor, egui::Pos2)>,
    pub picker_query: String,
    pub menu: Option<(MessageId, egui::Pos2)>,
    pub profile_menu: Option<egui::Pos2>,
    pub add_menu: Option<egui::Pos2>,
    pub toasts: Vec<(String, bool, f64, Option<(ChannelId, MessageId)>)>,
    pub search_box: String,
    pub focus_search: bool,
    pub focus_composer: bool,
    pub channels_closed: bool,
    pub dms_closed: bool,
    pub also_send: bool,
    pub title: String,
    pub applied: Option<Mode>,
    /// Last frame's hover action bar, so the row under it stays hovered while the pointer is on it.
    pub hover_bar: Option<(MessageId, egui::Rect)>,
    pub next_hover_bar: Option<(MessageId, egui::Rect)>,
    /// The last verification flow the dialog was opened for.
    pub verify_shown: Option<String>,
    /// Sign-out was asked for; set once it's done (the shell returns to sign-in).
    pub signing_out: bool,
    pub signed_out: bool,
}

/// The ChatCraft window.
pub struct ChatApp {
    pub session: Session,
    pub tokens: Tokens,
    /// The viewer's offset from UTC, in minutes (the app binary fills it in).
    pub utc_offset_minutes: i32,
    pub(crate) st: UiState,
}

impl ChatApp {
    pub fn new(session: Session) -> ChatApp {
        let mut app = ChatApp { session, tokens: Tokens::light(), utc_offset_minutes: 0, st: UiState::default() };
        app.st.focus_composer = true;
        app
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.tokens = if dark { Tokens::dark() } else { Tokens::light() };
    }

    /// Runs a UI-only command (`ui.*`) or an engine command by id.
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        match id {
            "ui.quickSwitcher" => self.st.dialog = Some(Dialog::QuickSwitcher { query: String::new(), selected: 0 }),
            "ui.newChannel" => {
                self.st.dialog = Some(Dialog::CreateChannel { name: String::new(), purpose: String::new(), private: false, error: None })
            }
            "ui.browseChannels" => {
                self.st.dialog = Some(Dialog::Browse { query: String::new(), directory: None, waiting: None, asked: None, unavailable: false });
            }
            "ui.newMessage" => self.st.dialog = Some(Dialog::NewMessage { query: String::new(), picked: Vec::new() }),
            "ui.people" => self.st.dialog = Some(Dialog::People { query: String::new() }),
            "ui.setStatus" => {
                let cur = self.session.me.and_then(|m| self.session.state.user(m)).map(|u| u.status.clone()).unwrap_or_default();
                self.st.dialog = Some(Dialog::Status { text: cur.text, emoji: cur.emoji, minutes: None });
            }
            "ui.toggleTheme" => self.set_dark(self.tokens.mode == Mode::Light),
            "ui.shortcuts" => self.st.dialog = Some(Dialog::Shortcuts),
            "ui.focusSearch" => self.st.focus_search = true,
            "ui.recovery" => self.st.dialog = Some(Dialog::Recovery { key: None, input: String::new(), waiting: None, message: None }),
            "ui.closeDialog" => self.st.dialog = None,
            "ui.signOut" => self.st.dialog = Some(Dialog::SignOut),
            _ if id.starts_with("ui.") => return Err(format!("unknown UI command `{id}`")),
            _ => {
                let r = self.session.run(id, params).map_err(|e| e.to_string());
                if let Err(e) = &r {
                    self.session.notice(e.clone(), true);
                }
                return r;
            }
        }
        Ok(Value::Null)
    }

    pub(crate) fn local(&self, ts: Ts) -> Ts {
        ts.plus_millis(i64::from(self.utc_offset_minutes) * 60_000)
    }

    /// Draws the whole window into `ui` and applies what the person did.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.st.applied != Some(self.tokens.mode) {
            self.tokens.apply(&ctx);
            self.st.applied = Some(self.tokens.mode);
        }
        self.session.focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        self.session.pump();
        if self.st.signing_out && matches!(self.session.link_status(), chatcraft_engine::LinkStatus::Offline(ref w) if w == "signed out") {
            self.st.signed_out = true;
        }
        self.collect_toasts(&ctx);
        // A new verification (theirs or ours) opens its dialog.
        if let Some(v) = self.session.verifications.last()
            && !v.stage.finished()
            && self.st.verify_shown.as_deref() != Some(v.flow.as_str())
        {
            self.st.verify_shown = Some(v.flow.clone());
            self.st.dialog = Some(Dialog::Verify { flow: v.flow.clone() });
        }
        let mut acts: Vec<Act> = Vec::new();
        self.st.hover_bar = self.st.next_hover_bar.take();
        keys::handle(self, &ctx, &mut acts);

        let t = self.tokens.clone();
        egui::Panel::top("top")
            .exact_size(44.0)
            .resizable(false)
            .frame(Frame::new().fill(t.top_bg).inner_margin(Margin::symmetric(10, 6)))
            .show(ui, |ui| chrome::top_bar(self, ui, &mut acts));
        egui::Panel::left("rail")
            .exact_size(68.0)
            .resizable(false)
            .frame(Frame::new().fill(t.rail_bg).inner_margin(Margin::symmetric(8, 10)))
            .show(ui, |ui| chrome::rail(self, ui, &mut acts));
        egui::Panel::left("sidebar")
            .default_size(268.0)
            .size_range(200.0..=440.0)
            .resizable(true)
            .frame(Frame::new().fill(t.side_bg).inner_margin(Margin::symmetric(0, 6)))
            .show(ui, |ui| sidebar::show(self, ui, &mut acts));
        if self.session.view.pane != chatcraft_engine::Pane::None {
            egui::Panel::right("pane")
                .default_size(400.0)
                .size_range(300.0..=680.0)
                .resizable(true)
                .frame(Frame::new().fill(t.pane_bg).stroke(Stroke::new(1.0, t.border)))
                .show(ui, |ui| pane::show(self, ui, &mut acts));
        }
        egui::CentralPanel::no_frame().frame(Frame::new().fill(t.main_bg)).show(ui, |ui| timeline::show(self, ui, &mut acts));

        dialogs::show(self, &ctx, &mut acts);
        chrome::popups(self, &ctx, &mut acts);
        chrome::toasts(self, &ctx, &mut acts);
        self.apply(acts, &ctx);
        self.update_title(&ctx);
        if !self.st.toasts.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn collect_toasts(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        while let Some(n) = self.session.notices.pop_front() {
            self.st.toasts.push((n.text, n.error, now + 4.5, None));
        }
        while let Some(a) = self.session.alerts.pop_front() {
            let visible = self.session.focused && self.session.view.channel == Some(a.channel);
            if !visible {
                self.st.toasts.push((format!("{}: {}", a.title, a.body), false, now + 6.0, Some((a.channel, a.message))));
            }
        }
        self.st.toasts.retain(|t| t.2 > now);
        let n = self.st.toasts.len();
        if n > 4 {
            self.st.toasts.drain(..n - 4);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let ch = self.session.view.channel.map(|c| self.session.state.title(c, self.session.me)).unwrap_or_default();
        let ws = &self.session.state.workspace.name;
        let unread: usize = self.session.sidebar().order().filter(|c| !c.muted).map(|c| c.mentions).sum();
        let badge = if unread > 0 { format!("({unread}) ") } else { String::new() };
        let title = if ch.is_empty() { format!("{badge}ChatCraft") } else { format!("{badge}{ch} — {ws} — ChatCraft") };
        if title != self.st.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.st.title = title;
        }
    }

    #[allow(clippy::cognitive_complexity)]
    fn apply(&mut self, acts: Vec<Act>, ctx: &egui::Context) {
        for a in acts {
            match a {
                Act::Run(id, v) => {
                    let _ = self.run(id, &v);
                }
                Act::Ui(id) => {
                    let _ = self.run(id, &Value::Null);
                }
                Act::Open(c) => {
                    let _ = self.run("nav.channel", &json!({ "channel": c.to_string() }));
                    self.st.focus_composer = true;
                }
                Act::OpenAt(c, m) => {
                    let _ = self.run("nav.channel", &json!({ "channel": c.to_string(), "message": m.0 }));
                }
                Act::Thread(m) => {
                    let _ = self.run("thread.open", &json!({ "message": m.0 }));
                }
                Act::Profile(u) => {
                    let _ = self.run("nav.profile", &json!({ "user": u.to_string() }));
                }
                Act::ProfileHandle(h) => {
                    let _ = self.run("nav.profile", &json!({ "user": format!("@{h}") }));
                }
                Act::ChannelName(n) => match self.session.state.channel_by_name(&n).map(|c| c.id) {
                    Some(c) => {
                        let _ = self.run("nav.channel", &json!({ "channel": c.to_string() }));
                    }
                    None => self.session.notice(format!("#{n} doesn't exist, or you can't see it"), true),
                },
                Act::Edit(m, text) => self.st.editing = Some((m, text)),
                Act::SaveEdit(m, text) => {
                    if self.run("message.edit", &json!({ "message": m.0, "text": text })).is_ok() {
                        self.st.editing = None;
                    }
                }
                Act::CancelEdit => self.st.editing = None,
                Act::Picker(f, pos) => {
                    self.st.picker = if self.st.picker.map(|p| p.0) == Some(f) { None } else { Some((f, pos)) };
                    self.st.picker_query.clear();
                    self.st.menu = None;
                }
                Act::Menu(m, pos) => {
                    self.st.menu = if self.st.menu.map(|x| x.0) == Some(m) { None } else { Some((m, pos)) };
                    self.st.picker = None;
                }
                Act::CloseMenus => {
                    self.st.menu = None;
                    self.st.picker = None;
                    self.st.profile_menu = None;
                    self.st.add_menu = None;
                }
                Act::Dialog(d) => self.st.dialog = Some(d),
                Act::CloseDialog => self.st.dialog = None,
                Act::Draft(c, text) => {
                    let _ = self.session.run("composer.setDraft", &json!({ "channel": c.to_string(), "text": text }));
                }
                Act::ThreadDraft(m, text) => {
                    if text.is_empty() {
                        self.session.thread_drafts.remove(&m);
                    } else {
                        self.session.thread_drafts.insert(m, text);
                    }
                }
                Act::Send(c) => {
                    let text = self.session.drafts.get(&c).cloned().unwrap_or_default();
                    if !text.trim().is_empty() {
                        let _ = self.run("message.send", &json!({ "channel": c.to_string(), "text": text }));
                    }
                }
                Act::Reply(root) => {
                    let text = self.session.thread_drafts.get(&root).cloned().unwrap_or_default();
                    if !text.trim().is_empty() {
                        let _ = self.run("message.reply", &json!({ "message": root.0, "text": text, "broadcast": self.st.also_send }));
                        self.st.also_send = false;
                    }
                }
                Act::Search(q) => {
                    let _ = self.run("search.messages", &json!({ "query": q }));
                }
                Act::Copy(text) => {
                    ctx.copy_text(text);
                    self.session.notice("Copied to the clipboard", false);
                }
            }
        }
    }
}

/// A small pill (unread mention counts).
pub(crate) fn badge(ui: &mut Ui, n: usize, bg: Color32, fg: Color32) {
    let text = if n > 99 { "99+".to_string() } else { n.to_string() };
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::proportional(11.0), fg);
    let size = egui::vec2((galley.size().x + 12.0).max(20.0), 18.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 9.0, bg);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
}

#[cfg(test)]
mod tests;
