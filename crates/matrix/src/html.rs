//! Matrix `formatted_body` (a small subset of HTML) → ChatCraft markup.
//!
//! Other Matrix clients send formatting as HTML (`org.matrix.custom.html`); the plain `body`
//! loses it. This converter understands the tags Matrix allows for messages — bold, italic,
//! strikethrough, inline and block code, links, mention pills, quotes, lists, line breaks — and
//! drops everything else (keeping its text). It never executes or renders HTML; unknown or
//! malformed markup degrades to text. Output is capped like any message.

use chatcraft_core::limits::MAX_MESSAGE_BYTES;

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let tail = rest.get(i..).unwrap_or_default();
        let end = tail.find(';').filter(|e| *e <= 10);
        let (rep, used) = match end.map(|e| (tail.get(1..e).unwrap_or_default(), e + 1)) {
            Some(("amp", n)) => ("&".to_string(), n),
            Some(("lt", n)) => ("<".to_string(), n),
            Some(("gt", n)) => (">".to_string(), n),
            Some(("quot", n)) => ("\"".to_string(), n),
            Some(("apos" | "#39", n)) => ("'".to_string(), n),
            Some(("nbsp", n)) => (" ".to_string(), n),
            Some((num, n)) if num.starts_with('#') => {
                let code = num
                    .strip_prefix("#x")
                    .or_else(|| num.strip_prefix("#X"))
                    .map_or_else(|| num.get(1..).and_then(|d| d.parse().ok()), |h| u32::from_str_radix(h, 16).ok());
                match code.and_then(char::from_u32) {
                    Some(c) => (c.to_string(), n),
                    None => ("&".to_string(), 1),
                }
            }
            _ => ("&".to_string(), 1),
        };
        out.push_str(&rep);
        rest = tail.get(used..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// The value of `attr` in a tag's attribute text.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let i = lower.find(&format!("{name}="))?;
    let rest = tag.get(i + name.len() + 1..)?;
    let (q, body) = match rest.chars().next()? {
        c @ ('"' | '\'') => (c, rest.get(1..)?),
        _ => (' ', rest),
    };
    let end = body.find(|c: char| c == q || (q == ' ' && c == '>')).unwrap_or(body.len());
    Some(decode_entities(body.get(..end)?))
}

/// `https://matrix.to/#/@alice:example.org` → `@alice:example.org`.
fn pill(href: &str) -> Option<String> {
    let id = href.strip_prefix("https://matrix.to/#/")?;
    let id = id.split('?').next().unwrap_or(id);
    let id = id.replace("%40", "@").replace("%3A", ":").replace("%3a", ":");
    id.starts_with('@').then_some(id)
}

/// Converts a `formatted_body`. `localpart` turns a mention pill's Matrix ID into the handle
/// ChatCraft shows (`@alice`).
pub fn to_markup(html: &str, localpart: &dyn Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut link: Option<(String, String)> = None; // (href, collected label) while inside <a>
    let mut lists: Vec<Option<u32>> = Vec::new(); // None = bulleted, Some(n) = numbered
    let mut pre = false;
    let mut quote = 0usize;
    let mut skip = 0usize; // inside <mx-reply> (the quoted fallback of a reply)
    let push = |out: &mut String, link: &mut Option<(String, String)>, s: &str| match link {
        Some((_, label)) => label.push_str(s),
        None => out.push_str(s),
    };
    let newline = |out: &mut String, quote: usize| {
        // A line holding only a quote prefix is replaced, not left empty.
        let start = out.rfind('\n').map_or(0, |i| i + 1);
        if start < out.len() && out.get(start..).is_some_and(|l| l.chars().all(|c| c == '>' || c == ' ')) {
            out.truncate(start);
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        for _ in 0..quote {
            out.push_str("> ");
        }
    };
    while !rest.is_empty() && out.len() < MAX_MESSAGE_BYTES {
        let Some(lt) = rest.find('<') else {
            if skip == 0 {
                let text = decode_entities(rest);
                push(&mut out, &mut link, &text);
            }
            break;
        };
        if skip == 0 && lt > 0 {
            let text = decode_entities(rest.get(..lt).unwrap_or_default());
            let text = if pre { text } else { text.replace('\n', " ") };
            push(&mut out, &mut link, &text);
        }
        let after = rest.get(lt + 1..).unwrap_or_default();
        let Some(gt) = after.find('>') else {
            if skip == 0 {
                push(&mut out, &mut link, &decode_entities(rest.get(lt..).unwrap_or_default()));
            }
            break;
        };
        let tag = after.get(..gt).unwrap_or_default().trim();
        rest = after.get(gt + 1..).unwrap_or_default();
        let closing = tag.starts_with('/');
        let name: String =
            tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
        if name == "mx-reply" {
            skip = if closing { skip.saturating_sub(1) } else { skip + 1 };
            continue;
        }
        if skip > 0 {
            continue;
        }
        match (name.as_str(), closing) {
            ("b" | "strong", _) => push(&mut out, &mut link, "*"),
            ("i" | "em", _) => push(&mut out, &mut link, "_"),
            ("del" | "s" | "strike", _) => push(&mut out, &mut link, "~"),
            ("code", _) if !pre => push(&mut out, &mut link, "`"),
            ("pre", false) => {
                newline(&mut out, quote);
                out.push_str("```\n");
                pre = true;
            }
            ("pre", true) => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("```\n");
                pre = false;
            }
            ("br", _) => newline(&mut out, quote),
            ("p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => newline(&mut out, quote),
            ("blockquote", false) => {
                quote += 1;
                newline(&mut out, quote);
            }
            ("blockquote", true) => {
                quote = quote.saturating_sub(1);
                newline(&mut out, quote);
            }
            ("ul", false) => lists.push(None),
            ("ol", false) => lists.push(Some(attr(tag, "start").and_then(|s| s.parse().ok()).unwrap_or(1))),
            ("ul" | "ol", true) => {
                lists.pop();
                newline(&mut out, quote);
            }
            ("li", false) => {
                newline(&mut out, quote);
                match lists.last_mut() {
                    Some(Some(n)) => {
                        out.push_str(&format!("{n}. "));
                        *n = n.saturating_add(1);
                    }
                    _ => out.push_str("- "),
                }
            }
            ("a", false) => link = Some((attr(tag, "href").unwrap_or_default(), String::new())),
            ("a", true) => {
                if let Some((href, label)) = link.take() {
                    let piece = match pill(&href) {
                        Some(id) => localpart(&id),
                        None if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("mailto:") => {
                            if label.trim().is_empty() || label == href { href } else { format!("<{href}|{}>", label.replace(['|', '>'], " ")) }
                        }
                        None => label,
                    };
                    out.push_str(&piece);
                }
            }
            _ => {}
        }
    }
    let mut s = out.trim_matches('\n').to_string();
    if s.len() > MAX_MESSAGE_BYTES {
        let cut = (0..=MAX_MESSAGE_BYTES).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
        s.truncate(cut);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(html: &str) -> String {
        to_markup(html, &|id: &str| format!("@{}", id.trim_start_matches('@').split(':').next().unwrap_or("")))
    }

    #[test]
    fn inline_styles_links_and_pills() {
        assert_eq!(conv("Hi <strong>there</strong>, <em>really</em> <del>not</del> <code>x &lt; y</code>"), "Hi *there*, _really_ ~not~ `x < y`");
        assert_eq!(conv(r#"see <a href="https://example.org/a?b=1&amp;c=2">the docs</a>"#), "see <https://example.org/a?b=1&c=2|the docs>");
        assert_eq!(conv(r#"<a href="https://matrix.to/#/@ada:example.org">Ada Park</a>: ping"#), "@ada: ping");
        assert_eq!(conv(r#"<a href="https://example.org">https://example.org</a>"#), "https://example.org");
        assert_eq!(conv("caf&#233; &#x1F680; &amp;amp"), "café 🚀 &amp");
    }

    #[test]
    fn blocks() {
        assert_eq!(conv("<p>one</p><p>two<br>three</p>"), "one\ntwo\nthree");
        assert_eq!(conv("<ul><li>a</li><li><b>b</b></li></ul>"), "- a\n- *b*");
        assert_eq!(conv(r#"<ol start="3"><li>c</li><li>d</li></ol>"#), "3. c\n4. d");
        assert_eq!(conv("<blockquote><p>quoted</p></blockquote><p>reply</p>"), "> quoted\nreply");
        assert_eq!(conv("<pre><code>let x = 1;\nlet y = *x*;</code></pre>"), "```\nlet x = 1;\nlet y = *x*;\n```");
        // A reply's quoted fallback is dropped (the thread relation carries it).
        assert_eq!(conv("<mx-reply><blockquote>old</blockquote></mx-reply>new"), "new");
    }

    #[test]
    fn hostile_html_never_panics() {
        for h in [
            "<",
            "<<<>>>",
            "<a href=",
            "<a href=\"",
            "&",
            "&#;",
            "&#xZZZZ;",
            "&#99999999;",
            "<b",
            "</a>",
            "<ol start=x><li>",
            "<pre>",
            "\\u{0}<script>alert(1)</script>",
        ] {
            let _ = conv(h);
        }
        let deep = "<blockquote>".repeat(10_000);
        assert!(conv(&deep).len() <= MAX_MESSAGE_BYTES);
        assert_eq!(conv("<script>alert(1)</script>"), "alert(1)", "tags are dropped, never run");
    }
}

/// Plain text as HTML (to tell whether a message has any formatting at all).
pub fn escape_text(s: &str) -> String {
    escape(s).replace('\n', "<br>")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn inline_html(items: &[chatcraft_text::Inline], mxid_of: &dyn Fn(&str) -> Option<String>) -> String {
    use chatcraft_text::Inline;
    let styled = |text: String, st: &chatcraft_text::Style| {
        let mut t = text;
        if st.code {
            t = format!("<code>{t}</code>");
        }
        if st.strike {
            t = format!("<del>{t}</del>");
        }
        if st.italic {
            t = format!("<em>{t}</em>");
        }
        if st.bold {
            t = format!("<strong>{t}</strong>");
        }
        t
    };
    items
        .iter()
        .map(|i| match i {
            Inline::Text { text, style } => styled(escape(text), style),
            Inline::Link { url, label, style } => styled(format!("<a href=\"{}\">{}</a>", escape(url), escape(label)), style),
            Inline::Mention { handle } => match mxid_of(handle) {
                Some(id) => format!("<a href=\"https://matrix.to/#/{}\">@{}</a>", escape(&id), escape(handle)),
                None => format!("@{}", escape(handle)),
            },
            Inline::Broadcast { name } => format!("@{}", escape(name)),
            Inline::ChannelRef { name } => format!("#{}", escape(name)),
            Inline::Emoji { name, glyph } => glyph.map_or_else(|| format!(":{}:", escape(name)), str::to_string),
        })
        .collect()
}

/// ChatCraft markup → Matrix HTML, so other clients see what ChatCraft people meant (`*x*` is
/// bold here, as in Slack, but italic in CommonMark). `mxid_of` turns `@handle` into a pill.
pub fn from_markup(src: &str, mxid_of: &dyn Fn(&str) -> Option<String>) -> String {
    use chatcraft_text::Block;
    let mut out = String::new();
    let mut list: Option<bool> = None; // Some(true) = numbered
    let close_list = |out: &mut String, list: &mut Option<bool>| {
        if let Some(n) = list.take() {
            out.push_str(if n { "</ol>" } else { "</ul>" });
        }
    };
    for (i, b) in chatcraft_text::parse(src).iter().enumerate() {
        match b {
            Block::Bullet { inlines } | Block::Numbered { inlines, .. } => {
                let numbered = matches!(b, Block::Numbered { .. });
                if list != Some(numbered) {
                    close_list(&mut out, &mut list);
                    out.push_str(if numbered { "<ol>" } else { "<ul>" });
                    list = Some(numbered);
                }
                out.push_str(&format!("<li>{}</li>", inline_html(inlines, mxid_of)));
                continue;
            }
            _ => close_list(&mut out, &mut list),
        }
        if i > 0 && !out.is_empty() && !out.ends_with('>') {
            out.push_str("<br>");
        }
        match b {
            Block::Paragraph { inlines } => {
                if i > 0
                    && !out.is_empty()
                    && out.ends_with('>')
                    && !out.ends_with("</ul>")
                    && !out.ends_with("</ol>")
                    && !out.ends_with("</pre>")
                    && !out.ends_with("</blockquote>")
                {
                    out.push_str("<br>");
                }
                out.push_str(&inline_html(inlines, mxid_of));
            }
            Block::Quote { inlines } => out.push_str(&format!("<blockquote>{}</blockquote>", inline_html(inlines, mxid_of))),
            Block::Code { text } => out.push_str(&format!("<pre><code>{}</code></pre>", escape(text))),
            Block::Bullet { .. } | Block::Numbered { .. } => {}
        }
    }
    close_list(&mut out, &mut list);
    out
}

#[cfg(test)]
mod from_markup_tests {
    use super::*;

    fn html(src: &str) -> String {
        from_markup(src, &|h: &str| (h == "ada").then(|| "@ada:example.org".to_string()))
    }

    #[test]
    fn markup_to_html_and_back() {
        assert_eq!(html("*bold* _it_ ~gone~ `x<y`"), "<strong>bold</strong> <em>it</em> <del>gone</del> <code>x&lt;y</code>");
        assert_eq!(html("hi @ada and @nobody"), r#"hi <a href="https://matrix.to/#/@ada:example.org">@ada</a> and @nobody"#);
        assert_eq!(html("intro\n- one\n- two\nafter"), "intro<ul><li>one</li><li>two</li></ul>after");
        assert_eq!(html("```\nlet x = 1;\n```"), "<pre><code>let x = 1;</code></pre>");
        // Round trip through the receiving converter keeps the meaning.
        let back = to_markup(&html("*Ship* it @ada, see <https://example.org|docs>"), &|id: &str| {
            format!("@{}", id.trim_start_matches('@').split(':').next().unwrap_or(""))
        });
        assert_eq!(back, "*Ship* it @ada, see <https://example.org|docs>");
        assert_eq!(html("<script>"), "&lt;script&gt;");
    }
}
