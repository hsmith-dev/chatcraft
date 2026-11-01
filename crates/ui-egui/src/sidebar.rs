//! The conversation sidebar: starred, channels, direct messages.

use chatcraft_engine::LinkStatus;
use chatcraft_engine::model::{ChannelKind, ChannelSummary};
use egui::{Align2, FontId, RichText, Sense, Ui, vec2};

use crate::icons::{self, Icon};
use crate::{Act, ChatApp, Dialog, avatar, badge};

fn section(ui: &mut Ui, app: &ChatApp, title: &str, closed: bool) -> (bool, egui::Rect) {
    let t = &app.tokens;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    if resp.hovered() {
        ui.painter().rect_filled(rect.shrink2(vec2(8.0, 0.0)), 6.0, t.side_hover);
    }
    let chev = egui::Rect::from_center_size(rect.left_center() + vec2(22.0, 0.0), vec2(12.0, 12.0));
    icons::paint(ui.painter(), chev, if closed { Icon::ChevronRight } else { Icon::ChevronDown }, t.side_fg);
    ui.painter().text(rect.left_center() + vec2(34.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(14.0), t.side_fg);
    (resp.clicked(), rect)
}

fn row(ui: &mut Ui, app: &ChatApp, c: &ChannelSummary, acts: &mut Vec<Act>) {
    let t = &app.tokens;
    let s = &app.session;
    let selected = s.view.channel == Some(c.id);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let inner = rect.shrink2(vec2(8.0, 0.0));
    if selected {
        ui.painter().rect_filled(inner, 6.0, t.side_active_bg);
    } else if resp.hovered() {
        ui.painter().rect_filled(inner, 6.0, t.side_hover);
    }
    let label = if c.mentions > 0 {
        format!("{} ({} mentions)", c.title, c.mentions)
    } else if c.unread > 0 {
        format!("{} (unread)", c.title)
    } else {
        c.title.clone()
    };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label.clone()));
    let unread = c.unread > 0 && !c.muted && !selected;
    let fg = if selected {
        t.side_active_fg
    } else if unread {
        t.side_strong
    } else if c.muted {
        t.side_muted
    } else {
        t.side_fg
    };
    let icon_rect = egui::Rect::from_center_size(inner.left_center() + vec2(16.0, 0.0), vec2(18.0, 18.0));
    match c.kind {
        ChannelKind::Public => icons::paint(ui.painter(), icon_rect.shrink(2.0), Icon::Hash, fg),
        ChannelKind::Private => icons::paint(ui.painter(), icon_rect.shrink(3.0), Icon::Lock, fg),
        ChannelKind::Direct => {
            let u = c.peer.and_then(|p| s.state.user(p));
            avatar::paint(ui, icon_rect.expand(1.0), u, t);
            avatar::presence_dot(ui, icon_rect.expand(1.0), u, t, if selected { t.side_active_bg } else { t.side_bg });
        }
        ChannelKind::Group => {
            ui.painter().rect_filled(icon_rect.expand(1.0), 4.0, t.side_hover);
            let n = s.state.channel(c.id).map(|ch| ch.members.len().saturating_sub(1)).unwrap_or(0);
            ui.painter().text(icon_rect.center(), Align2::CENTER_CENTER, n.to_string(), FontId::proportional(11.0), fg);
        }
    }
    let title = if c.kind.is_dm() { c.title.clone() } else { c.title.trim_start_matches('#').to_string() };
    let font = FontId::proportional(14.5);
    let badge_w = if c.mentions > 0 && !selected { 34.0 } else { 6.0 };
    let max_w = (inner.width() - 40.0 - badge_w).max(20.0);
    let mut job = egui::text::LayoutJob::simple_singleline(title, font, fg);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
    let galley = ui.painter().layout_job(job);
    let pos = inner.left_center() + vec2(32.0, -galley.size().y / 2.0);
    ui.painter().galley(pos, galley.clone(), fg);
    if unread {
        // A faux-bold pass for unread conversations (the default font has no bold face).
        ui.painter().galley(pos + vec2(0.45, 0.0), galley, fg);
    }
    if c.mentions > 0 && !selected {
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(inner.right_center() - vec2(30.0, 9.0), vec2(30.0, 18.0))));
        badge(&mut child, c.mentions, t.badge, t.badge_fg);
    }
    if s.state.channel(c.id).is_some_and(|ch| ch.encrypted) && !c.kind.is_dm() && c.kind != ChannelKind::Private {
        // Public channels are rarely encrypted; mark the exception (private rooms already show a lock).
        let r =
            egui::Rect::from_center_size(inner.right_center() - vec2(if c.mentions > 0 && !selected { 44.0 } else { 14.0 }, 0.0), vec2(12.0, 12.0));
        icons::paint(ui.painter(), r, Icon::Shield, fg);
    }
    let resp = if let Some(peer) = c.peer.and_then(|p| s.state.user(p)) {
        let mut tip = format!("{} · {}", peer.name(), peer.presence.label());
        if !peer.status.text.is_empty() {
            tip.push_str(&format!("\n{}", peer.status.text));
        }
        resp.on_hover_text(tip)
    } else {
        resp
    };
    if resp.clicked() {
        acts.push(Act::Open(c.id));
    }
}

fn add_row(ui: &mut Ui, app: &ChatApp, label: &str) -> egui::Response {
    let t = &app.tokens;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let inner = rect.shrink2(vec2(8.0, 0.0));
    if resp.hovered() {
        ui.painter().rect_filled(inner, 6.0, t.side_hover);
    }
    let r = egui::Rect::from_center_size(inner.left_center() + vec2(16.0, 0.0), vec2(18.0, 18.0));
    ui.painter().rect_filled(r, 4.0, t.side_hover);
    icons::paint(ui.painter(), r.shrink(3.0), Icon::Plus, t.side_fg);
    ui.painter().text(inner.left_center() + vec2(32.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(14.0), t.side_fg);
    resp
}

pub(crate) fn show(app: &mut ChatApp, ui: &mut Ui, acts: &mut Vec<Act>) {
    let t = app.tokens.clone();
    // Header: workspace name and compose.
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(RichText::new(&app.session.state.workspace.name).font(FontId::proportional(18.0)).strong().color(t.side_strong));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            if icons::button(ui, Icon::Compose, 30.0, t.side_strong, t.side_hover, "New message (Ctrl/Cmd+N)").clicked() {
                acts.push(Act::Dialog(Dialog::NewMessage { query: String::new(), picked: Vec::new() }));
            }
        });
    });
    ui.add_space(6.0);
    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), egui::Stroke::new(1.0, t.side_hover));
    ui.add_space(8.0);
    let bar = app.session.sidebar();
    egui::ScrollArea::vertical().id_salt("sidebar-scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        if !bar.starred.is_empty() {
            section(ui, app, "Starred", false);
            for c in &bar.starred {
                row(ui, app, c, acts);
            }
            ui.add_space(10.0);
        }
        let (toggle, _) = section(ui, app, "Channels", app.st.channels_closed);
        if toggle {
            app.st.channels_closed = !app.st.channels_closed;
        }
        for c in &bar.channels {
            // A closed section still shows the open and unread conversations.
            if !app.st.channels_closed || app.session.view.channel == Some(c.id) || c.unread > 0 {
                row(ui, app, c, acts);
            }
        }
        if !app.st.channels_closed {
            let r = add_row(ui, app, "Add channels");
            if r.clicked() {
                acts.push(Act::CloseMenus);
                app.st.add_menu = Some(r.rect.left_bottom() + vec2(16.0, 2.0));
            }
        }
        ui.add_space(10.0);
        let (toggle, _) = section(ui, app, "Direct messages", app.st.dms_closed);
        if toggle {
            app.st.dms_closed = !app.st.dms_closed;
        }
        for c in &bar.dms {
            if !app.st.dms_closed || app.session.view.channel == Some(c.id) || c.unread > 0 {
                row(ui, app, c, acts);
            }
        }
        if !app.st.dms_closed && add_row(ui, app, "New message").clicked() {
            acts.push(Act::Ui("ui.newMessage"));
        }
        ui.add_space(16.0);
    });
    // Connection status, only when it isn't fine.
    match app.session.link_status() {
        LinkStatus::Online => {}
        LinkStatus::Connecting => status_line(ui, &t, "Connecting…"),
        LinkStatus::Offline(why) => status_line(ui, &t, &why),
    }
}

fn status_line(ui: &mut Ui, t: &crate::Tokens, text: &str) {
    egui::Frame::new().fill(t.badge).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(text).color(t.badge_fg).small());
    });
}
