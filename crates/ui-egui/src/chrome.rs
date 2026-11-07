//! Window chrome: the top bar, the left rail, popups and toasts.

use chatcraft_engine::Pane;
use chatcraft_engine::text::emoji;
use egui::{Align2, Color32, FontId, Frame, Id, Margin, RichText, Sense, Stroke, Ui, vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::{Act, ChatApp, PickFor, avatar};

pub(crate) fn top_bar(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    ui.horizontal_centered(|ui| {
        ui.add_space(64.0);
        let back = app.session.can_go_back();
        let fwd = app.session.can_go_forward();
        let dim = |on: bool| if on { t.top_fg } else { t.top_fg.linear_multiply(0.35) };
        if icons::button(ui, Icon::ArrowLeft, 28.0, dim(back), t.top_search_bg, "Back").clicked() && back {
            acts.push(Act::Run("nav.back", json!({})));
        }
        if icons::button(ui, Icon::ArrowRight, 28.0, dim(fwd), t.top_search_bg, "Forward").clicked() && fwd {
            acts.push(Act::Run("nav.forward", json!({})));
        }
        // Search, centred in what's left.
        let right_w = 120.0;
        let avail = ui.available_width() - right_w;
        let w = avail.clamp(240.0, 680.0);
        ui.add_space(((avail - w) / 2.0).max(8.0));
        Frame::new().fill(t.top_search_bg).corner_radius(7).inner_margin(Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.set_width(w - 16.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Search, t.top_fg.linear_multiply(0.8));
                let hint = format!("Search {}", app.session.state.workspace.name);
                ui.visuals_mut().weak_text_color = Some(t.rail_fg);
                let edit = egui::TextEdit::singleline(&mut app.st.search_box)
                    .id(Id::unique("top-search"))
                    .hint_text(RichText::new(hint).color(t.rail_fg))
                    .text_color(t.top_fg)
                    .frame(Frame::NONE)
                    .desired_width(f32::INFINITY);
                let resp = ui.add(edit);
                if app.st.focus_search {
                    resp.request_focus();
                    app.st.focus_search = false;
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !app.st.search_box.trim().is_empty() {
                    acts.push(Act::Search(app.st.search_box.clone()));
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let me = app.session.me.and_then(|m| app.session.state.user(m));
            let r = avatar::show(ui, 28.0, me, &t, Some(t.top_bg)).on_hover_text("Your profile and status");
            if r.clicked() {
                acts.push(Act::CloseMenus);
                app.st.profile_menu = Some(r.rect.left_bottom() + vec2(-200.0, 6.0));
            }
            ui.add_space(4.0);
            let theme_icon = if t.mode == crate::Mode::Light { Icon::Moon } else { Icon::Sun };
            if icons::button(ui, theme_icon, 28.0, t.top_fg, t.top_search_bg, "Switch light / dark (Ctrl/Cmd+Shift+L)").clicked() {
                acts.push(Act::Ui("ui.toggleTheme"));
            }
            if icons::button(ui, Icon::Info, 28.0, t.top_fg, t.top_search_bg, "Keyboard shortcuts (Ctrl/Cmd+/)").clicked() {
                acts.push(Act::Ui("ui.shortcuts"));
            }
        });
    });
}

fn rail_button(ui: &mut Ui, app: &ChatApp, icon: Icon, label: &str, active: bool, count: usize, tip: &str) -> bool {
    let t = &app.tokens;
    let (rect, resp) = ui.allocate_exact_size(vec2(52.0, 54.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, label));
    let tile = egui::Rect::from_center_size(rect.center_top() + vec2(0.0, 18.0), vec2(36.0, 32.0));
    if active || resp.hovered() {
        ui.painter().rect_filled(tile, 8.0, if active { t.rail_active } else { t.rail_active.linear_multiply(0.6) });
    }
    icons::paint(ui.painter(), tile.shrink(8.0), icon, if active { Color32::WHITE } else { t.rail_fg });
    ui.painter().text(rect.center_bottom() - vec2(0.0, 6.0), Align2::CENTER_BOTTOM, label, FontId::proportional(10.5), t.rail_fg);
    if count > 0 {
        let c = tile.right_top() + vec2(-2.0, 3.0);
        ui.painter().circle_filled(c, 8.0, t.badge);
        ui.painter().text(c, Align2::CENTER_CENTER, if count > 9 { "9+".into() } else { count.to_string() }, FontId::proportional(10.0), t.badge_fg);
    }
    resp.on_hover_text(tip).clicked()
}

pub(crate) fn rail(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    ui.vertical_centered(|ui| {
        // Workspace tile: the workspace's initials.
        let name = &app.session.state.workspace.name;
        let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase();
        let (r, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
        ui.painter().rect_filled(r, 10.0, t.side_active_bg);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, initials, FontId::proportional(16.0), t.side_active_fg);
        resp.on_hover_text(name.clone());
        ui.add_space(14.0);
        let pane = app.session.view.pane;
        let mentions = app
            .session
            .me
            .map(|me| app.session.state.mentions_of(me, 50).iter().filter(|m| m.id > app.session.state.last_read(me, m.channel)).count())
            .unwrap_or(0);
        if rail_button(ui, app, Icon::Home, "Home", !matches!(pane, Pane::Activity | Pane::Saved), 0, "Home") {
            acts.push(Act::Run("nav.closePane", json!({})));
        }
        if rail_button(ui, app, Icon::Chat, "DMs", false, 0, "New direct message (Ctrl/Cmd+N)") {
            acts.push(Act::Ui("ui.newMessage"));
        }
        if rail_button(ui, app, Icon::Bell, "Activity", pane == Pane::Activity, mentions, "Mentions (Ctrl/Cmd+Shift+M)") {
            acts.push(Act::Run("nav.activity", json!({})));
        }
        if rail_button(ui, app, Icon::Bookmark, "Later", pane == Pane::Saved, 0, "Saved for later (Ctrl/Cmd+Shift+S)") {
            acts.push(Act::Run("nav.saved", json!({})));
        }
        if rail_button(ui, app, Icon::People, "People", false, 0, "People directory") {
            acts.push(Act::Ui("ui.people"));
        }
        ui.add_space(6.0);
        if icons::button(ui, Icon::Plus, 36.0, t.rail_fg, t.rail_active, "Create a channel").clicked() {
            acts.push(Act::Ui("ui.newChannel"));
        }
    });
}

fn popup_frame(t: &crate::Tokens) -> Frame {
    Frame::new().fill(t.main_bg).stroke(Stroke::new(1.0, t.border)).corner_radius(10).inner_margin(Margin::same(8)).shadow(egui::Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: t.shadow,
    })
}

fn menu_item(ui: &mut Ui, t: &crate::Tokens, label: &str, hint: &str, danger: bool) -> bool {
    let w = ui.available_width().max(180.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, t.accent);
    }
    let fg = if resp.hovered() {
        t.accent_fg
    } else if danger {
        t.danger
    } else {
        t.text
    };
    ui.painter().text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(14.0), fg);
    if !hint.is_empty() {
        ui.painter().text(
            rect.right_center() - vec2(10.0, 0.0),
            Align2::RIGHT_CENTER,
            hint,
            FontId::proportional(12.0),
            if resp.hovered() { t.accent_fg } else { t.muted },
        );
    }
    resp.clicked()
}

#[allow(clippy::cognitive_complexity)]
pub(crate) fn popups(app: &mut ChatApp, ctx: &egui::Context, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let mut close = false;
    if let Some((target, pos)) = app.st.picker {
        let r = egui::Area::new(Id::unique("emoji-picker")).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(ctx, |ui| {
            popup_frame(&t).show(ui, |ui| {
                ui.set_width(300.0);
                let q = ui.add(egui::TextEdit::singleline(&mut app.st.picker_query).hint_text("Search emoji").desired_width(f32::INFINITY));
                q.request_focus();
                ui.add_space(4.0);
                let list = emoji::search(&app.st.picker_query);
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    egui::Grid::new("emoji-grid").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                        for (i, (name, glyph)) in list.iter().enumerate() {
                            let b = ui.add(
                                egui::Button::new(RichText::new(*glyph).font(FontId::proportional(20.0))).frame(false).min_size(vec2(32.0, 32.0)),
                            );
                            if b.on_hover_text(format!(":{name}:")).clicked() {
                                acts.push(match target {
                                    PickFor::React(m) => Act::Run("message.react", json!({ "message": m.0, "emoji": name, "on": true })),
                                    PickFor::Composer(c) => {
                                        let mut d = app.session.drafts.get(&c).cloned().unwrap_or_default();
                                        d.push_str(&format!(":{name}: "));
                                        Act::Draft(c, d)
                                    }
                                    PickFor::Thread(root) => {
                                        let mut d = app.session.thread_drafts.get(&root).cloned().unwrap_or_default();
                                        d.push_str(&format!(":{name}: "));
                                        Act::ThreadDraft(root, d)
                                    }
                                });
                                close = true;
                            }
                            if i % 8 == 7 {
                                ui.end_row();
                            }
                        }
                    });
                });
                if list.is_empty() {
                    ui.label(RichText::new("No emoji match").color(t.muted));
                }
            });
        });
        if r.response.clicked_elsewhere() && !close {
            close = true;
        }
    }
    if let Some((mid, pos)) = app.st.menu {
        let me = app.session.me.unwrap_or_default();
        let msg = app.session.state.message(mid).cloned();
        let r = egui::Area::new(Id::unique("msg-menu")).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(ctx, |ui| {
            popup_frame(&t).show(ui, |ui| {
                ui.set_width(220.0);
                let Some(m) = msg else { return };
                let saved = app.session.state.saved.get(&me).is_some_and(|s| s.contains(&m.id));
                if menu_item(ui, &t, "Reply in thread", "", false) {
                    acts.push(Act::Thread(m.id));
                    close = true;
                }
                if menu_item(ui, &t, if saved { "Remove from saved" } else { "Save for later" }, "", false) {
                    acts.push(Act::Run("message.save", json!({ "message": m.id.0 })));
                    close = true;
                }
                if menu_item(ui, &t, if m.pinned { "Unpin from conversation" } else { "Pin to conversation" }, "", false) {
                    acts.push(Act::Run("message.pin", json!({ "message": m.id.0 })));
                    close = true;
                }
                if menu_item(ui, &t, "Copy link", "", false) {
                    acts.push(Act::Copy(format!("chatcraft://{}/{}", m.channel, m.id)));
                    close = true;
                }
                if menu_item(ui, &t, "Copy text", "", false) {
                    acts.push(Act::Copy(m.text.clone()));
                    close = true;
                }
                if m.author == me {
                    ui.separator();
                    if menu_item(ui, &t, "Edit message", "E", false) {
                        acts.push(Act::Edit(m.id, m.text.clone()));
                        close = true;
                    }
                    if menu_item(ui, &t, "Delete message…", "", true) {
                        acts.push(Act::Run("message.delete", json!({ "message": m.id.0 })));
                        close = true;
                    }
                }
            });
        });
        if r.response.clicked_elsewhere() && !close {
            close = true;
        }
    }
    if let Some(pos) = app.st.profile_menu {
        let me = app.session.me.and_then(|m| app.session.state.user(m)).cloned();
        let r = egui::Area::new(Id::unique("profile-menu")).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(ctx, |ui| {
            popup_frame(&t).show(ui, |ui| {
                ui.set_width(230.0);
                if let Some(u) = &me {
                    ui.horizontal(|ui| {
                        avatar::show(ui, 36.0, Some(u), &t, Some(t.main_bg));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(u.name()).strong().color(t.strong));
                            ui.label(RichText::new(u.presence.label()).color(t.muted).small());
                        });
                    });
                    if !u.status.text.is_empty() || !u.status.emoji.is_empty() {
                        let glyph = emoji::lookup(&u.status.emoji).unwrap_or("");
                        ui.label(RichText::new(format!("{glyph} {}", u.status.text)).color(t.text));
                    }
                    ui.separator();
                    if menu_item(ui, &t, "Update your status", "", false) {
                        acts.push(Act::Ui("ui.setStatus"));
                        close = true;
                    }
                    let away = u.presence == chatcraft_engine::model::Presence::Away;
                    if menu_item(ui, &t, if away { "Set yourself as active" } else { "Set yourself as away" }, "", false) {
                        acts.push(Act::Run("profile.toggleAway", json!({})));
                        close = true;
                    }
                    let dnd = u.presence == chatcraft_engine::model::Presence::Dnd;
                    if menu_item(ui, &t, if dnd { "Resume notifications" } else { "Pause notifications" }, "", false) {
                        acts.push(Act::Run("profile.setPresence", json!({ "presence": if dnd { "active" } else { "dnd" } })));
                        close = true;
                    }
                    if menu_item(ui, &t, "Encryption and key backup", "", false) {
                        acts.push(Act::Ui("ui.recovery"));
                        close = true;
                    }
                    if menu_item(ui, &t, "Profile", "", false) {
                        acts.push(Act::Profile(u.id));
                        close = true;
                    }
                    ui.separator();
                    if menu_item(ui, &t, "Sign out", "", true) {
                        acts.push(Act::Ui("ui.signOut"));
                        close = true;
                    }
                }
            });
        });
        if r.response.clicked_elsewhere() && !close {
            close = true;
        }
    }
    if let Some(pos) = app.st.add_menu {
        let r = egui::Area::new(Id::unique("add-menu")).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(ctx, |ui| {
            popup_frame(&t).show(ui, |ui| {
                ui.set_width(220.0);
                if menu_item(ui, &t, "Create a new channel", "", false) {
                    acts.push(Act::Ui("ui.newChannel"));
                    close = true;
                }
                if menu_item(ui, &t, "Browse channels", "", false) {
                    acts.push(Act::Ui("ui.browseChannels"));
                    close = true;
                }
            });
        });
        if r.response.clicked_elsewhere() && !close {
            close = true;
        }
    }
    if close {
        acts.push(Act::CloseMenus);
    }
}

pub(crate) fn toasts(app: &mut ChatApp, ctx: &egui::Context, acts: &mut Vec<Act>) {
    if app.st.toasts.is_empty() {
        return;
    }
    let t = app.tokens.clone();
    let screen = ctx.content_rect();
    egui::Area::new(Id::unique("toasts")).order(egui::Order::Tooltip).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -88.0)).interactable(true).show(
        ctx,
        |ui| {
            ui.set_max_width((screen.width() * 0.5).max(320.0));
            for (text, error, _, target) in app.st.toasts.iter().rev() {
                let fill = if *error { t.danger } else { t.toast_bg };
                let fg = if *error { Color32::WHITE } else { t.toast_fg };
                let r = Frame::new().fill(fill).corner_radius(8).inner_margin(Margin::symmetric(14, 9)).show(ui, |ui| {
                    ui.label(RichText::new(text).color(fg));
                });
                if let Some((c, m)) = target
                    && r.response.interact(Sense::click()).clicked()
                {
                    acts.push(Act::OpenAt(*c, *m));
                }
                ui.add_space(6.0);
            }
        },
    );
}
