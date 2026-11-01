//! Message search (L2).
//!
//! Queries read like the ones people already type: words, `"exact phrases"`, `-excluded`, and
//! modifiers `in:#channel`, `in:@person` (a direct message), `from:@person`, `to:@person`
//! (mentions; `me` is you), `has:link`, `has:reaction`, `has:pin`, `is:thread`, `is:saved`, `before:`/`after:`/
//! `on:`/`during:` with `YYYY-MM-DD` dates. Only conversations the viewer may see are searched.
//!
//! M0 scans messages directly (fast enough for tens of thousands); an inverted index is a
//! tracked gap (`docs/gaps.md`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use chatcraft_core::limits::MAX_SEARCH_RESULTS;
use chatcraft_core::{ChannelId, MessageId, Ts, UserId};
use chatcraft_model::{Message, State};
use serde::Serialize;

/// A parsed query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub words: Vec<String>,
    pub phrases: Vec<String>,
    pub exclude: Vec<String>,
    pub in_channels: Vec<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub has_link: bool,
    pub has_reaction: bool,
    pub has_pin: bool,
    pub is_thread: bool,
    pub is_saved: bool,
    pub before: Option<Ts>,
    pub after: Option<Ts>,
    /// Modifiers that were not understood (shown back to the user).
    pub unknown: Vec<String>,
}

const DAY_MS: i64 = 86_400_000;

/// Splits on whitespace, keeping `"quoted phrases"` together.
fn tokens(q: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.chars() {
        match c {
            '"' => {
                if !cur.is_empty() || quoted {
                    out.push((std::mem::take(&mut cur), quoted));
                }
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push((std::mem::take(&mut cur), false));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push((cur, quoted));
    }
    out
}

impl Query {
    pub fn parse(q: &str) -> Query {
        let mut out = Query::default();
        for (tok, quoted) in tokens(q) {
            if quoted {
                if !tok.trim().is_empty() {
                    out.phrases.push(tok.to_lowercase());
                }
                continue;
            }
            if let Some(rest) = tok.strip_prefix('-').filter(|r| !r.is_empty()) {
                out.exclude.extend(chatcraft_text::words(rest));
                continue;
            }
            let Some((key, val)) = tok.split_once(':').filter(|(k, v)| !k.is_empty() && !v.is_empty()) else {
                out.words.extend(chatcraft_text::words(&tok));
                continue;
            };
            let val = val.to_lowercase();
            match key.to_ascii_lowercase().as_str() {
                "in" => out.in_channels.push(val),
                "from" => out.from.push(val),
                "to" => out.to.push(val),
                "has" => match val.as_str() {
                    "link" | "links" => out.has_link = true,
                    "reaction" | "reactions" | "emoji" => out.has_reaction = true,
                    "pin" | "pins" | "pinned" => out.has_pin = true,
                    _ => out.unknown.push(tok),
                },
                "is" => match val.as_str() {
                    "thread" | "reply" => out.is_thread = true,
                    "saved" => out.is_saved = true,
                    _ => out.unknown.push(tok),
                },
                "before" => out.before = Ts::parse_date(&val).or_else(|| bad(&mut out.unknown, &tok)),
                "after" => out.after = Ts::parse_date(&val).map(|t| t.plus_millis(DAY_MS)).or_else(|| bad(&mut out.unknown, &tok)),
                "on" | "during" => match Ts::parse_date(&val) {
                    Some(t) => {
                        out.after = Some(t);
                        out.before = Some(t.plus_millis(DAY_MS));
                    }
                    None => out.unknown.push(tok),
                },
                // `https://…` and other colons in ordinary words.
                _ => out.words.extend(chatcraft_text::words(&tok)),
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
            && self.phrases.is_empty()
            && self.in_channels.is_empty()
            && self.from.is_empty()
            && self.to.is_empty()
            && !(self.has_link || self.has_reaction || self.has_pin || self.is_thread || self.is_saved)
            && self.before.is_none()
            && self.after.is_none()
    }
}

fn bad(unknown: &mut Vec<String>, tok: &str) -> Option<Ts> {
    unknown.push(tok.to_string());
    None
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    pub message: MessageId,
    pub channel: ChannelId,
    pub score: u32,
}

/// Handles or ids; `me` / `@me` is the viewer.
fn resolve_users(state: &State, names: &[String], viewer: UserId) -> Option<Vec<UserId>> {
    names.iter().map(|n| if n.trim_start_matches('@') == "me" { Some(viewer) } else { state.resolve_user(n) }).collect()
}

fn resolve_channels(state: &State, names: &[String], viewer: UserId) -> Option<Vec<ChannelId>> {
    names.iter().map(|n| state.resolve_channel(n, Some(viewer))).collect()
}

/// Runs a query for `viewer`. Newest first, best matches first within the limit.
pub fn search(state: &State, viewer: UserId, query: &Query, limit: usize) -> Vec<Hit> {
    if query.is_empty() {
        return Vec::new();
    }
    // Unknown names match nothing (rather than everything).
    let (Some(chans), Some(from), Some(to)) =
        (resolve_channels(state, &query.in_channels, viewer), resolve_users(state, &query.from, viewer), resolve_users(state, &query.to, viewer))
    else {
        return Vec::new();
    };
    let saved = state.saved.get(&viewer);
    let mut hits: Vec<Hit> = Vec::new();
    for m in state.messages.values().rev() {
        if m.deleted || !state.can_see(viewer, m.channel) {
            continue;
        }
        if (!chans.is_empty() && !chans.contains(&m.channel))
            || (!from.is_empty() && !from.contains(&m.author))
            || (!to.is_empty() && !to.iter().any(|u| m.mentions.contains(u)))
            || (query.has_reaction && m.reactions.is_empty())
            || (query.has_pin && !m.pinned)
            || (query.is_thread && m.thread.is_none() && m.reply_count == 0)
            || (query.is_saved && !saved.is_some_and(|s| s.contains(&m.id)))
            || query.before.is_some_and(|b| m.ts >= b)
            || query.after.is_some_and(|a| m.ts < a)
        {
            continue;
        }
        if query.has_link && chatcraft_text::links(&m.text).is_empty() {
            continue;
        }
        let Some(score) = score(m, query) else { continue };
        hits.push(Hit { message: m.id, channel: m.channel, score });
        if hits.len() >= limit.min(MAX_SEARCH_RESULTS) * 4 {
            break;
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(b.message.cmp(&a.message)));
    hits.truncate(limit.min(MAX_SEARCH_RESULTS));
    hits
}

fn score(m: &Message, q: &Query) -> Option<u32> {
    let plain = chatcraft_text::plain(&m.text).to_lowercase();
    let words = chatcraft_text::words(&plain);
    if q.exclude.iter().any(|x| words.contains(x)) {
        return None;
    }
    let mut score = 1u32;
    for w in &q.words {
        // Prefix match, so "deploy" finds "deployment".
        let exact = words.iter().filter(|x| *x == w).count();
        let prefix = words.iter().filter(|x| x.starts_with(w.as_str())).count();
        if prefix == 0 {
            return None;
        }
        score = score.saturating_add(u32::try_from(exact * 3 + prefix).unwrap_or(u32::MAX));
    }
    for p in &q.phrases {
        if !plain.contains(p.as_str()) {
            return None;
        }
        score = score.saturating_add(5);
    }
    Some(score)
}

#[cfg(test)]
mod tests;
