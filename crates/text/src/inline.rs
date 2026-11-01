//! Inline markup: styles, links, mentions, channel refs and emoji.

use crate::{Inline, Style, emoji};

/// Nesting depth cap (bold inside italic inside strike…). Deeper markers stay literal.
const MAX_DEPTH: usize = 4;

/// Parses one line of inline markup.
pub fn parse_inline(src: &str) -> Vec<Inline> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    run(&chars, Style::default(), 0, &mut out);
    merge_text(out)
}

fn is_boundary(c: Option<&char>) -> bool {
    c.is_none_or(|c| c.is_whitespace() || "([{\"'.,;:!?-—*_~".contains(*c))
}

/// Finds the closing `marker` (1–2 chars) for an opener ending at `from`.
fn find_close(chars: &[char], from: usize, marker: &[char]) -> Option<usize> {
    let n = marker.len();
    let mut i = from;
    while i + n <= chars.len() {
        if chars.get(i..i + n) == Some(marker) && i > from {
            let before = chars.get(i - 1);
            let after = chars.get(i + n);
            let closes = before.is_some_and(|c| !c.is_whitespace()) && is_boundary(after) && after != marker.first();
            if closes {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn push_text(out: &mut Vec<Inline>, s: String, style: Style) {
    if !s.is_empty() {
        out.push(Inline::Text { text: s, style });
    }
}

fn styled(style: Style, c: char) -> Style {
    let mut s = style;
    match c {
        '*' => s.bold = true,
        '_' => s.italic = true,
        '~' => s.strike = true,
        _ => {}
    }
    s
}

fn run(chars: &[char], style: Style, depth: usize, out: &mut Vec<Inline>) {
    let mut buf = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let prev = if i == 0 { None } else { chars.get(i - 1) };
        let at_start = i == 0 || is_boundary(prev);
        // Inline code: literal up to the next backtick.
        if c == '`'
            && let Some(len) = chars.get(i + 1..).and_then(|r| r.iter().position(|&x| x == '`'))
            && len > 0
        {
            push_text(out, std::mem::take(&mut buf), style);
            let code: String = chars.get(i + 1..i + 1 + len).map(|s| s.iter().collect()).unwrap_or_default();
            out.push(Inline::Text { text: code, style: Style { code: true, ..style } });
            i += len + 2;
            continue;
        }
        // Styles: `*x*`, `**x**`, `_x_`, `~x~`, `~~x~~`.
        if matches!(c, '*' | '_' | '~') && at_start && depth < MAX_DEPTH {
            let double = chars.get(i + 1) == Some(&c) && c != '_';
            let (single, pair) = ([c], [c, c]);
            let marker: &[char] = if double { &pair } else { &single };
            let open_end = i + marker.len();
            if chars.get(open_end).is_some_and(|n| !n.is_whitespace())
                && let Some(close) = find_close(chars, open_end, marker)
            {
                push_text(out, std::mem::take(&mut buf), style);
                if let Some(inner) = chars.get(open_end..close) {
                    run(inner, styled(style, c), depth + 1, out);
                }
                i = close + marker.len();
                continue;
            }
        }
        // `<url|label>` / `<url>` / `<@handle>` / `<#channel>`.
        if c == '<'
            && let Some(len) = chars.get(i + 1..).and_then(|r| r.iter().take(2048).position(|&x| x == '>'))
        {
            let inner: String = chars.get(i + 1..i + 1 + len).map(|s| s.iter().collect()).unwrap_or_default();
            if let Some(item) = angle(&inner, style) {
                push_text(out, std::mem::take(&mut buf), style);
                out.push(item);
                i += len + 2;
                continue;
            }
        }
        // Bare links.
        if at_start && (c == 'h' || c == 'H') {
            let rest: String = chars.get(i..).map(|s| s.iter().take(8).collect::<String>()).unwrap_or_default().to_ascii_lowercase();
            if rest.starts_with("https://") || rest.starts_with("http://") {
                let len = chars.get(i..).map(|r| r.iter().take_while(|c| !c.is_whitespace() && **c != '>').count()).unwrap_or(0);
                let mut url: String = chars.get(i..i + len).map(|s| s.iter().collect()).unwrap_or_default();
                // Trailing sentence punctuation is not part of the link.
                while url.ends_with(['.', ',', ')', '!', '?', ';', ':']) {
                    url.pop();
                }
                let used = url.chars().count();
                push_text(out, std::mem::take(&mut buf), style);
                out.push(Inline::Link { label: url.clone(), url, style });
                i += used;
                continue;
            }
        }
        // `@handle`, `#channel`.
        if (c == '@' || c == '#') && at_start {
            let name: String = chars
                .get(i + 1..)
                .map(|r| r.iter().take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.')).collect())
                .unwrap_or_default();
            let name = name.trim_end_matches('.').to_string();
            if !name.is_empty() {
                push_text(out, std::mem::take(&mut buf), style);
                let used = name.chars().count();
                let lower = name.to_lowercase();
                out.push(match (c, lower.as_str()) {
                    ('@', "here" | "channel" | "everyone") => Inline::Broadcast { name: lower },
                    ('@', _) => Inline::Mention { handle: lower },
                    _ => Inline::ChannelRef { name: lower },
                });
                i += used + 1;
                continue;
            }
        }
        // `:shortcode:`.
        if c == ':'
            && let Some(len) = chars.get(i + 1..).and_then(|r| r.iter().take(65).position(|&x| x == ':'))
            && len > 0
        {
            let name: String = chars.get(i + 1..i + 1 + len).map(|s| s.iter().collect()).unwrap_or_default();
            if emoji::valid_name(&name) {
                push_text(out, std::mem::take(&mut buf), style);
                let glyph = emoji::lookup(&name);
                out.push(Inline::Emoji { name, glyph });
                i += len + 2;
                continue;
            }
        }
        buf.push(c);
        i += 1;
    }
    push_text(out, buf, style);
}

fn angle(inner: &str, style: Style) -> Option<Inline> {
    if let Some(h) = inner.strip_prefix('@') {
        return (!h.is_empty() && !h.contains(char::is_whitespace)).then(|| Inline::Mention { handle: h.to_lowercase() });
    }
    if let Some(c) = inner.strip_prefix('#') {
        return (!c.is_empty() && !c.contains(char::is_whitespace)).then(|| Inline::ChannelRef { name: c.to_lowercase() });
    }
    let (url, label) = match inner.split_once('|') {
        Some((u, l)) => (u, l),
        None => (inner, inner),
    };
    let lower = url.to_ascii_lowercase();
    let ok = (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")) && !url.contains(char::is_whitespace);
    ok.then(|| Inline::Link { url: url.to_string(), label: if label.is_empty() { url.to_string() } else { label.to_string() }, style })
}

/// Joins neighbouring text runs with the same style.
fn merge_text(items: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(items.len());
    for it in items {
        if let (Some(Inline::Text { text: a, style: sa }), Inline::Text { text: b, style: sb }) = (out.last_mut(), &it)
            && sa == sb
        {
            a.push_str(b);
            continue;
        }
        out.push(it);
    }
    out
}
