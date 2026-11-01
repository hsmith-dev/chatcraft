//! The Matrix SAS emoji table (from the Matrix specification) and which glyphs the bundled
//! fonts can draw. In a security check a blank box would be worse than no picture, so the
//! verification dialog always shows the emoji's name and shows the picture only when it draws.

/// The 64 SAS emoji, in specification order (the glyph test walks it).
#[cfg_attr(not(test), allow(dead_code))]
pub const TABLE: [&str; 64] = [
    "🐶", "🐱", "🦁", "🐎", "🦄", "🐷", "🐘", "🐰", "🐼", "🐓", "🐧", "🐢", "🐟", "🐙", "🦋", "🌷", "🌳", "🌵", "🍄", "🌏", "🌙", "☁️", "🔥", "🍌",
    "🍎", "🍓", "🌽", "🍕", "🎂", "❤️", "😀", "🤖", "🎩", "👓", "🔧", "🎅", "👍", "☂️", "⌛", "⏰", "🎁", "💡", "📕", "✏️", "📎", "✂️", "🔒", "🔑",
    "🔨", "☎️", "🏁", "🚂", "🚲", "✈️", "🚀", "🏆", "⚽", "🎸", "🎺", "🔔", "⚓", "🎧", "📁", "📌",
];

/// SAS emoji the bundled fonts have no glyph for (kept true by `sas_glyphs_are_known`).
pub const UNDRAWABLE: &[&str] = &["🦁", "🦄", "🦋", "🤖"];

pub fn drawable(symbol: &str) -> bool {
    !UNDRAWABLE.contains(&symbol)
}
