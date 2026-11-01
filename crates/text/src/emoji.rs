//! Emoji shortcodes. Glyphs are Unicode characters (rendered by the system or bundled open
//! fonts), never images from another product.
//!
//! Only emoji the bundled open fonts can draw are offered (`ui-egui`'s
//! `every_offered_emoji_has_a_glyph` test enforces it). An unknown shortcode still shows as
//! readable `:name:` text. A colour emoji font is a tracked gap.

/// `(shortcode, glyph)`, in picker order: the reactions people reach for first.
pub const ALL: &[(&str, &str)] = &[
    ("+1", "👍"),
    ("thumbsup", "👍"),
    ("-1", "👎"),
    ("thumbsdown", "👎"),
    ("heart", "❤️"),
    ("joy", "😂"),
    ("smile", "😄"),
    ("grinning", "😀"),
    ("laughing", "😆"),
    ("wink", "😉"),
    ("blush", "😊"),
    ("eyes", "👀"),
    ("tada", "🎉"),
    ("fire", "🔥"),
    ("rocket", "🚀"),
    ("clap", "👏"),
    ("raised_hands", "🙌"),
    ("pray", "🙏"),
    ("100", "💯"),
    ("white_check_mark", "✅"),
    ("heavy_check_mark", "✔️"),
    ("x", "❌"),
    ("warning", "⚠️"),
    ("question", "❓"),
    ("exclamation", "❗"),
    ("bulb", "💡"),
    ("star", "⭐"),
    ("sparkles", "✨"),
    ("zap", "⚡"),
    ("coffee", "☕"),
    ("wave", "👋"),
    ("ok_hand", "👌"),
    ("muscle", "💪"),
    ("point_up", "☝️"),
    ("point_right", "👉"),
    ("cry", "😢"),
    ("sob", "😭"),
    ("sweat_smile", "😅"),
    ("heart_eyes", "😍"),
    ("sunglasses", "😎"),
    ("neutral_face", "😐"),
    ("confused", "😕"),
    ("scream", "😱"),
    ("memo", "📝"),
    ("calendar", "📅"),
    ("pushpin", "📌"),
    ("paperclip", "📎"),
    ("link", "🔗"),
    ("lock", "🔒"),
    ("key", "🔑"),
    ("bell", "🔔"),
    ("mega", "📣"),
    ("speech_balloon", "💬"),
    ("hourglass", "⌛"),
    ("stopwatch", "⏱️"),
    ("chart_with_upwards_trend", "📈"),
    ("bar_chart", "📊"),
    ("computer", "💻"),
    ("gear", "⚙️"),
    ("wrench", "🔧"),
    ("hammer", "🔨"),
    ("bug", "🐛"),
    ("package", "📦"),
    ("art", "🎨"),
    ("books", "📚"),
    ("pencil", "✏️"),
    ("sun", "☀️"),
    ("sunny", "☀️"),
    ("cloud", "☁️"),
    ("rainbow", "🌈"),
    ("snowflake", "❄️"),
    ("seedling", "🌱"),
    ("palm_tree", "🌴"),
    ("airplane", "✈️"),
    ("house", "🏠"),
    ("pizza", "🍕"),
    ("cake", "🍰"),
    ("beers", "🍻"),
    ("trophy", "🏆"),
    ("medal", "🏅"),
    ("dart", "🎯"),
    ("gift", "🎁"),
    ("balloon", "🎈"),
    ("headphones", "🎧"),
    ("musical_note", "🎵"),
    ("camera", "📷"),
    ("movie_camera", "🎥"),
    ("ghost", "👻"),
    ("jack_o_lantern", "🎃"),
    ("dog", "🐶"),
    ("cat", "🐱"),
    ("turtle", "🐢"),
    ("red_circle", "🔴"),
    ("large_blue_circle", "🔵"),
    ("arrow_right", "➡️"),
    ("arrow_left", "⬅️"),
    ("arrow_up", "⬆️"),
    ("arrow_down", "⬇️"),
    ("repeat", "🔁"),
    ("no_entry", "⛔"),
    ("construction", "🚧"),
    ("house_with_garden", "🏡"),
    ("palm", "🌴"),
];

/// The quick reactions offered on hover, as shortcodes.
pub const QUICK: &[&str] = &["+1", "heart", "joy", "eyes", "tada", "white_check_mark"];

/// The Unicode glyph for a shortcode, if known.
pub fn lookup(name: &str) -> Option<&'static str> {
    ALL.iter().find(|(n, _)| *n == name).map(|(_, g)| *g)
}

/// A shortcode for a glyph, if known (first match).
pub fn shortcode_for(glyph: &str) -> Option<&'static str> {
    ALL.iter().find(|(_, g)| *g == glyph).map(|(n, _)| *n)
}

/// Shortcodes are `a-z 0-9 _ + -`, 1–64 bytes.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= chatcraft_core_limits::MAX_EMOJI_NAME
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'+' | b'-'))
}

/// Shortcodes whose name contains `query` (for the picker search), deduplicated by glyph.
pub fn search(query: &str) -> Vec<(&'static str, &'static str)> {
    let q = query.trim().trim_matches(':').to_lowercase();
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for (n, g) in ALL {
        if (q.is_empty() || n.contains(q.as_str())) && !seen.contains(g) {
            seen.push(g);
            out.push((*n, *g));
        }
    }
    out
}

/// Local copy of the core limit (text sits beside core at L1 without depending on it).
mod chatcraft_core_limits {
    pub const MAX_EMOJI_NAME: usize = 64;
}
