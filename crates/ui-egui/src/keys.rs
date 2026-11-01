//! Keyboard shortcuts: engine commands by their portable shortcut, plus the UI commands.

use egui::{Key, Modifiers};
use serde_json::json;

use crate::{Act, ChatApp, UI_COMMANDS};
use chatcraft_engine::{Pane, Session};

/// Parses `Mod+Shift+K` into modifiers and a key.
pub(crate) fn parse(shortcut: &str) -> Option<(Modifiers, Key)> {
    let mut m = Modifiers::NONE;
    let mut key = None;
    for part in shortcut.split('+') {
        match part {
            "Mod" => m.command = true,
            "Shift" => m.shift = true,
            "Alt" => m.alt = true,
            "Ctrl" => m.ctrl = true,
            "[" => key = Some(Key::OpenBracket),
            "]" => key = Some(Key::CloseBracket),
            "/" => key = Some(Key::Slash),
            "," => key = Some(Key::Comma),
            "Up" => key = Some(Key::ArrowUp),
            "Down" => key = Some(Key::ArrowDown),
            k => key = Key::from_name(k),
        }
    }
    key.map(|k| (m, k))
}

pub(crate) fn handle(app: &mut ChatApp, ctx: &egui::Context, acts: &mut Vec<Act>) {
    // Escape: close the innermost thing.
    if ctx.input(|i| i.key_pressed(Key::Escape) && i.modifiers.is_none()) {
        if app.st.dialog.is_some() {
            return; // the modal handles it
        }
        if app.st.picker.is_some() || app.st.menu.is_some() || app.st.profile_menu.is_some() || app.st.add_menu.is_some() {
            acts.push(Act::CloseMenus);
        } else if app.st.editing.is_some() {
            acts.push(Act::CancelEdit);
        } else if app.session.view.pane != Pane::None {
            acts.push(Act::Run("nav.closePane", json!({})));
        } else {
            acts.push(Act::Run("channel.markRead", json!({})));
        }
        return;
    }
    if app.st.dialog.is_some() {
        return;
    }
    for (id, _, key) in UI_COMMANDS {
        if let Some((m, k)) = parse(key)
            && ctx.input_mut(|i| i.consume_key(m, k))
        {
            acts.push(Act::Ui(id));
        }
    }
    for spec in Session::registry().all() {
        for key in spec.shortcut.split(" / ") {
            // Plain keys (Enter, Up, Escape) belong to the composer and are handled there.
            if !(key.contains("Mod+") || key.contains("Alt+") || key == "Shift+Escape") {
                continue;
            }
            // Ui commands own these.
            if UI_COMMANDS.iter().any(|(_, _, k)| *k == key) {
                continue;
            }
            if let Some((m, k)) = parse(key)
                && ctx.input_mut(|i| i.consume_key(m, k))
            {
                acts.push(Act::Run(spec.id, json!({})));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_parses() {
        for s in Session::registry().all() {
            for k in s.shortcut.split(" / ").filter(|k| !k.is_empty()) {
                assert!(parse(k).is_some(), "{}: {k}", s.id);
            }
        }
        for (id, _, k) in UI_COMMANDS {
            assert!(parse(k).is_some(), "{id}: {k}");
        }
    }
}
