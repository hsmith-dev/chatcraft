//! Message markup (L1).
//!
//! People write messages the way they do in other chat apps: `*bold*` or `**bold**`, `_italic_`,
//! `~strike~`, `` `code` ``, fenced code blocks, `> quotes`, `- bullets`, `1. numbered`, links as
//! bare URLs or `<url|label>`, `@handle`, `@here`/`@channel`/`@everyone`, `#channel` and
//! `:emoji:` shortcodes. [`parse`] turns the source into [`Block`]s for the UI; the helpers pull
//! out what the hub and search need (mentions, plain text, words, links).
//!
//! The parser is total: any input produces some output, nothing panics, and nesting is bounded.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod emoji;
mod inline;

use serde::Serialize;
use unicode_segmentation::UnicodeSegmentation;

pub use inline::parse_inline;

/// Character styling of an inline run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
}

/// An inline piece of a line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Inline {
    Text {
        text: String,
        style: Style,
    },
    Link {
        url: String,
        label: String,
        style: Style,
    },
    /// `@handle` (lowercase, without the `@`).
    Mention {
        handle: String,
    },
    /// `@here`, `@channel` or `@everyone` (without the `@`).
    Broadcast {
        name: String,
    },
    /// `#channel` (without the `#`).
    ChannelRef {
        name: String,
    },
    /// `:shortcode:`; `glyph` is the Unicode emoji when the shortcode is known.
    Emoji {
        name: String,
        glyph: Option<&'static str>,
    },
}

/// A line-level block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    Paragraph { inlines: Vec<Inline> },
    Quote { inlines: Vec<Inline> },
    Bullet { inlines: Vec<Inline> },
    Numbered { number: u32, inlines: Vec<Inline> },
    Code { text: String },
}

/// Parses a message body into blocks.
pub fn parse(src: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut lines = src.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            // Fenced code: everything up to the closing fence (or the end) is literal.
            let mut code = Vec::new();
            if let Some(inline_end) = rest.strip_suffix("```").filter(|_| rest.len() >= 3) {
                out.push(Block::Code { text: inline_end.to_string() });
                continue;
            }
            if !rest.trim().is_empty() {
                code.push(rest.to_string());
            }
            for l in lines.by_ref() {
                if let Some(before) = l.trim_end().strip_suffix("```") {
                    if !before.trim().is_empty() {
                        code.push(before.to_string());
                    }
                    break;
                }
                code.push(l.to_string());
            }
            out.push(Block::Code { text: code.join("\n") });
        } else if let Some(rest) = trimmed.strip_prefix("&gt; ").or_else(|| trimmed.strip_prefix("> ")).or_else(|| (trimmed == ">").then_some("")) {
            out.push(Block::Quote { inlines: parse_inline(rest) });
        } else if let Some(rest) = ["- ", "* ", "• "].iter().find_map(|p| trimmed.strip_prefix(p)) {
            out.push(Block::Bullet { inlines: parse_inline(rest) });
        } else if let Some((n, rest)) = numbered(trimmed) {
            out.push(Block::Numbered { number: n, inlines: parse_inline(rest) });
        } else {
            out.push(Block::Paragraph { inlines: parse_inline(line) });
        }
    }
    out
}

fn numbered(line: &str) -> Option<(u32, &str)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 6 {
        return None;
    }
    let rest = line.get(digits..)?.strip_prefix(". ")?;
    Some((line.get(..digits)?.parse().ok()?, rest))
}

fn for_each_inline(src: &str, mut f: impl FnMut(&Inline)) {
    for b in parse(src) {
        match &b {
            Block::Paragraph { inlines } | Block::Quote { inlines } | Block::Bullet { inlines } | Block::Numbered { inlines, .. } => {
                inlines.iter().for_each(&mut f);
            }
            Block::Code { .. } => {}
        }
    }
}

/// Handles mentioned with `@handle` (lowercase, deduplicated, in order).
pub fn mentions(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for_each_inline(src, |i| {
        if let Inline::Mention { handle } = i
            && !out.contains(handle)
        {
            out.push(handle.clone());
        }
    });
    out
}

/// `@here`, `@channel` or `@everyone` appears outside code.
pub fn broadcasts(src: &str) -> bool {
    let mut any = false;
    for_each_inline(src, |i| any |= matches!(i, Inline::Broadcast { .. }));
    any
}

/// Link URLs (bare or `<url|label>`).
pub fn links(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for_each_inline(src, |i| {
        if let Inline::Link { url, .. } = i {
            out.push(url.clone());
        }
    });
    out
}

/// The message as plain text: markup removed, emoji shown as glyphs, one line per block.
pub fn plain(src: &str) -> String {
    let mut lines = Vec::new();
    for b in parse(src) {
        let line = match &b {
            Block::Paragraph { inlines } | Block::Quote { inlines } | Block::Bullet { inlines } | Block::Numbered { inlines, .. } => {
                inlines.iter().map(inline_plain).collect::<String>()
            }
            Block::Code { text } => text.clone(),
        };
        lines.push(line);
    }
    lines.join("\n")
}

fn inline_plain(i: &Inline) -> String {
    match i {
        Inline::Text { text, .. } => text.clone(),
        Inline::Link { label, .. } => label.clone(),
        Inline::Mention { handle } => format!("@{handle}"),
        Inline::Broadcast { name } => format!("@{name}"),
        Inline::ChannelRef { name } => format!("#{name}"),
        Inline::Emoji { name, glyph } => glyph.map(str::to_string).unwrap_or_else(|| format!(":{name}:")),
    }
}

/// Lowercase words for search (Unicode word boundaries; punctuation dropped).
pub fn words(src: &str) -> Vec<String> {
    src.unicode_words().map(str::to_lowercase).collect()
}

/// A one-line preview of at most `max` characters, with an ellipsis when cut.
pub fn snippet(src: &str, max: usize) -> String {
    let flat = plain(src).split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut s: String = flat.chars().take(max.saturating_sub(1)).collect();
    s.push('…');
    s
}

#[cfg(test)]
mod tests;
