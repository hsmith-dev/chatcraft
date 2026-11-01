//! Message bodies: `chatcraft_text` blocks rendered as egui widgets.

use chatcraft_engine::text::{Block, Inline, Style, parse};
use egui::{FontId, RichText, Sense, Stroke, Ui, vec2};

use crate::theme::Tokens;

/// Something clicked inside a message body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clicked {
    Person(String),
    Channel(String),
}

fn styled(text: &str, st: Style, t: &Tokens) -> RichText {
    let mut r = RichText::new(text).color(t.text);
    if st.bold {
        r = r.strong().color(t.strong);
    }
    if st.italic {
        r = r.italics();
    }
    if st.strike {
        r = r.strikethrough();
    }
    if st.code {
        r = r.monospace().color(t.code_fg).background_color(t.code_bg);
    }
    r
}

/// Inlines on wrapped lines. `me` is the viewer's handle (their mentions are highlighted).
fn inlines(ui: &mut Ui, items: &[Inline], t: &Tokens, me: &str, out: &mut Option<Clicked>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
        for it in items {
            match it {
                Inline::Text { text, style } => {
                    ui.label(styled(text, *style, t));
                }
                Inline::Link { url, label, style } => {
                    let mut rt = styled(label, *style, t).color(t.link);
                    if !style.code {
                        rt = rt.underline();
                    }
                    ui.hyperlink_to(rt, url).on_hover_text(url);
                }
                Inline::Mention { handle } => {
                    let bg = if handle == me { t.mention_me_bg } else { t.mention_bg };
                    let r =
                        ui.add(egui::Label::new(RichText::new(format!("@{handle}")).color(t.mention_fg).background_color(bg)).sense(Sense::click()));
                    if r.clicked() {
                        *out = Some(Clicked::Person(handle.clone()));
                    }
                }
                Inline::Broadcast { name } => {
                    ui.label(RichText::new(format!("@{name}")).color(t.mention_fg).background_color(t.mention_me_bg));
                }
                Inline::ChannelRef { name } => {
                    let r = ui.add(egui::Label::new(RichText::new(format!("#{name}")).color(t.link)).sense(Sense::click()));
                    if r.clicked() {
                        *out = Some(Clicked::Channel(name.clone()));
                    }
                }
                Inline::Emoji { name, glyph } => match glyph {
                    Some(g) => {
                        ui.label(RichText::new(*g).font(FontId::proportional(16.0))).on_hover_text(format!(":{name}:"));
                    }
                    None => {
                        ui.label(RichText::new(format!(":{name}:")).color(t.muted));
                    }
                },
            }
        }
    });
}

/// Renders a message body. Returns what was clicked, if anything.
pub fn show(ui: &mut Ui, src: &str, t: &Tokens, me: &str) -> Option<Clicked> {
    let mut out = None;
    let only_emoji = is_jumbo(src);
    if let Some(glyphs) = only_emoji {
        ui.label(RichText::new(glyphs).font(FontId::proportional(30.0)));
        return None;
    }
    for b in parse(src) {
        match &b {
            Block::Paragraph { inlines: items } => {
                if items.is_empty() {
                    ui.add_space(6.0);
                } else {
                    inlines(ui, items, t, me, &mut out);
                }
            }
            Block::Quote { inlines: items } => {
                ui.horizontal(|ui| {
                    let (bar, _) = ui.allocate_exact_size(vec2(3.0, 18.0), Sense::hover());
                    ui.painter().rect_filled(bar, 1.5, t.quote_bar);
                    ui.add_space(6.0);
                    ui.vertical(|ui| inlines(ui, items, t, me, &mut out));
                });
            }
            Block::Bullet { inlines: items } => {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("•").color(t.text));
                    ui.add_space(4.0);
                    ui.vertical(|ui| inlines(ui, items, t, me, &mut out));
                });
            }
            Block::Numbered { number, inlines: items } => {
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    ui.label(RichText::new(format!("{number}.")).color(t.text));
                    ui.add_space(4.0);
                    ui.vertical(|ui| inlines(ui, items, t, me, &mut out));
                });
            }
            Block::Code { text } => {
                egui::Frame::new()
                    .fill(t.code_bg)
                    .stroke(Stroke::new(1.0, t.border))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(text).monospace().color(t.text));
                    });
            }
        }
    }
    out
}

/// A message of one to three emoji (and nothing else) is shown large.
fn is_jumbo(src: &str) -> Option<String> {
    let blocks = parse(src);
    let [Block::Paragraph { inlines: items }] = blocks.as_slice() else { return None };
    let mut glyphs = String::new();
    let mut n = 0;
    for it in items {
        match it {
            Inline::Emoji { glyph: Some(g), .. } => {
                glyphs.push_str(g);
                n += 1;
            }
            Inline::Text { text, .. } if text.trim().is_empty() => {}
            _ => return None,
        }
    }
    (1..=3).contains(&n).then_some(glyphs)
}
