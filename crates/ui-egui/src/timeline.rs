//! The open conversation: header, messages, composer.

use chatcraft_engine::model::{ChannelKind, Message, MessageId};
use chatcraft_engine::text::emoji;
use egui::{Align2, Color32, FontId, Frame, Id, Key, Margin, RichText, Sense, Stroke, Ui, vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::markup::{self, Clicked};
use crate::{Act, ChatApp, Dialog, PickFor, avatar};

const GROUP_MS: i64 = 5 * 60_000;
const MAX_SHOWN: usize = 400;

pub(crate) fn show(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    let Some(c) = app.session.view.channel else {
        ui.centered_and_justified(|ui| ui.label(RichText::new("Pick a conversation from the sidebar").color(app.tokens.muted)));
        return;
    };
    let t = app.tokens.clone();
    egui::Panel::top("channel-header")
        .exact_size(56.0)
        .resizable(false)
        .frame(Frame::new().fill(t.main_bg).inner_margin(Margin::symmetric(20, 8)).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| header(app, ui, c, acts));
    egui::Panel::bottom("composer")
        .resizable(false)
        .frame(Frame::new().fill(t.main_bg).inner_margin(Margin { left: 20, right: 20, top: 4, bottom: 16 }))
        .show(ui, |ui| {
            let ch = app.session.state.channel(c).cloned();
            match ch {
                Some(ch) if ch.archived => banner(ui, &t, &format!("You're viewing #{}, which is archived.", ch.name), None, acts),
                Some(ch) if app.session.previewing() => {
                    let purpose = if ch.purpose.is_empty() { String::new() } else { format!("\n{}", ch.purpose) };
                    banner(ui, &t, &format!("You're viewing #{}{purpose}", ch.name), Some(("Join channel", "channel.join")), acts);
                }
                Some(_) => composer(app, ui, Target::Channel(c), acts),
                None => {}
            }
        });
    egui::CentralPanel::no_frame().show(ui, |ui| messages(app, ui, c, acts));
}

fn header(app: &mut ChatApp, ui: &mut Ui, c: chatcraft_engine::model::ChannelId, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let s = &app.session;
    let Some(ch) = s.state.channel(c) else { return };
    let me = s.me.unwrap_or_default();
    let starred = s.state.starred.get(&me).is_some_and(|x| x.contains(&c));
    let muted = s.state.muted.get(&me).is_some_and(|x| x.contains(&c));
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                match ch.kind {
                    ChannelKind::Public => icons::paint(ui.painter(), r.shrink(1.0), Icon::Hash, t.strong),
                    ChannelKind::Private => icons::paint(ui.painter(), r.shrink(2.0), Icon::Lock, t.strong),
                    _ => {
                        let peer = ch.members.iter().find(|u| **u != me).and_then(|u| s.state.user(*u));
                        avatar::paint(ui, r, peer, &t);
                    }
                }
                let title = if ch.kind.is_dm() { s.state.title(c, s.me) } else { ch.name.clone() };
                let r =
                    ui.add(egui::Label::new(RichText::new(title).font(FontId::proportional(18.0)).strong().color(t.strong)).sense(Sense::click()));
                if r.on_hover_text("Conversation details").clicked() {
                    acts.push(Act::Run("nav.details", json!({})));
                }
                if !ch.kind.is_dm() || ch.kind == ChannelKind::Group {
                    let star = if starred { Icon::StarFilled } else { Icon::Star };
                    let col = if starred { t.side_active_bg } else { t.muted };
                    if icons::button(ui, star, 24.0, col, t.row_hover, if starred { "Unstar" } else { "Star" }).clicked() {
                        acts.push(Act::Run("channel.star", json!({ "channel": c.to_string() })));
                    }
                }
                if muted {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Mute, t.muted);
                }
            });
            let sub = if !ch.topic.is_empty() {
                ch.topic.clone()
            } else if ch.kind == ChannelKind::Direct {
                ch.members
                    .iter()
                    .find(|u| **u != me)
                    .and_then(|u| s.state.user(*u))
                    .map(|u| format!("{} · {}", u.title, u.presence.label()))
                    .unwrap_or_default()
            } else {
                "Add a topic".into()
            };
            let r = ui.add(egui::Label::new(RichText::new(sub).color(t.muted).small()).truncate().sense(Sense::click()));
            if !ch.kind.is_dm() && r.on_hover_text("Edit topic").clicked() {
                acts.push(Act::Dialog(Dialog::Topic { channel: c, text: ch.topic.clone() }));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, Icon::Info, 30.0, t.muted, t.row_hover, "Details (Ctrl/Cmd+Shift+I)").clicked() {
                acts.push(Act::Run("nav.details", json!({})));
            }
            if !ch.kind.is_dm() {
                // Member faces and count.
                let n = ch.members.len();
                let r = Frame::new().stroke(Stroke::new(1.0, t.border)).corner_radius(8).inner_margin(Margin::symmetric(6, 3)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for u in ch.members.iter().take(3).filter_map(|u| s.state.user(*u)) {
                            avatar::show(ui, 20.0, Some(u), &t, None);
                        }
                        ui.label(RichText::new(n.to_string()).color(t.text));
                    });
                });
                if r.response.interact(Sense::click()).on_hover_text("Members").clicked() {
                    acts.push(Act::Run("nav.details", json!({})));
                }
            }
        });
    });
}

fn banner(ui: &mut Ui, t: &crate::Tokens, text: &str, button: Option<(&str, &'static str)>, acts: &mut Vec<Act>) {
    Frame::new().fill(t.row_hover).corner_radius(10).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(text).color(t.text));
            if let Some((label, cmd)) = button {
                ui.add_space(6.0);
                let b =
                    egui::Button::new(RichText::new(label).color(t.accent_fg).strong()).fill(t.accent).corner_radius(6).min_size(vec2(140.0, 32.0));
                if ui.add(b).clicked() {
                    acts.push(Act::Run(cmd, json!({})));
                }
            }
        });
    });
}

fn day(app: &ChatApp, m: &Message) -> String {
    let local = app.local(m.ts);
    let today = app.local(chatcraft_engine::model::Ts::now());
    if local.day_key() == today.day_key() {
        "Today".into()
    } else if local.plus_millis(86_400_000).day_key() == today.day_key() {
        "Yesterday".into()
    } else {
        local.day_label()
    }
}

fn divider(ui: &mut Ui, label: &str, line: Color32, fg: Color32, pill_bg: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, line));
    let g = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), fg);
    let pill = egui::Rect::from_center_size(rect.center(), g.size() + vec2(22.0, 8.0));
    ui.painter().rect(pill, 12.0, pill_bg, Stroke::new(1.0, line), egui::StrokeKind::Inside);
    ui.painter().galley(pill.center() - g.size() / 2.0, g, fg);
}

fn new_line(ui: &mut Ui, t: &crate::Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().hline(rect.left()..=rect.right() - 48.0, y, Stroke::new(1.0, t.new_line));
    ui.painter().text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, "New", FontId::proportional(12.0), t.new_line);
}

fn intro(app: &ChatApp, ui: &mut Ui, c: chatcraft_engine::model::ChannelId) {
    let t = &app.tokens;
    let s = &app.session;
    let Some(ch) = s.state.channel(c) else { return };
    ui.add_space(24.0);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.vertical(|ui| {
            if ch.kind.is_dm() {
                let title = s.state.title(c, s.me);
                ui.label(RichText::new(title).font(FontId::proportional(22.0)).strong().color(t.strong));
                ui.label(RichText::new("This is the very beginning of your direct message history. Only the people here can see it.").color(t.muted));
            } else {
                ui.label(RichText::new(format!("#{}", ch.name)).font(FontId::proportional(24.0)).strong().color(t.strong));
                let who = s
                    .state
                    .user(ch.created_by)
                    .map(|u| format!("@{} created this channel.", u.handle))
                    .unwrap_or_else(|| "Created with the workspace.".into());
                ui.label(RichText::new(format!("{who} This is the very beginning of #{}.", ch.name)).color(t.muted));
                if !ch.purpose.is_empty() {
                    ui.label(RichText::new(&ch.purpose).color(t.text));
                }
            }
        });
    });
    ui.add_space(12.0);
}

fn messages(app: &mut ChatApp, ui: &mut Ui, c: chatcraft_engine::model::ChannelId, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let all: Vec<Message> = app.session.state.timeline(c).into_iter().cloned().collect();
    let start = all.len().saturating_sub(MAX_SHOWN);
    let shown = all.get(start..).unwrap_or_default();
    let marker = app.session.view.unread_from;
    egui::ScrollArea::vertical().id_salt(("timeline", c.0)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        if start == 0 {
            intro(app, ui, c);
        }
        let mut prev: Option<&Message> = None;
        for m in shown {
            let d = day(app, m);
            let new_day = prev.is_none_or(|p| day(app, p) != d);
            if new_day {
                divider(ui, &d, t.border, t.text, t.main_bg);
            }
            if marker == Some(m.id) {
                new_line(ui, &t);
            }
            let compact = !new_day
                && marker != Some(m.id)
                && prev
                    .is_some_and(|p| p.author == m.author && !p.deleted && m.ts.millis_since(p.ts) < GROUP_MS && !m.broadcast && p.reply_count == 0);
            row(app, ui, m, compact, false, acts);
            prev = Some(m);
        }
        ui.add_space(8.0);
    });
}

/// One message. Used by the timeline and the thread pane.
#[allow(clippy::cognitive_complexity)]
pub(crate) fn row(app: &mut ChatApp, ui: &mut Ui, m: &Message, compact: bool, in_thread: bool, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let me = app.session.me.unwrap_or_default();
    let my_handle = app.session.state.user(me).map(|u| u.handle.clone()).unwrap_or_default();
    let author = app.session.state.user(m.author).cloned();
    let bg = ui.painter().add(egui::Shape::Noop);
    let full_w = ui.available_width();
    let editing = app.st.editing.as_ref().is_some_and(|(id, _)| *id == m.id);
    ui.add_space(if compact { 0.0 } else { 6.0 });
    let resp = ui
        .horizontal_top(|ui| {
            ui.set_width(full_w);
            ui.add_space(20.0);
            if compact {
                ui.allocate_exact_size(vec2(36.0, 18.0), Sense::hover());
            } else {
                let r = avatar::show(ui, 36.0, author.as_ref(), &t, None);
                if r.clicked() {
                    acts.push(Act::Profile(m.author));
                }
            }
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width((full_w - 84.0).max(80.0));
                ui.spacing_mut().item_spacing.y = 2.0;
                if !compact {
                    ui.horizontal(|ui| {
                        let name = author.as_ref().map(|u| u.name().to_string()).unwrap_or_else(|| "Unknown".into());
                        let r = ui.add(egui::Label::new(RichText::new(name).strong().color(t.strong)).sense(Sense::click()));
                        if r.clicked() {
                            acts.push(Act::Profile(m.author));
                        }
                        if author.as_ref().is_some_and(|u| u.bot) {
                            Frame::new().fill(t.row_hover).corner_radius(3).inner_margin(Margin::symmetric(4, 0)).show(ui, |ui| {
                                ui.label(RichText::new("APP").small().color(t.muted));
                            });
                        }
                        if let Some(u) = author.as_ref().filter(|u| !u.status.emoji.is_empty())
                            && let Some(g) = emoji::lookup(&u.status.emoji)
                        {
                            ui.label(g).on_hover_text(&u.status.text);
                        }
                        ui.label(RichText::new(app.local(m.ts).clock()).small().color(t.muted)).on_hover_text(app.local(m.ts).rfc3339());
                        if m.pinned {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                            icons::paint(ui.painter(), r, Icon::Pin, t.muted);
                        }
                    });
                }
                if m.broadcast
                    && !in_thread
                    && let Some(root) = m.thread.and_then(|r| app.session.state.message(r))
                {
                    let r = ui.add(
                        egui::Label::new(
                            RichText::new(format!("replied to a thread: {}", chatcraft_engine::text::snippet(&root.text, 60))).small().color(t.muted),
                        )
                        .sense(Sense::click()),
                    );
                    if r.clicked() {
                        acts.push(Act::Thread(root.id));
                    }
                }
                if m.deleted {
                    ui.label(RichText::new("This message was deleted.").italics().color(t.muted));
                } else if editing {
                    edit_box(app, ui, m.id, acts);
                } else {
                    match markup::show(ui, &m.text, &t, &my_handle) {
                        Some(Clicked::Person(h)) => acts.push(Act::ProfileHandle(h)),
                        Some(Clicked::Channel(n)) => acts.push(Act::ChannelName(n)),
                        None => {}
                    }
                    if m.edited.is_some() {
                        ui.label(RichText::new("(edited)").small().color(t.faint));
                    }
                }
                if !m.reactions.is_empty() && !m.deleted {
                    reactions(app, ui, m, acts);
                }
                if m.reply_count > 0 && !in_thread {
                    thread_summary(app, ui, m, acts);
                }
            });
        })
        .response;
    let rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left(), resp.rect.top() - if compact { 1.0 } else { 4.0 }),
        egui::pos2(ui.max_rect().right(), resp.rect.bottom() + 2.0),
    );
    let pointer = ui.ctx().pointer_hover_pos();
    let on_other_bar = app.st.hover_bar.is_some_and(|(id, r)| id != m.id && pointer.is_some_and(|p| r.contains(p)));
    let on_own_bar = app.st.hover_bar.is_some_and(|(id, r)| id == m.id && pointer.is_some_and(|p| r.contains(p)));
    let blocked = app.st.dialog.is_some() || app.st.picker.is_some() || app.st.menu.is_some();
    let hovered = !blocked && ((ui.rect_contains_pointer(rect) && !on_other_bar) || on_own_bar) || app.st.menu.is_some_and(|(id, _)| id == m.id);
    let focused = app.session.view.focus == Some(m.id);
    if focused {
        ui.painter().set(bg, egui::Shape::rect_filled(rect, 0.0, t.row_focus));
        resp.scroll_to_me(Some(egui::Align::Center));
        app.session.view.focus = None;
    } else if hovered || editing {
        ui.painter().set(bg, egui::Shape::rect_filled(rect, 0.0, t.row_hover));
    }
    if hovered && compact && !editing {
        ui.painter().text(
            egui::pos2(rect.left() + 38.0, resp.rect.top() + 10.0),
            Align2::CENTER_CENTER,
            app.local(m.ts).clock(),
            FontId::proportional(10.5),
            t.muted,
        );
    }
    if hovered && !m.deleted && !editing {
        action_bar(app, ui, m, rect, in_thread, acts);
    }
}

fn action_bar(app: &mut ChatApp, ui: &mut Ui, m: &Message, row: egui::Rect, in_thread: bool, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let w = if in_thread { 214.0 } else { 246.0 };
    let bar = egui::Rect::from_min_size(egui::pos2(row.right() - w - 18.0, row.top() - 14.0), vec2(w, 32.0));
    app.st.next_hover_bar = Some((m.id, bar));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    Frame::new()
        .fill(t.main_bg)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(8)
        .inner_margin(Margin::symmetric(3, 2))
        .shadow(egui::Shadow { offset: [0, 1], blur: 4, spread: 0, color: t.shadow })
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            for name in ["white_check_mark", "eyes", "raised_hands"] {
                let g = emoji::lookup(name).unwrap_or("?");
                let b = ui.add(egui::Button::new(RichText::new(g).font(FontId::proportional(16.0))).frame(false).min_size(vec2(28.0, 26.0)));
                if b.on_hover_text(format!(":{name}:")).clicked() {
                    acts.push(Act::Run("message.react", json!({ "message": m.id.0, "emoji": name })));
                }
            }
            let r = icons::button(ui, Icon::Smile, 28.0, t.muted, t.row_hover, "Add reaction");
            if r.clicked() {
                acts.push(Act::Picker(PickFor::React(m.id), r.rect.left_bottom() + vec2(-260.0, 4.0)));
            }
            if !in_thread && icons::button(ui, Icon::Thread, 28.0, t.muted, t.row_hover, "Reply in thread").clicked() {
                acts.push(Act::Thread(m.id));
            }
            let saved = app.session.me.is_some_and(|me| app.session.state.saved.get(&me).is_some_and(|s| s.contains(&m.id)));
            if icons::button(
                ui,
                Icon::Bookmark,
                28.0,
                if saved { t.accent } else { t.muted },
                t.row_hover,
                if saved { "Remove from saved" } else { "Save for later" },
            )
            .clicked()
            {
                acts.push(Act::Run("message.save", json!({ "message": m.id.0 })));
            }
            let r = icons::button(ui, Icon::More, 28.0, t.muted, t.row_hover, "More actions");
            if r.clicked() {
                acts.push(Act::Menu(m.id, r.rect.left_bottom() + vec2(-200.0, 4.0)));
            }
        });
}

fn reactions(app: &mut ChatApp, ui: &mut Ui, m: &Message, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let me = app.session.me.unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (name, who) in &m.reactions {
            let mine = who.contains(&me);
            let glyph = emoji::lookup(name).map(str::to_string).unwrap_or_else(|| format!(":{name}:"));
            let (fill, stroke) = if mine { (t.reaction_mine_bg, t.reaction_mine_border) } else { (t.reaction_bg, t.reaction_border) };
            let r = Frame::new().fill(fill).stroke(Stroke::new(1.0, stroke)).corner_radius(12).inner_margin(Margin::symmetric(7, 1)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(glyph).font(FontId::proportional(15.0)));
                    ui.label(RichText::new(who.len().to_string()).small().color(if mine { t.mention_fg } else { t.text }));
                });
            });
            let names: Vec<String> = who
                .iter()
                .filter_map(|u| app.session.state.user(*u))
                .map(|u| if u.id == me { "You".to_string() } else { u.name().to_string() })
                .collect();
            let resp = r.response.interact(Sense::click()).on_hover_text(format!("{} reacted with :{name}:", names.join(", ")));
            if resp.clicked() {
                acts.push(Act::Run("message.react", json!({ "message": m.id.0, "emoji": name, "on": !mine })));
            }
        }
        let r = Frame::new().fill(t.reaction_bg).corner_radius(12).inner_margin(Margin::symmetric(7, 3)).show(ui, |ui| {
            let (rr, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), rr, Icon::Smile, t.muted);
        });
        let resp = r.response.interact(Sense::click()).on_hover_text("Add reaction");
        if resp.clicked() {
            acts.push(Act::Picker(PickFor::React(m.id), resp.rect.left_bottom() + vec2(0.0, 4.0)));
        }
    });
}

fn thread_summary(app: &mut ChatApp, ui: &mut Ui, m: &Message, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let r = Frame::new().corner_radius(6).inner_margin(Margin::symmetric(4, 2)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for u in m.repliers.iter().take(4).filter_map(|u| app.session.state.user(*u)) {
                avatar::show(ui, 22.0, Some(u), &t, None);
            }
            ui.add_space(4.0);
            let n = m.reply_count;
            ui.label(RichText::new(if n == 1 { "1 reply".to_string() } else { format!("{n} replies") }).strong().color(t.link));
            if let Some(last) = m.last_reply {
                ui.label(RichText::new(format!("Last reply {}", last.ago(chatcraft_engine::model::Ts::now()))).small().color(t.muted));
            }
        });
    });
    let resp = r.response.interact(Sense::click());
    if resp.hovered() {
        ui.painter().rect_stroke(resp.rect, 6.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    }
    if resp.clicked() {
        acts.push(Act::Thread(m.id));
    }
}

fn edit_box(app: &mut ChatApp, ui: &mut Ui, id: MessageId, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let Some((_, text)) = app.st.editing.as_mut() else { return };
    Frame::new().fill(t.composer_bg).stroke(Stroke::new(1.5, t.accent)).corner_radius(8).inner_margin(Margin::same(8)).show(ui, |ui| {
        let edit = egui::TextEdit::multiline(text)
            .id(Id::unique(("edit", id.0)))
            .frame(Frame::NONE)
            .desired_rows(1)
            .desired_width(f32::INFINITY)
            .return_key(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter));
        let r = ui.add(edit);
        if !r.has_focus() && !r.lost_focus() {
            r.request_focus();
        }
        let enter = r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
        let esc = ui.input(|i| i.key_pressed(Key::Escape));
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel").clicked();
            let save = ui.add(egui::Button::new(RichText::new("Save").color(t.accent_fg)).fill(t.accent)).clicked();
            if save || enter {
                acts.push(Act::SaveEdit(id, text.clone()));
            } else if cancel || esc {
                acts.push(Act::CancelEdit);
            }
        });
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Channel(chatcraft_engine::model::ChannelId),
    Thread(MessageId),
}

/// Wraps the selection in `before`/`after` (or inserts the pair at the caret).
fn wrap_selection(ctx: &egui::Context, id: Id, text: &mut String, before: &str, after: &str) {
    let state = egui::text_edit::TextEditState::load(ctx, id);
    let range = state.as_ref().and_then(|s| s.cursor.char_range());
    let n = text.chars().count();
    let (a, b) = match range {
        Some(r) => {
            let (x, y) = (r.primary.index.0.min(n), r.secondary.index.0.min(n));
            (x.min(y), x.max(y))
        }
        None => (n, n),
    };
    let byte = |ci: usize| text.char_indices().nth(ci).map_or(text.len(), |(i, _)| i);
    let (ba, bb) = (byte(a), byte(b));
    let inner = text.get(ba..bb).unwrap_or_default().to_string();
    let new = format!("{}{before}{inner}{after}{}", text.get(..ba).unwrap_or_default(), text.get(bb..).unwrap_or_default());
    *text = new;
    if let Some(mut st) = state {
        let start = a + before.chars().count();
        let end = start + inner.chars().count();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(start), egui::text::CCursor::new(end))));
        st.store(ctx, id);
    }
}

/// Suggestions for the word being typed: `@people`, `#channels`, `:emoji:`.
fn suggestions(app: &ChatApp, text: &str) -> Vec<(String, String)> {
    let word = text.rsplit(char::is_whitespace).next().unwrap_or_default();
    let s = &app.session.state;
    if let Some(q) = word.strip_prefix('@') {
        let q = q.to_lowercase();
        let mut v: Vec<(String, String)> = s
            .users
            .values()
            .filter(|u| !u.deactivated && (u.handle.starts_with(&q) || u.name().to_lowercase().contains(&q)))
            .map(|u| (format!("@{} ", u.handle), format!("{}  @{}", u.name(), u.handle)))
            .take(6)
            .collect();
        for b in ["here", "channel"] {
            if b.starts_with(&q) && !q.is_empty() {
                v.push((format!("@{b} "), format!("@{b} — notify everyone")));
            }
        }
        return v;
    }
    if let Some(q) = word.strip_prefix('#') {
        let q = q.to_lowercase();
        return s
            .channels
            .values()
            .filter(|c| !c.kind.is_dm() && c.name.starts_with(&q) && app.session.me.is_some_and(|me| s.can_see(me, c.id)))
            .map(|c| (format!("#{} ", c.name), format!("#{}", c.name)))
            .take(6)
            .collect();
    }
    if let Some(q) = word.strip_prefix(':')
        && q.len() >= 2
        && !q.contains(':')
    {
        return emoji::search(q).into_iter().take(8).map(|(n, g)| (format!(":{n}: "), format!("{g}  :{n}:"))).collect();
    }
    Vec::new()
}

#[allow(clippy::cognitive_complexity)]
pub(crate) fn composer(app: &mut ChatApp, ui: &mut Ui, target: Target, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let ctx = ui.ctx().clone();
    let (id, mut text, placeholder) = match target {
        Target::Channel(c) => (
            Id::unique(("composer", c.0)),
            app.session.drafts.get(&c).cloned().unwrap_or_default(),
            format!("Message {}", app.session.state.title(c, app.session.me)),
        ),
        Target::Thread(r) => (Id::unique(("reply", r.0)), app.session.thread_drafts.get(&r).cloned().unwrap_or_default(), "Reply…".to_string()),
    };
    let before = text.clone();
    let mut sent = false;
    let sugg = suggestions(app, &text);
    // Tab completes the first suggestion (and doesn't move focus).
    let tab = !sugg.is_empty() && ctx.memory(|m| m.has_focus(id)) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab));
    if !sugg.is_empty() {
        Frame::new().fill(t.main_bg).stroke(Stroke::new(1.0, t.border)).corner_radius(8).inner_margin(Margin::same(4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, (insert, label)) in sugg.iter().enumerate() {
                let r = ui.add(egui::Button::new(RichText::new(label).color(t.text)).frame(false).min_size(vec2(ui.available_width(), 24.0)));
                if r.clicked() || (tab && i == 0) {
                    let cut = text.rfind(char::is_whitespace).map_or(0, |p| p + 1);
                    text.truncate(cut);
                    text.push_str(insert);
                    ctx.memory_mut(|m| m.request_focus(id));
                }
            }
        });
        ui.add_space(4.0);
    }
    let focused = ctx.memory(|m| m.has_focus(id));
    let border = if focused { t.accent } else { t.composer_border };
    Frame::new()
        .fill(t.composer_bg)
        .stroke(Stroke::new(if focused { 1.5 } else { 1.0 }, border))
        .corner_radius(10)
        .inner_margin(Margin::same(0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Formatting toolbar.
            Frame::new()
                .fill(t.toolbar_bg)
                .corner_radius(egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 })
                .inner_margin(Margin::symmetric(6, 3))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let fmt = |ui: &mut Ui, label: RichText, tip: &str| {
                            ui.add(egui::Button::new(label).frame(false).min_size(vec2(28.0, 24.0))).on_hover_text(tip).clicked()
                        };
                        if fmt(ui, RichText::new("B").strong().color(t.muted), "Bold (*text*)") {
                            wrap_selection(&ctx, id, &mut text, "*", "*");
                        }
                        if fmt(ui, RichText::new("I").italics().color(t.muted), "Italic (_text_)") {
                            wrap_selection(&ctx, id, &mut text, "_", "_");
                        }
                        if fmt(ui, RichText::new("S").strikethrough().color(t.muted), "Strikethrough (~text~)") {
                            wrap_selection(&ctx, id, &mut text, "~", "~");
                        }
                        if icons::button(ui, Icon::Code, 26.0, t.muted, t.row_hover, "Code (`text`)").clicked() {
                            wrap_selection(&ctx, id, &mut text, "`", "`");
                        }
                        if icons::button(ui, Icon::Link, 26.0, t.muted, t.row_hover, "Link (<url|text>)").clicked() {
                            wrap_selection(&ctx, id, &mut text, "<https://|", ">");
                        }
                        if icons::button(ui, Icon::List, 26.0, t.muted, t.row_hover, "Bulleted list").clicked() {
                            if !text.is_empty() && !text.ends_with('\n') {
                                text.push('\n');
                            }
                            text.push_str("- ");
                        }
                        if icons::button(ui, Icon::Quote, 26.0, t.muted, t.row_hover, "Quote").clicked() {
                            if !text.is_empty() && !text.ends_with('\n') {
                                text.push('\n');
                            }
                            text.push_str("> ");
                        }
                    });
                });
            Frame::new().inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
                let edit = egui::TextEdit::multiline(&mut text)
                    .id(id)
                    .hint_text(RichText::new(placeholder).color(t.faint))
                    .frame(Frame::NONE)
                    .desired_rows(1)
                    .desired_width(f32::INFINITY)
                    .text_color(t.text)
                    .return_key(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter));
                let r = egui::ScrollArea::vertical().max_height(220.0).id_salt(("composer-scroll", id)).show(ui, |ui| ui.add(edit)).inner;
                if app.st.focus_composer && matches!(target, Target::Channel(_)) && app.st.dialog.is_none() {
                    r.request_focus();
                    app.st.focus_composer = false;
                }
                let enter = r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift && !i.modifiers.alt);
                let up = r.has_focus() && text.is_empty() && ui.input(|i| i.key_pressed(Key::ArrowUp));
                if enter && !text.trim().is_empty() {
                    push_text(target, &text, acts);
                    push_send(target, acts);
                    sent = true;
                } else if up
                    && let Target::Channel(c) = target
                    && let Some(last) = app.session.last_own_message(c)
                    && let Some(m) = app.session.state.message(last)
                {
                    acts.push(Act::Edit(last, m.text.clone()));
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let r = icons::button(ui, Icon::Smile, 28.0, t.muted, t.row_hover, "Emoji");
                if r.clicked() {
                    let f = match target {
                        Target::Channel(c) => PickFor::Composer(c),
                        Target::Thread(root) => PickFor::Thread(root),
                    };
                    acts.push(Act::Picker(f, r.rect.left_top() + vec2(0.0, -300.0)));
                }
                if icons::button(ui, Icon::At, 28.0, t.muted, t.row_hover, "Mention someone").clicked() {
                    if !text.is_empty() && !text.ends_with(' ') {
                        text.push(' ');
                    }
                    text.push('@');
                    ctx.memory_mut(|m| m.request_focus(id));
                }
                if let Target::Thread(_) = target {
                    let ch = app.session.view.channel.map(|c| app.session.state.title(c, app.session.me)).unwrap_or_default();
                    ui.checkbox(&mut app.st.also_send, RichText::new(format!("Also send to {ch}")).small().color(t.muted));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    let ready = !text.trim().is_empty();
                    let (fill, fg) = if ready { (t.accent, t.accent_fg) } else { (Color32::TRANSPARENT, t.faint) };
                    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 28.0), Sense::click());
                    ui.painter().rect_filled(rect, 6.0, fill);
                    icons::paint(ui.painter(), rect.shrink2(vec2(9.0, 7.0)), Icon::Send, fg);
                    if resp.on_hover_text("Send (Enter) · new line (Shift+Enter)").clicked() && ready {
                        push_text(target, &text, acts);
                        push_send(target, acts);
                        sent = true;
                    }
                });
            });
            ui.add_space(4.0);
        });
    if text != before && !sent {
        push_text(target, &text, acts);
    }
}

fn push_text(target: Target, text: &str, acts: &mut Vec<Act>) {
    acts.push(match target {
        Target::Channel(c) => Act::Draft(c, text.to_string()),
        Target::Thread(r) => Act::ThreadDraft(r, text.to_string()),
    });
}

fn push_send(target: Target, acts: &mut Vec<Act>) {
    acts.push(match target {
        Target::Channel(c) => Act::Send(c),
        Target::Thread(r) => Act::Reply(r),
    });
}
