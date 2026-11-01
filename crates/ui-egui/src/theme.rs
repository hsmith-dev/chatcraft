//! Colour tokens. Every colour the UI paints comes from here. The palette is ChatCraft's own:
//! deep teal chrome with amber and vermilion accents.

use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Debug)]
pub struct Tokens {
    pub mode: Mode,
    pub rail_bg: Color32,
    pub rail_fg: Color32,
    pub rail_active: Color32,
    pub side_bg: Color32,
    pub side_fg: Color32,
    pub side_strong: Color32,
    pub side_muted: Color32,
    pub side_hover: Color32,
    pub side_active_bg: Color32,
    pub side_active_fg: Color32,
    pub top_bg: Color32,
    pub top_search_bg: Color32,
    pub top_fg: Color32,
    pub main_bg: Color32,
    pub pane_bg: Color32,
    pub text: Color32,
    pub strong: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub border: Color32,
    pub row_hover: Color32,
    pub row_focus: Color32,
    pub accent: Color32,
    pub accent_fg: Color32,
    pub badge: Color32,
    pub badge_fg: Color32,
    pub link: Color32,
    pub code_bg: Color32,
    pub code_fg: Color32,
    pub mention_bg: Color32,
    pub mention_fg: Color32,
    pub mention_me_bg: Color32,
    pub reaction_bg: Color32,
    pub reaction_border: Color32,
    pub reaction_mine_bg: Color32,
    pub reaction_mine_border: Color32,
    pub composer_bg: Color32,
    pub composer_border: Color32,
    pub toolbar_bg: Color32,
    pub toast_bg: Color32,
    pub toast_fg: Color32,
    pub danger: Color32,
    pub new_line: Color32,
    pub quote_bar: Color32,
    pub shadow: Color32,
    pub presence_active: Color32,
    pub presence_away: Color32,
    pub presence_dnd: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Tokens {
    pub fn light() -> Tokens {
        Tokens {
            mode: Mode::Light,
            rail_bg: hex(0x0B2326),
            rail_fg: hex(0xC9DEDB),
            rail_active: hex(0x2A5C5F),
            side_bg: hex(0x103236),
            side_fg: hex(0xB8CFCC),
            side_strong: hex(0xFFFFFF),
            side_muted: hex(0x7FA19D),
            side_hover: hex(0x1A464A),
            side_active_bg: hex(0xF2C14E),
            side_active_fg: hex(0x1B1A12),
            top_bg: hex(0x0B2326),
            top_search_bg: hex(0x24484B),
            top_fg: hex(0xE4EFEE),
            main_bg: hex(0xFFFFFF),
            pane_bg: hex(0xFBFBFA),
            text: hex(0x24292C),
            strong: hex(0x0F1416),
            muted: hex(0x6A7378),
            faint: hex(0xA5ACB0),
            border: hex(0xE2E5E6),
            row_hover: hex(0xF4F6F6),
            row_focus: hex(0xFFF4D6),
            accent: hex(0x0E7C73),
            accent_fg: hex(0xFFFFFF),
            badge: hex(0xD9462B),
            badge_fg: hex(0xFFFFFF),
            link: hex(0x1F6FB2),
            code_bg: hex(0xF3F4F4),
            code_fg: hex(0xB33A3A),
            mention_bg: hex(0xE3F1EF),
            mention_fg: hex(0x0C6A62),
            mention_me_bg: hex(0xFCEBC3),
            reaction_bg: hex(0xF1F3F3),
            reaction_border: hex(0xF1F3F3),
            reaction_mine_bg: hex(0xE1F2F0),
            reaction_mine_border: hex(0x3D9A91),
            composer_bg: hex(0xFFFFFF),
            composer_border: hex(0xC9CED0),
            toolbar_bg: hex(0xF7F8F8),
            toast_bg: hex(0x1E2A2C),
            toast_fg: hex(0xF4F7F7),
            danger: hex(0xC2361F),
            new_line: hex(0xD9462B),
            quote_bar: hex(0xD2D7D9),
            shadow: Color32::from_black_alpha(40),
            presence_active: hex(0x2BAC76),
            presence_away: hex(0x9AA3A7),
            presence_dnd: hex(0xD9462B),
        }
    }

    pub fn dark() -> Tokens {
        Tokens {
            mode: Mode::Dark,
            rail_bg: hex(0x07181A),
            rail_fg: hex(0xA9C3C0),
            rail_active: hex(0x1E4447),
            side_bg: hex(0x0C2326),
            side_fg: hex(0xA4BDBA),
            side_strong: hex(0xF4F8F8),
            side_muted: hex(0x6E8F8B),
            side_hover: hex(0x14353A),
            side_active_bg: hex(0xE8B640),
            side_active_fg: hex(0x15140E),
            top_bg: hex(0x07181A),
            top_search_bg: hex(0x1A3638),
            top_fg: hex(0xD9E6E4),
            main_bg: hex(0x16191B),
            pane_bg: hex(0x1A1E20),
            text: hex(0xD8DCDE),
            strong: hex(0xF3F5F6),
            muted: hex(0x959DA1),
            faint: hex(0x5F676B),
            border: hex(0x2C3235),
            row_hover: hex(0x1F2427),
            row_focus: hex(0x3A3220),
            accent: hex(0x2DA89C),
            accent_fg: hex(0x061412),
            badge: hex(0xE0583D),
            badge_fg: hex(0xFFFFFF),
            link: hex(0x6CB2EB),
            code_bg: hex(0x23292C),
            code_fg: hex(0xEF8A80),
            mention_bg: hex(0x173B37),
            mention_fg: hex(0x7FD6CB),
            mention_me_bg: hex(0x4A3B16),
            reaction_bg: hex(0x23292C),
            reaction_border: hex(0x23292C),
            reaction_mine_bg: hex(0x173B37),
            reaction_mine_border: hex(0x2DA89C),
            composer_bg: hex(0x1C2023),
            composer_border: hex(0x3A4145),
            toolbar_bg: hex(0x202528),
            toast_bg: hex(0xEDF1F1),
            toast_fg: hex(0x152022),
            danger: hex(0xF06A52),
            new_line: hex(0xE0583D),
            quote_bar: hex(0x434B4F),
            shadow: Color32::from_black_alpha(120),
            presence_active: hex(0x3CC48A),
            presence_away: hex(0x7B8488),
            presence_dnd: hex(0xE0583D),
        }
    }

    /// Avatar tile colours (picked per person by `User::color`).
    pub fn avatar(&self, i: u8) -> Color32 {
        const P: [u32; 12] = [0x0E7C73, 0xC2571A, 0x3B6FB6, 0x8A5A9E, 0x2E8B57, 0xB8423F, 0x7A6A1F, 0x2F7F9E, 0x9E4F7A, 0x5B7A2E, 0xA0662C, 0x4E5BA6];
        hex(P.get(usize::from(i) % P.len()).copied().unwrap_or(0x0E7C73))
    }

    pub fn presence(&self, p: chatcraft_engine::model::Presence) -> Option<Color32> {
        use chatcraft_engine::model::Presence;
        match p {
            Presence::Active => Some(self.presence_active),
            Presence::Away => Some(self.presence_away),
            Presence::Dnd => Some(self.presence_dnd),
            Presence::Offline => None,
        }
    }

    /// egui's own widget visuals, derived from the tokens.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.mode == Mode::Dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.main_bg;
        v.window_fill = self.main_bg;
        v.extreme_bg_color = self.composer_bg;
        v.faint_bg_color = self.row_hover;
        v.hyperlink_color = self.link;
        v.selection.bg_fill = self.accent.linear_multiply(0.35);
        v.selection.stroke = egui::Stroke::new(1.0, self.accent);
        v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, self.text);
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.border);
        v.window_stroke = egui::Stroke::new(1.0, self.border);
        v.window_corner_radius = egui::CornerRadius::same(10);
        v.override_text_color = None;
        ctx.set_visuals(v);
        ctx.global_style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(6.0, 4.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
            s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
            s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
            s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
        });
    }
}
