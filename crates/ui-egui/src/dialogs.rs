//! Modal dialogs.

use chatcraft_engine::Session;
use chatcraft_engine::model::{ChannelId, UserId};
use chatcraft_engine::text::emoji;
use egui::{FontId, Frame, Id, Key, Margin, RichText, Sense, Stroke, Ui, vec2};
use serde_json::json;

use chatcraft_engine::proto::VerifyStage;

use crate::icons::{self, Icon};
use crate::{Act, ChatApp, Dialog, RecoveryStep, UI_COMMANDS, avatar, sas};

/// A switcher entry.
#[derive(Clone)]
enum Hit {
    Channel(ChannelId, String),
    Person(UserId, String),
}

/// Subsequence score: higher is better, `None` if `q` doesn't match.
fn fuzzy(hay: &str, q: &str) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    let h = hay.to_lowercase();
    if h.starts_with(q) {
        return Some(100 - i32::try_from(h.len()).unwrap_or(99));
    }
    if h.contains(q) {
        return Some(50 - i32::try_from(h.len()).unwrap_or(49));
    }
    let mut it = h.chars();
    q.chars().all(|c| it.any(|x| x == c)).then_some(10)
}

fn switcher_hits(app: &ChatApp, query: &str) -> Vec<Hit> {
    let s = &app.session;
    let q = query.trim().trim_start_matches(['#', '@']).to_lowercase();
    let Some(me) = s.me else { return Vec::new() };
    let mut scored: Vec<(i32, Hit)> = Vec::new();
    for c in s.state.channels.values().filter(|c| s.state.can_see(me, c.id) && !c.archived) {
        let title = s.state.title(c.id, s.me);
        if let Some(sc) = fuzzy(title.trim_start_matches('#'), &q) {
            let bonus = if c.members.contains(&me) { 5 } else { 0 } + if s.state.unread(me, c.id).0 > 0 { 3 } else { 0 };
            scored.push((sc + bonus, Hit::Channel(c.id, title)));
        }
    }
    for u in s.state.users.values().filter(|u| !u.deactivated && u.id != me) {
        let has_dm = s.state.dm_with(&[me, u.id].into_iter().collect()).is_some();
        if has_dm {
            continue;
        }
        if let Some(sc) = fuzzy(u.name(), &q).or_else(|| fuzzy(&u.handle, &q)) {
            scored.push((sc - 2, Hit::Person(u.id, u.name().to_string())));
        }
    }
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    scored.into_iter().take(9).map(|(_, h)| h).collect()
}

fn frame(app: &ChatApp) -> Frame {
    let t = &app.tokens;
    Frame::new().fill(t.main_bg).stroke(Stroke::new(1.0, t.border)).corner_radius(12).inner_margin(Margin::same(20)).shadow(egui::Shadow {
        offset: [0, 8],
        blur: 32,
        spread: 0,
        color: t.shadow,
    })
}

fn heading(ui: &mut Ui, app: &ChatApp, text: &str) {
    ui.label(RichText::new(text).font(FontId::proportional(20.0)).strong().color(app.tokens.strong));
    ui.add_space(10.0);
}

fn primary(ui: &mut Ui, app: &ChatApp, label: &str, enabled: bool) -> bool {
    let t = &app.tokens;
    let b = egui::Button::new(RichText::new(label).color(t.accent_fg).strong())
        .fill(if enabled { t.accent } else { t.faint })
        .corner_radius(6)
        .min_size(vec2(96.0, 32.0));
    ui.add_enabled(enabled, b).clicked()
}

#[allow(clippy::cognitive_complexity)]
pub(crate) fn show(app: &mut ChatApp, ctx: &egui::Context, acts: &mut Vec<Act>) {
    let Some(mut dialog) = app.st.dialog.clone() else { return };
    let t = app.tokens.clone();
    let mut close = false;
    let resp =
        egui::Modal::new(Id::unique("dialog")).backdrop_color(egui::Color32::from_black_alpha(110)).frame(frame(app)).show(ctx, |ui| match &mut dialog {
            Dialog::QuickSwitcher { query, selected } => {
                ui.set_width(520.0);
                heading(ui, app, "Jump to…");
                let r = ui.add(
                    egui::TextEdit::singleline(query)
                        .hint_text("Conversations and people")
                        .desired_width(f32::INFINITY)
                        .font(FontId::proportional(16.0)),
                );
                r.request_focus();
                let hits = switcher_hits(app, query);
                let n = hits.len();
                if ui.input(|i| i.key_pressed(Key::ArrowDown)) && n > 0 {
                    *selected = (*selected + 1) % n;
                }
                if ui.input(|i| i.key_pressed(Key::ArrowUp)) && n > 0 {
                    *selected = (*selected + n - 1) % n;
                }
                if r.changed() {
                    *selected = 0;
                }
                let enter = ui.input(|i| i.key_pressed(Key::Enter));
                ui.add_space(8.0);
                for (i, h) in hits.iter().enumerate() {
                    let label = match h {
                        Hit::Channel(_, title) => title.clone(),
                        Hit::Person(_, name) => format!("{name} — start a conversation"),
                    };
                    let sel = i == *selected;
                    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                    if sel || row.hovered() {
                        ui.painter().rect_filled(rect, 6.0, if sel { t.accent } else { t.row_hover });
                    }
                    let fg = if sel { t.accent_fg } else { t.text };
                    ui.painter().text(rect.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, label, FontId::proportional(15.0), fg);
                    if row.clicked() || (enter && sel) {
                        match h {
                            Hit::Channel(c, _) => acts.push(Act::Open(*c)),
                            Hit::Person(u, _) => acts.push(Act::Run("dm.open", json!({ "users": [u.to_string()] }))),
                        }
                        close = true;
                    }
                }
                if hits.is_empty() {
                    ui.label(RichText::new("No matches").color(t.muted));
                }
            }
            Dialog::CreateChannel { name, purpose, private, error } => {
                ui.set_width(460.0);
                heading(ui, app, "Create a channel");
                ui.label(RichText::new("Name").strong().color(t.text));
                let r = ui.add(egui::TextEdit::singleline(name).hint_text("e.g. plan-budget").desired_width(f32::INFINITY));
                if name.is_empty() {
                    r.request_focus();
                }
                let normalized = chatcraft_engine::core::normalize_channel_name(name).map_err(|e| e.to_string());
                if !name.is_empty() {
                    ui.label(RichText::new(normalized.clone().map(|n| format!("#{n}")).unwrap_or_else(|e| e)).small().color(t.muted));
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Description (optional)").strong().color(t.text));
                ui.add(egui::TextEdit::singleline(purpose).hint_text("What's this channel about?").desired_width(f32::INFINITY));
                ui.add_space(8.0);
                ui.checkbox(private, "Make private — only people you invite can see it");
                if let Some(e) = error {
                    ui.label(RichText::new(e.as_str()).color(t.danger));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let ok = normalized.is_ok();
                    if primary(ui, app, "Create", ok) || (ok && ui.input(|i| i.key_pressed(Key::Enter))) {
                        let r = app.session.run("channel.create", &json!({ "name": name, "purpose": purpose, "private": *private }));
                        match r {
                            Ok(_) => close = true,
                            Err(e) => *error = Some(e.to_string()),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::NewMessage { query, picked } => {
                ui.set_width(480.0);
                heading(ui, app, "New message");
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("To:").color(t.muted));
                    let mut remove = None;
                    for u in picked.iter() {
                        let name = app.session.state.user(*u).map(|x| x.name().to_string()).unwrap_or_default();
                        if ui
                            .add(egui::Button::new(RichText::new(format!("{name}  ×")).color(t.mention_fg)).fill(t.mention_bg).corner_radius(10))
                            .clicked()
                        {
                            remove = Some(*u);
                        }
                    }
                    if let Some(u) = remove {
                        picked.retain(|x| *x != u);
                    }
                });
                let r = ui.add(egui::TextEdit::singleline(query).hint_text("Type a name").desired_width(f32::INFINITY));
                r.request_focus();
                let me = app.session.me.unwrap_or_default();
                let q = query.trim().trim_start_matches('@').to_lowercase();
                let people: Vec<_> = app
                    .session
                    .state
                    .users
                    .values()
                    .filter(|u| {
                        !u.deactivated && !picked.contains(&u.id) && (q.is_empty() || u.handle.contains(&q) || u.name().to_lowercase().contains(&q))
                    })
                    .cloned()
                    .collect();
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for u in &people {
                        let r = ui.horizontal(|ui| {
                            avatar::show(ui, 26.0, Some(u), &t, Some(t.main_bg));
                            ui.label(RichText::new(u.name()).color(t.text));
                            if u.id == me {
                                ui.label(RichText::new("(you)").color(t.muted));
                            }
                            ui.label(RichText::new(format!("@{}", u.handle)).small().color(t.muted));
                        });
                        if r.response.interact(Sense::click()).clicked() {
                            picked.push(u.id);
                            query.clear();
                        }
                    }
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if primary(ui, app, "Go", !picked.is_empty()) {
                        acts.push(Act::Run("dm.open", json!({ "users": picked.iter().map(ToString::to_string).collect::<Vec<_>>() })));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::Browse { query } => {
                ui.set_width(560.0);
                heading(ui, app, "Browse channels");
                let r = ui.add(egui::TextEdit::singleline(query).hint_text("Search by name or description").desired_width(f32::INFINITY));
                r.request_focus();
                let list = app.session.run("channel.browse", &json!({ "query": query })).unwrap_or_default();
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for c in list.as_array().into_iter().flatten() {
                        let id = c["id"].as_u64().map(ChannelId).unwrap_or_default();
                        let joined = c["joined"].as_bool().unwrap_or(false);
                        Frame::new().inner_margin(Margin::symmetric(4, 6)).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    let lock = if c["private"].as_bool().unwrap_or(false) { "🔒 " } else { "#" };
                                    ui.label(RichText::new(format!("{lock}{}", c["name"].as_str().unwrap_or_default())).strong().color(t.strong));
                                    let members = c["members"].as_u64().unwrap_or(0);
                                    let extra = if joined { " · Joined" } else { "" };
                                    let purpose = c["purpose"].as_str().unwrap_or_default();
                                    ui.label(RichText::new(format!("{members} members{extra} · {purpose}")).small().color(t.muted));
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if joined {
                                        if ui.button("Open").clicked() {
                                            acts.push(Act::Open(id));
                                            close = true;
                                        }
                                    } else {
                                        if primary(ui, app, "Join", true) {
                                            acts.push(Act::Run("channel.join", json!({ "channel": id.to_string() })));
                                            close = true;
                                        }
                                        if ui.button("View").clicked() {
                                            acts.push(Act::Open(id));
                                            close = true;
                                        }
                                    }
                                });
                            });
                        });
                        ui.separator();
                    }
                });
            }
            Dialog::People { query } => {
                ui.set_width(560.0);
                heading(ui, app, "People");
                let r = ui.add(egui::TextEdit::singleline(query).hint_text("Search by name or handle").desired_width(f32::INFINITY));
                r.request_focus();
                let q = query.trim().trim_start_matches('@').to_lowercase();
                let people: Vec<_> = app
                    .session
                    .state
                    .users
                    .values()
                    .filter(|u| !u.deactivated && (q.is_empty() || u.handle.contains(&q) || u.name().to_lowercase().contains(&q)))
                    .cloned()
                    .collect();
                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                    egui::Grid::new("people").num_columns(3).spacing(vec2(12.0, 12.0)).show(ui, |ui| {
                        for (i, u) in people.iter().enumerate() {
                            let r = ui.vertical(|ui| {
                                ui.set_width(160.0);
                                avatar::show(ui, 56.0, Some(u), &t, Some(t.main_bg));
                                ui.label(RichText::new(u.name()).strong().color(t.strong));
                                ui.label(RichText::new(&u.title).small().color(t.muted));
                            });
                            if r.response.interact(Sense::click()).clicked() {
                                acts.push(Act::Profile(u.id));
                                close = true;
                            }
                            if i % 3 == 2 {
                                ui.end_row();
                            }
                        }
                    });
                });
            }
            Dialog::Status { text, emoji: em, minutes } => {
                ui.set_width(460.0);
                heading(ui, app, "Set a status");
                ui.horizontal(|ui| {
                    let g = emoji::lookup(em).unwrap_or("😄");
                    ui.label(RichText::new(g).font(FontId::proportional(22.0)));
                    ui.add(egui::TextEdit::singleline(text).hint_text("What's your status?").desired_width(f32::INFINITY));
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Suggestions").small().color(t.muted));
                for (e, label, mins) in [
                    ("calendar", "In a meeting", Some(60)),
                    ("house_with_garden", "Working remotely", None),
                    ("headphones", "Focus time", Some(120)),
                    ("cry", "Out sick", None),
                    ("palm_tree", "Vacationing", None),
                ] {
                    let g = emoji::lookup(e).unwrap_or_default();
                    let dur = mins.map(|m| if m >= 60 { format!(" — {} h", m / 60) } else { format!(" — {m} min") }).unwrap_or_default();
                    if ui.add(egui::Button::new(RichText::new(format!("{g}  {label}{dur}")).color(t.text)).frame(false)).clicked() {
                        *em = e.into();
                        *text = label.into();
                        *minutes = mins;
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Clear after").color(t.muted));
                    for (label, m) in
                        [("Don't clear", None), ("30 min", Some(30)), ("1 hour", Some(60)), ("4 hours", Some(240)), ("Today", Some(480))]
                    {
                        if ui.selectable_label(*minutes == m, label).clicked() {
                            *minutes = m;
                        }
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if primary(ui, app, "Save", !text.trim().is_empty() || !em.is_empty()) {
                        acts.push(Act::Run("profile.setStatus", json!({ "text": text, "emoji": em, "minutes": minutes })));
                        close = true;
                    }
                    if ui.button("Clear status").clicked() {
                        acts.push(Act::Run("profile.clearStatus", json!({})));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::Topic { channel, text } => {
                ui.set_width(460.0);
                let title = app.session.state.title(*channel, app.session.me);
                heading(ui, app, &format!("Edit topic for {title}"));
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                r.request_focus();
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if primary(ui, app, "Save", true) || ui.input(|i| i.key_pressed(Key::Enter)) {
                        acts.push(Act::Run("channel.setTopic", json!({ "channel": channel.to_string(), "topic": text })));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::Verify { flow } => {
                ui.set_width(520.0);
                let Some(v) = app.session.verifications.iter().find(|v| v.flow == *flow).cloned() else {
                    close = true;
                    return;
                };
                let who = app.session.state.user(v.user).map(|u| u.name().to_string()).unwrap_or_else(|| "this person".into());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Shield, t.accent);
                    ui.label(RichText::new(format!("Verify {who}")).font(FontId::proportional(20.0)).strong().color(t.strong));
                });
                ui.add_space(8.0);
                let action = |name: &'static str| Act::Run(name, json!({ "flow": v.flow }));
                match &v.stage {
                    VerifyStage::Requested => {
                        ui.label(
                            RichText::new(format!(
                                "{who} wants to verify that you are really you, so you can both trust that no one is reading your encrypted messages."
                            ))
                            .color(t.text),
                        );
                        ui.add_space(6.0);
                        ui.label(RichText::new("You'll compare seven emoji with them — in person, or on a call you trust.").color(t.muted));
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if primary(ui, app, "Start verifying", true) {
                                acts.push(action("security.accept"));
                            }
                            if ui.button("Not now").clicked() {
                                acts.push(action("security.cancel"));
                                close = true;
                            }
                        });
                    }
                    VerifyStage::Waiting => {
                        let text =
                            if v.incoming { "Connecting…".to_string() } else { format!("Waiting for {who} to accept on one of their devices…") };
                        ui.label(RichText::new(text).color(t.text));
                        ui.add_space(12.0);
                        if ui.button("Cancel").clicked() {
                            acts.push(action("security.cancel"));
                            close = true;
                        }
                    }
                    VerifyStage::Emoji { emoji } => {
                        ui.label(RichText::new(format!("Check that {who} sees the same emoji, in the same order:")).color(t.text));
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for (symbol, name) in emoji {
                                Frame::new().fill(t.row_hover).corner_radius(10).inner_margin(Margin::symmetric(4, 8)).show(ui, |ui| {
                                    ui.set_width(58.0);
                                    ui.vertical_centered(|ui| {
                                        let glyph = if sas::drawable(symbol) { symbol.as_str() } else { " " };
                                        ui.label(RichText::new(glyph).font(FontId::proportional(30.0)));
                                        ui.label(RichText::new(name).small().strong().color(t.strong));
                                    });
                                });
                            }
                        });
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if primary(ui, app, "They match", true) {
                                acts.push(action("security.confirm"));
                            }
                            let b = egui::Button::new(RichText::new("They don't match").color(t.danger)).corner_radius(6).min_size(vec2(96.0, 32.0));
                            if ui.add(b).clicked() {
                                acts.push(action("security.mismatch"));
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("If they don't match, someone may be intercepting your connection. Don't confirm.").small().color(t.muted),
                        );
                    }
                    VerifyStage::Confirmed => {
                        ui.label(RichText::new(format!("Waiting for {who} to confirm the emoji…")).color(t.text));
                    }
                    VerifyStage::Done => {
                        ui.label(RichText::new(format!("{who} is verified.")).strong().color(t.accent));
                        ui.label(
                            RichText::new("Your encrypted messages with them can't be read by anyone else, including the server.").color(t.text),
                        );
                        ui.add_space(12.0);
                        if primary(ui, app, "Done", true) {
                            close = true;
                        }
                    }
                    VerifyStage::Cancelled { reason } => {
                        ui.label(RichText::new(format!("Verification stopped: {reason}")).color(t.danger));
                        ui.add_space(12.0);
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    }
                }
            }
            Dialog::Recovery { key, input, waiting, message } => {
                ui.set_width(520.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Shield, t.accent);
                    ui.label(RichText::new("Encryption and key backup").font(FontId::proportional(20.0)).strong().color(t.strong));
                });
                ui.add_space(8.0);
                // An answer that has arrived.
                if let Some((req, step)) = *waiting
                    && let Some(r) = app.session.take_result(req)
                {
                    *waiting = None;
                    match (step, r) {
                        (RecoveryStep::SetUp, Ok(v)) => *key = v.get("recoveryKey").and_then(serde_json::Value::as_str).map(str::to_string),
                        (RecoveryStep::Restore, Ok(v)) => {
                            let left = v.get("stillUndecryptable").and_then(serde_json::Value::as_u64).unwrap_or(0);
                            let text = if left == 0 {
                                "Restored. Your encrypted history is readable on this device.".to_string()
                            } else {
                                format!("Restored. {left} older messages still have no key in the backup.")
                            };
                            *message = Some((text, false));
                            input.clear();
                        }
                        (_, Err(e)) => *message = Some((e.to_string(), true)),
                    }
                }
                let submit = |app: &mut ChatApp, id: &str, params: serde_json::Value| -> Result<Option<u64>, String> {
                    let r = app.session.run(id, &params).map_err(|e| e.to_string())?;
                    Ok(r.get("pending").and_then(serde_json::Value::as_u64))
                };
                if let Some(k) = key.clone() {
                    ui.label(RichText::new("Your recovery key").strong().color(t.strong));
                    ui.label(RichText::new("Write it down or keep it in a password manager. With it, any new device can read your encrypted history. ChatCraft can't show it again, and no one, not even the server, can recover it for you.").color(t.text));
                    ui.add_space(8.0);
                    Frame::new().fill(t.code_bg).stroke(Stroke::new(1.0, t.border)).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&k).monospace().size(15.0).color(t.strong));
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Copy").clicked() {
                            acts.push(Act::Copy(k.clone()));
                        }
                        if primary(ui, app, "I've saved it", true) {
                            close = true;
                        }
                    });
                } else {
                    let busy = waiting.is_some();
                    let step = waiting.map(|w| w.1);
                    ui.label(RichText::new("Back up your message keys").strong().color(t.strong));
                    ui.label(RichText::new("Keys are backed up to the server, encrypted with a recovery key that only you hold, so a new device can read your history.").color(t.text));
                    ui.add_space(6.0);
                    if primary(ui, app, if step == Some(RecoveryStep::SetUp) { "Setting up…" } else { "Set up key backup" }, !busy) {
                        match submit(app, "security.setupRecovery", json!({})) {
                            Ok(Some(req)) => *waiting = Some((req, RecoveryStep::SetUp)),
                            Ok(None) => {}
                            Err(e) => *message = Some((e, true)),
                        }
                    }
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(RichText::new("Restore on this device").strong().color(t.strong));
                    ui.label(RichText::new("Enter the recovery key you saved when you set up backup.").color(t.muted));
                    ui.add(egui::TextEdit::singleline(input).hint_text("recovery key").font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                    ui.add_space(6.0);
                    if primary(ui, app, if step == Some(RecoveryStep::Restore) { "Restoring…" } else { "Restore" }, !busy && !input.trim().is_empty()) {
                        match submit(app, "security.recover", json!({ "key": input.trim() })) {
                            Ok(Some(req)) => *waiting = Some((req, RecoveryStep::Restore)),
                            Ok(None) => {}
                            Err(e) => *message = Some((e, true)),
                        }
                    }
                    if let Some((text, error)) = message {
                        ui.add_space(6.0);
                        ui.label(RichText::new(text.as_str()).color(if *error { t.danger } else { t.accent }));
                    }
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    if busy {
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                    }
                }
            }
            Dialog::Shortcuts => {
                ui.set_width(560.0);
                heading(ui, app, "Keyboard shortcuts");
                let mac = cfg!(target_os = "macos");
                let pretty = |k: &str| if mac { k.replace("Mod+", "Cmd+").replace("Alt+", "Option+") } else { k.replace("Mod+", "Ctrl+") };
                egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                    egui::Grid::new("keys").num_columns(2).striped(true).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                        for (_, label, key) in UI_COMMANDS {
                            ui.label(RichText::new(*label).color(t.text));
                            ui.label(RichText::new(pretty(key)).monospace().color(t.muted));
                            ui.end_row();
                        }
                        for s in Session::registry().all().iter().filter(|s| !s.shortcut.is_empty()) {
                            ui.label(RichText::new(s.label).color(t.text));
                            ui.label(RichText::new(pretty(s.shortcut)).monospace().color(t.muted));
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            }
        });
    if close || resp.should_close() {
        acts.push(Act::CloseDialog);
    } else {
        app.st.dialog = Some(dialog);
    }
}
