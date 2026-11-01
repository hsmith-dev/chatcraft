//! The right-hand pane: thread, details, search results, activity, saved items, profiles.

use chatcraft_engine::Pane;
use chatcraft_engine::model::{ChannelKind, Message};
use chatcraft_engine::text::{emoji, snippet};
use egui::{FontId, Frame, Margin, RichText, Sense, Stroke, Ui, vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::timeline::{self, Target};
use crate::{Act, ChatApp, Dialog, avatar};

fn title_bar(app: &ChatApp, ui: &mut Ui, title: &str, sub: &str, acts: &mut Vec<Act>) {
    let t = &app.tokens;
    Frame::new().inner_margin(Margin::symmetric(16, 10)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).font(FontId::proportional(17.0)).strong().color(t.strong));
            if !sub.is_empty() {
                ui.label(RichText::new(sub).color(t.muted));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Close, 28.0, t.muted, t.row_hover, "Close").clicked() {
                    acts.push(Act::Run("nav.closePane", json!({})));
                }
            });
        });
    });
    let y = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, t.border));
}

pub(crate) fn show(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    match app.session.view.pane {
        Pane::Thread => thread(app, ui, acts),
        Pane::Details => details(app, ui, acts),
        Pane::Search => results(app, ui, acts),
        Pane::Activity => activity(app, ui, acts),
        Pane::Saved => saved(app, ui, acts),
        Pane::Profile => profile(app, ui, acts),
        Pane::None => {}
    }
}

fn thread(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    let Some(root) = app.session.view.thread.and_then(|r| app.session.state.message(r)).cloned() else {
        title_bar(app, ui, "Thread", "", acts);
        ui.label("This thread is no longer available.");
        return;
    };
    let ch = app.session.state.title(root.channel, app.session.me);
    title_bar(app, ui, "Thread", &ch, acts);
    let t = app.tokens.clone();
    let replies: Vec<Message> = app.session.state.replies(root.id).into_iter().cloned().collect();
    egui::Panel::bottom("thread-composer").resizable(false).frame(Frame::new().inner_margin(Margin::symmetric(16, 12))).show(ui, |ui| {
        let can_write = app.session.state.channel(root.channel).is_some_and(|c| !c.archived) && !app.session.previewing();
        if can_write {
            timeline::composer(app, ui, Target::Thread(root.id), acts);
        }
    });
    egui::CentralPanel::no_frame().show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(("thread", root.id.0)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            timeline::row(app, ui, &root, false, true, acts);
            if !replies.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let n = replies.len();
                    ui.label(RichText::new(if n == 1 { "1 reply".into() } else { format!("{n} replies") }).color(t.muted).small());
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width() - 20.0, 12.0), Sense::hover());
                    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, t.border));
                });
            }
            let mut prev: Option<&Message> = None;
            for m in &replies {
                let compact = prev.is_some_and(|p| p.author == m.author && m.ts.millis_since(p.ts) < 5 * 60_000);
                timeline::row(app, ui, m, compact, true, acts);
                prev = Some(m);
            }
            ui.add_space(8.0);
        });
    });
}

fn details(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    let Some(c) = app.session.view.channel else { return };
    let Some(ch) = app.session.state.channel(c).cloned() else { return };
    let t = app.tokens.clone();
    let title = app.session.state.title(c, app.session.me);
    title_bar(app, ui, &title, "", acts);
    let me = app.session.me.unwrap_or_default();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let card = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
                Frame::new().fill(t.main_bg).stroke(Stroke::new(1.0, t.border)).corner_radius(10).inner_margin(Margin::same(14)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    add(ui);
                });
                ui.add_space(10.0);
            };
            if !ch.kind.is_dm() {
                card(ui, &mut |ui| {
                    ui.label(RichText::new("Topic").strong().color(t.strong));
                    ui.label(RichText::new(if ch.topic.is_empty() { "No topic yet" } else { &ch.topic }).color(t.text));
                    if ui.link("Edit topic").clicked() {
                        acts.push(Act::Dialog(Dialog::Topic { channel: c, text: ch.topic.clone() }));
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("Description").strong().color(t.strong));
                    ui.label(RichText::new(if ch.purpose.is_empty() { "No description" } else { &ch.purpose }).color(t.text));
                    ui.add_space(8.0);
                    let who = app.session.state.user(ch.created_by).map(|u| u.name().to_string()).unwrap_or_else(|| "the workspace".into());
                    ui.label(RichText::new(format!("Created by {who} on {}", app.local(ch.created).day_label())).small().color(t.muted));
                });
            }
            card(ui, &mut |ui| {
                ui.label(RichText::new(format!("Members · {}", ch.members.len())).strong().color(t.strong));
                ui.add_space(4.0);
                for u in ch.members.iter().filter_map(|u| app.session.state.user(*u)) {
                    let r = ui.horizontal(|ui| {
                        avatar::show(ui, 28.0, Some(u), &t, Some(t.main_bg));
                        ui.label(RichText::new(u.name()).color(t.text));
                        if u.id == me {
                            ui.label(RichText::new("(you)").color(t.muted));
                        }
                        if !u.title.is_empty() {
                            ui.label(RichText::new(&u.title).small().color(t.muted));
                        }
                    });
                    if r.response.interact(Sense::click()).clicked() {
                        acts.push(Act::Profile(u.id));
                    }
                }
            });
            let pins: Vec<Message> = app.session.state.timeline(c).into_iter().filter(|m| m.pinned).cloned().collect();
            if !pins.is_empty() {
                card(ui, &mut |ui| {
                    ui.label(RichText::new(format!("Pinned · {}", pins.len())).strong().color(t.strong));
                    for m in &pins {
                        let who = app.session.state.user(m.author).map(|u| u.name().to_string()).unwrap_or_default();
                        if ui
                            .add(egui::Label::new(RichText::new(format!("{who}: {}", snippet(&m.text, 90))).color(t.text)).sense(Sense::click()))
                            .clicked()
                        {
                            acts.push(Act::OpenAt(c, m.id));
                        }
                    }
                });
            }
            let muted = app.session.state.muted.get(&me).is_some_and(|s| s.contains(&c));
            card(ui, &mut |ui| {
                if ui.button(if muted { "Unmute conversation" } else { "Mute conversation" }).clicked() {
                    acts.push(Act::Run("channel.mute", json!({ "channel": c.to_string() })));
                }
                if ch.kind != ChannelKind::Direct && ch.kind != ChannelKind::Group && app.session.state.workspace.default_channel != Some(c) {
                    if ui.button(RichText::new("Leave channel").color(t.danger)).clicked() {
                        acts.push(Act::Run("channel.leave", json!({ "channel": c.to_string() })));
                    }
                    if ui.button(if ch.archived { "Unarchive channel" } else { "Archive channel" }).clicked() {
                        acts.push(Act::Run(if ch.archived { "channel.unarchive" } else { "channel.archive" }, json!({ "channel": c.to_string() })));
                    }
                }
            });
        });
    });
}

fn result_list(app: &mut ChatApp, ui: &mut Ui, msgs: &[Message], empty: &str, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    let me = app.session.state.user(app.session.me.unwrap_or_default()).map(|u| u.handle.clone()).unwrap_or_default();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if msgs.is_empty() {
                ui.label(RichText::new(empty).color(t.muted));
            }
            for m in msgs {
                let author = app.session.state.user(m.author).cloned();
                let where_ = app.session.state.title(m.channel, app.session.me);
                let r =
                    Frame::new().fill(t.main_bg).stroke(Stroke::new(1.0, t.border)).corner_radius(10).inner_margin(Margin::same(10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(format!("{where_} · {}", app.local(m.ts).day_label())).small().color(t.muted));
                        ui.horizontal_top(|ui| {
                            avatar::show(ui, 28.0, author.as_ref(), &t, None);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(author.as_ref().map(|u| u.name().to_string()).unwrap_or_default()).strong().color(t.strong),
                                    );
                                    ui.label(RichText::new(app.local(m.ts).clock()).small().color(t.muted));
                                });
                                let _ = crate::markup::show(ui, &m.text, &t, &me);
                            });
                        });
                    });
                if r.response.interact(Sense::click()).on_hover_text("Jump to message").clicked() {
                    acts.push(Act::OpenAt(m.channel, m.id));
                }
                ui.add_space(8.0);
            }
        });
    });
}

fn results(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    let q = app.session.search_query.clone();
    let n = app.session.search_hits.len();
    title_bar(app, ui, "Search", &format!("{n} result{} for “{q}”", if n == 1 { "" } else { "s" }), acts);
    let msgs: Vec<Message> = app.session.search_hits.iter().filter_map(|h| app.session.state.message(h.message)).cloned().collect();
    result_list(app, ui, &msgs, "No messages match. Try fewer words, or modifiers like in:#channel from:@person has:link before:2026-11-01.", acts);
}

fn activity(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    title_bar(app, ui, "Activity", "Mentions", acts);
    let me = app.session.me.unwrap_or_default();
    let msgs: Vec<Message> = app.session.state.mentions_of(me, 100).into_iter().cloned().collect();
    result_list(app, ui, &msgs, "Nothing yet. When someone mentions you, it shows up here.", acts);
}

fn saved(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    title_bar(app, ui, "Later", "Saved items", acts);
    let me = app.session.me.unwrap_or_default();
    let ids: Vec<_> = app.session.state.saved.get(&me).map(|s| s.iter().rev().copied().collect()).unwrap_or_default();
    let msgs: Vec<Message> = ids.iter().filter_map(|id| app.session.state.message(*id)).filter(|m| !m.deleted).cloned().collect();
    result_list(app, ui, &msgs, "Save messages for later from a message's hover menu.", acts);
}

fn profile(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    let Some(u) = app.session.view.profile.and_then(|u| app.session.state.user(u)).cloned() else { return };
    title_bar(app, ui, "Profile", "", acts);
    let t = app.tokens.clone();
    Frame::new().inner_margin(Margin::same(20)).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            avatar::show(ui, 120.0, Some(&u), &t, Some(t.pane_bg));
            ui.add_space(10.0);
            ui.label(RichText::new(u.name()).font(FontId::proportional(22.0)).strong().color(t.strong));
            if !u.title.is_empty() {
                ui.label(RichText::new(&u.title).color(t.text));
            }
            ui.label(RichText::new(format!("@{} · {}", u.handle, u.presence.label())).color(t.muted));
            if !u.status.text.is_empty() || !u.status.emoji.is_empty() {
                let g = emoji::lookup(&u.status.emoji).unwrap_or("");
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{g} {}", u.status.text)).color(t.text));
            }
            ui.add_space(14.0);
            if Some(u.id) != app.session.me {
                let b = egui::Button::new(RichText::new("Message").color(t.accent_fg).strong())
                    .fill(t.accent)
                    .corner_radius(6)
                    .min_size(vec2(160.0, 32.0));
                if ui.add(b).clicked() {
                    acts.push(Act::Run("dm.open", json!({ "users": [format!("@{}", u.handle)] })));
                }
            } else if ui.button("Update your status").clicked() {
                acts.push(Act::Ui("ui.setStatus"));
            }
            if ui.button("Copy handle").clicked() {
                acts.push(Act::Copy(format!("@{}", u.handle)));
            }
        });
    });
}
