//! Avatars: initials on a coloured rounded tile, with an optional presence dot.

use chatcraft_engine::model::User;
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, vec2};

use crate::theme::Tokens;

/// Paints an avatar tile at `rect`.
pub fn paint(ui: &Ui, rect: Rect, user: Option<&User>, t: &Tokens) {
    let p = ui.painter();
    let (color, initials, bot) = match user {
        Some(u) => (t.avatar(u.color), u.initials(), u.bot),
        None => (t.faint, "?".to_string(), false),
    };
    let radius = (rect.width() * 0.22).round();
    p.rect_filled(rect, radius, color);
    let size = rect.width() * if initials.chars().count() > 1 { 0.38 } else { 0.46 };
    p.text(rect.center() + vec2(0.0, rect.height() * 0.02), Align2::CENTER_CENTER, initials, FontId::proportional(size), egui::Color32::WHITE);
    if bot {
        // A small notch marks integrations.
        let r = Rect::from_min_size(rect.right_top() + vec2(-rect.width() * 0.32, 0.0), Vec2::splat(rect.width() * 0.32));
        p.rect_filled(r, 2.0, t.strong.linear_multiply(0.5));
    }
}

/// Paints a presence dot at the bottom-right of `rect`, ringed in `ring`.
pub fn presence_dot(ui: &Ui, rect: Rect, user: Option<&User>, t: &Tokens, ring: egui::Color32) {
    let Some(u) = user else { return };
    let r = (rect.width() * 0.17).max(3.5);
    let c: Pos2 = rect.right_bottom() - vec2(r * 0.6, r * 0.6);
    match t.presence(u.presence) {
        Some(col) => {
            ui.painter().circle_filled(c, r + 1.5, ring);
            ui.painter().circle_filled(c, r, col);
        }
        None => {
            ui.painter().circle_filled(c, r + 1.5, ring);
            ui.painter().circle_stroke(c, r - 0.5, Stroke::new(1.5, t.presence_away));
        }
    }
}

/// Allocates and paints an avatar; returns its (clickable) response.
pub fn show(ui: &mut Ui, size: f32, user: Option<&User>, t: &Tokens, ring: Option<egui::Color32>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    paint(ui, rect, user, t);
    if let Some(ring) = ring {
        presence_dot(ui, rect, user, t, ring);
    }
    resp
}
