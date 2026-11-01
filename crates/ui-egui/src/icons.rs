//! The icon set, drawn in code from lines and circles (original artwork; no files, no fonts).
//! Each icon fills the square `r` and uses the colour `c`.

use egui::{Color32, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Hash,
    Lock,
    Home,
    Chat,
    Bell,
    Bookmark,
    People,
    Search,
    Plus,
    ChevronDown,
    ChevronRight,
    ArrowLeft,
    ArrowRight,
    Star,
    StarFilled,
    Smile,
    Thread,
    Pencil,
    Trash,
    Pin,
    More,
    Close,
    Send,
    At,
    Info,
    Compose,
    Link,
    List,
    Quote,
    Code,
    Moon,
    Sun,
    Mute,
    Archive,
    Shield,
}

fn s(c: Color32, w: f32) -> Stroke {
    Stroke::new(w, c)
}

fn at(r: Rect, x: f32, y: f32) -> Pos2 {
    pos2(r.left() + r.width() * x, r.top() + r.height() * y)
}

fn poly(p: &Painter, r: Rect, pts: &[(f32, f32)], st: Stroke) {
    for w in pts.windows(2) {
        if let [a, b] = w {
            p.line_segment([at(r, a.0, a.1), at(r, b.0, b.1)], st);
        }
    }
}

fn arc(p: &Painter, center: Pos2, radius: f32, from: f32, to: f32, st: Stroke) {
    let n = 18;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let t = from + (to - from) * (i as f32 / n as f32);
            center + vec2(t.cos(), t.sin()) * radius
        })
        .collect();
    p.add(egui::Shape::line(pts, st));
}

/// Paints `icon` into `r`.
#[allow(clippy::cognitive_complexity)]
pub fn paint(p: &Painter, r: Rect, icon: Icon, c: Color32) {
    let w = (r.width() / 11.0).max(1.2);
    let st = s(c, w);
    let ctr = r.center();
    let rad = r.width() * 0.5;
    use std::f32::consts::{PI, TAU};
    match icon {
        Icon::Hash => {
            poly(p, r, &[(0.38, 0.15), (0.30, 0.85)], st);
            poly(p, r, &[(0.70, 0.15), (0.62, 0.85)], st);
            poly(p, r, &[(0.15, 0.38), (0.85, 0.38)], st);
            poly(p, r, &[(0.13, 0.62), (0.83, 0.62)], st);
        }
        Icon::Lock => {
            let body = Rect::from_min_max(at(r, 0.2, 0.45), at(r, 0.8, 0.9));
            p.rect_filled(body, 2.0, c);
            arc(p, at(r, 0.5, 0.45), r.width() * 0.2, PI, TAU, st);
            poly(p, r, &[(0.3, 0.45), (0.3, 0.38)], st);
            poly(p, r, &[(0.7, 0.45), (0.7, 0.38)], st);
        }
        Icon::Home => {
            poly(p, r, &[(0.12, 0.5), (0.5, 0.15), (0.88, 0.5)], st);
            poly(p, r, &[(0.25, 0.42), (0.25, 0.85), (0.75, 0.85), (0.75, 0.42)], st);
            poly(p, r, &[(0.43, 0.85), (0.43, 0.62), (0.57, 0.62), (0.57, 0.85)], st);
        }
        Icon::Chat => {
            p.rect_stroke(Rect::from_min_max(at(r, 0.12, 0.18), at(r, 0.88, 0.7)), 4.0, st, egui::StrokeKind::Middle);
            poly(p, r, &[(0.3, 0.7), (0.25, 0.88), (0.48, 0.7)], st);
        }
        Icon::Bell => {
            arc(p, at(r, 0.5, 0.45), rad * 0.55, PI, TAU, st);
            poly(p, r, &[(0.22, 0.45), (0.22, 0.7), (0.12, 0.78), (0.88, 0.78), (0.78, 0.7), (0.78, 0.45)], st);
            p.circle_filled(at(r, 0.5, 0.88), w * 1.1, c);
        }
        Icon::Bookmark => poly(p, r, &[(0.25, 0.12), (0.75, 0.12), (0.75, 0.9), (0.5, 0.7), (0.25, 0.9), (0.25, 0.12)], st),
        Icon::People => {
            p.circle_stroke(at(r, 0.38, 0.33), rad * 0.2, st);
            arc(p, at(r, 0.38, 0.88), rad * 0.42, PI, TAU, st);
            p.circle_stroke(at(r, 0.7, 0.36), rad * 0.15, st);
            arc(p, at(r, 0.72, 0.84), rad * 0.3, PI * 1.2, TAU, st);
        }
        Icon::Search => {
            p.circle_stroke(at(r, 0.43, 0.43), rad * 0.5, st);
            poly(p, r, &[(0.62, 0.62), (0.88, 0.88)], s(c, w * 1.3));
        }
        Icon::Plus => {
            poly(p, r, &[(0.5, 0.18), (0.5, 0.82)], st);
            poly(p, r, &[(0.18, 0.5), (0.82, 0.5)], st);
        }
        Icon::ChevronDown => poly(p, r, &[(0.25, 0.4), (0.5, 0.65), (0.75, 0.4)], st),
        Icon::ChevronRight => poly(p, r, &[(0.4, 0.25), (0.65, 0.5), (0.4, 0.75)], st),
        Icon::ArrowLeft => {
            poly(p, r, &[(0.82, 0.5), (0.18, 0.5)], st);
            poly(p, r, &[(0.42, 0.26), (0.18, 0.5), (0.42, 0.74)], st);
        }
        Icon::ArrowRight => {
            poly(p, r, &[(0.18, 0.5), (0.82, 0.5)], st);
            poly(p, r, &[(0.58, 0.26), (0.82, 0.5), (0.58, 0.74)], st);
        }
        Icon::Star | Icon::StarFilled => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = -PI / 2.0 + i as f32 * PI / 5.0;
                    let rr = if i % 2 == 0 { rad * 0.9 } else { rad * 0.4 };
                    ctr + vec2(a.cos(), a.sin()) * rr
                })
                .collect();
            if icon == Icon::StarFilled {
                p.add(egui::Shape::convex_polygon(pts.clone(), c, Stroke::NONE));
                // The concave star is filled as a pentagon plus its points.
                let inner: Vec<Pos2> = pts.iter().skip(1).step_by(2).copied().collect();
                p.add(egui::Shape::convex_polygon(inner, c, Stroke::NONE));
            }
            p.add(egui::Shape::closed_line(pts, st));
        }
        Icon::Smile => {
            p.circle_stroke(ctr, rad * 0.8, st);
            p.circle_filled(at(r, 0.36, 0.4), w * 0.9, c);
            p.circle_filled(at(r, 0.64, 0.4), w * 0.9, c);
            arc(p, ctr, rad * 0.45, 0.2 * PI, 0.8 * PI, st);
        }
        Icon::Thread => {
            p.rect_stroke(Rect::from_min_max(at(r, 0.12, 0.15), at(r, 0.88, 0.68)), 4.0, st, egui::StrokeKind::Middle);
            poly(p, r, &[(0.3, 0.68), (0.25, 0.88), (0.48, 0.68)], st);
            poly(p, r, &[(0.28, 0.34), (0.72, 0.34)], st);
            poly(p, r, &[(0.28, 0.5), (0.58, 0.5)], st);
        }
        Icon::Pencil => {
            poly(p, r, &[(0.2, 0.8), (0.25, 0.62), (0.68, 0.18), (0.84, 0.34), (0.4, 0.78), (0.2, 0.8)], st);
            poly(p, r, &[(0.6, 0.26), (0.76, 0.42)], st);
        }
        Icon::Trash => {
            poly(p, r, &[(0.15, 0.25), (0.85, 0.25)], st);
            poly(p, r, &[(0.4, 0.25), (0.4, 0.13), (0.6, 0.13), (0.6, 0.25)], st);
            poly(p, r, &[(0.24, 0.25), (0.3, 0.88), (0.7, 0.88), (0.76, 0.25)], st);
        }
        Icon::Pin => {
            poly(p, r, &[(0.35, 0.12), (0.65, 0.12)], st);
            poly(p, r, &[(0.4, 0.12), (0.38, 0.45), (0.25, 0.58), (0.75, 0.58), (0.62, 0.45), (0.6, 0.12)], st);
            poly(p, r, &[(0.5, 0.58), (0.5, 0.9)], st);
        }
        Icon::More => {
            for y in [0.25, 0.5, 0.75] {
                p.circle_filled(at(r, 0.5, y), w * 1.1, c);
            }
        }
        Icon::Close => {
            poly(p, r, &[(0.25, 0.25), (0.75, 0.75)], st);
            poly(p, r, &[(0.75, 0.25), (0.25, 0.75)], st);
        }
        Icon::Send => {
            let pts = vec![at(r, 0.12, 0.18), at(r, 0.9, 0.5), at(r, 0.12, 0.82), at(r, 0.26, 0.5)];
            p.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
        }
        Icon::At => {
            p.circle_stroke(ctr, rad * 0.25, st);
            arc(p, ctr, rad * 0.75, 0.15 * PI, 1.95 * PI, st);
            poly(p, r, &[(0.62, 0.38), (0.62, 0.6), (0.7, 0.66), (0.84, 0.6)], st);
        }
        Icon::Info => {
            p.circle_stroke(ctr, rad * 0.8, st);
            poly(p, r, &[(0.5, 0.45), (0.5, 0.75)], st);
            p.circle_filled(at(r, 0.5, 0.3), w * 0.9, c);
        }
        Icon::Compose => {
            poly(p, r, &[(0.55, 0.18), (0.18, 0.18), (0.18, 0.82), (0.82, 0.82), (0.82, 0.45)], st);
            poly(p, r, &[(0.42, 0.6), (0.45, 0.48), (0.8, 0.12), (0.9, 0.22), (0.55, 0.57), (0.42, 0.6)], st);
        }
        Icon::Link => {
            p.rect_stroke(Rect::from_center_size(at(r, 0.36, 0.6), vec2(r.width() * 0.42, r.height() * 0.26)), 5.0, st, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_center_size(at(r, 0.64, 0.4), vec2(r.width() * 0.42, r.height() * 0.26)), 5.0, st, egui::StrokeKind::Middle);
        }
        Icon::List => {
            for y in [0.28, 0.5, 0.72] {
                p.circle_filled(at(r, 0.2, y), w, c);
                poly(p, r, &[(0.36, y), (0.86, y)], st);
            }
        }
        Icon::Quote => {
            poly(p, r, &[(0.2, 0.18), (0.2, 0.82)], s(c, w * 1.4));
            for y in [0.35, 0.5, 0.65] {
                poly(p, r, &[(0.38, y), (0.84, y)], st);
            }
        }
        Icon::Code => {
            poly(p, r, &[(0.36, 0.26), (0.12, 0.5), (0.36, 0.74)], st);
            poly(p, r, &[(0.64, 0.26), (0.88, 0.5), (0.64, 0.74)], st);
            poly(p, r, &[(0.56, 0.2), (0.44, 0.8)], st);
        }
        Icon::Moon => {
            arc(p, ctr, rad * 0.75, 0.6 * PI, 1.9 * PI, st);
            arc(p, at(r, 0.68, 0.36), rad * 0.5, 0.45 * PI, 1.1 * PI, st);
        }
        Icon::Sun => {
            p.circle_stroke(ctr, rad * 0.32, st);
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([ctr + d * rad * 0.55, ctr + d * rad * 0.85], st);
            }
        }
        Icon::Mute => {
            paint(p, r, Icon::Bell, c);
            poly(p, r, &[(0.12, 0.12), (0.88, 0.88)], st);
        }
        Icon::Shield => {
            poly(p, r, &[(0.5, 0.1), (0.85, 0.24), (0.8, 0.58), (0.5, 0.9), (0.2, 0.58), (0.15, 0.24), (0.5, 0.1)], st);
            poly(p, r, &[(0.36, 0.5), (0.47, 0.62), (0.66, 0.38)], st);
        }
        Icon::Archive => {
            p.rect_stroke(Rect::from_min_max(at(r, 0.12, 0.18), at(r, 0.88, 0.36)), 2.0, st, egui::StrokeKind::Middle);
            poly(p, r, &[(0.18, 0.36), (0.18, 0.84), (0.82, 0.84), (0.82, 0.36)], st);
            poly(p, r, &[(0.4, 0.52), (0.6, 0.52)], st);
        }
    }
}

/// A square, frameless icon button with a hover background and tooltip.
pub fn button(ui: &mut Ui, icon: Icon, size: f32, color: Color32, hover_bg: Color32, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, if tip.is_empty() { format!("{icon:?}") } else { tip.to_string() }));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, hover_bg);
    }
    let inset = (size * 0.24).round();
    paint(ui.painter(), rect.shrink(inset), icon, color);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}
