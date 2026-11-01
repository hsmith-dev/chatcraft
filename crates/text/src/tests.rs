use super::*;

fn t(text: &str) -> Inline {
    Inline::Text { text: text.into(), style: Style::default() }
}
fn b(text: &str) -> Inline {
    Inline::Text { text: text.into(), style: Style { bold: true, ..Style::default() } }
}

#[test]
fn bold_in_both_dialects() {
    assert_eq!(parse_inline("a *b* c"), vec![t("a "), b("b"), t(" c")]);
    assert_eq!(parse_inline("a **b** c"), vec![t("a "), b("b"), t(" c")]);
}

#[test]
fn unmatched_markers_stay_literal() {
    assert_eq!(parse_inline("2 * 3 = 6"), vec![t("2 * 3 = 6")]);
    assert_eq!(parse_inline("snake_case_name"), vec![t("snake_case_name")]);
    assert_eq!(parse_inline("*open"), vec![t("*open")]);
}

#[test]
fn nested_styles() {
    let got = parse_inline("_it *both*_");
    assert_eq!(
        got,
        vec![
            Inline::Text { text: "it ".into(), style: Style { italic: true, ..Style::default() } },
            Inline::Text { text: "both".into(), style: Style { italic: true, bold: true, ..Style::default() } },
        ]
    );
}

#[test]
fn code_is_literal() {
    assert_eq!(parse_inline("run `*x*` now")[1], Inline::Text { text: "*x*".into(), style: Style { code: true, ..Style::default() } });
    let blocks = parse("before\n```\nlet x = *y*;\n```\nafter");
    assert_eq!(blocks[1], Block::Code { text: "let x = *y*;".into() });
    assert_eq!(blocks.len(), 3);
    assert!(mentions("```@ada```").is_empty());
}

#[test]
fn links_mentions_channels_emoji() {
    let got = parse_inline("hi @Ada, see #launch-plan and https://example.org/x. :tada: <https://a.b|docs> @here");
    assert!(got.contains(&Inline::Mention { handle: "ada".into() }));
    assert!(got.contains(&Inline::ChannelRef { name: "launch-plan".into() }));
    assert!(got.contains(&Inline::Link { url: "https://example.org/x".into(), label: "https://example.org/x".into(), style: Style::default() }));
    assert!(got.contains(&Inline::Emoji { name: "tada".into(), glyph: Some("🎉") }));
    assert!(got.contains(&Inline::Link { url: "https://a.b".into(), label: "docs".into(), style: Style::default() }));
    assert!(got.contains(&Inline::Broadcast { name: "here".into() }));
    assert_eq!(mentions("@ada @bo @ada"), vec!["ada".to_string(), "bo".to_string()]);
    assert!(broadcasts("ping @channel"));
    assert!(!parse_inline("email me at a@b.com").iter().any(|i| matches!(i, Inline::Mention { .. })));
}

#[test]
fn blocks() {
    let got = parse("> quoted\n- one\n2. two\nplain");
    assert!(matches!(got[0], Block::Quote { .. }));
    assert!(matches!(got[1], Block::Bullet { .. }));
    assert!(matches!(got[2], Block::Numbered { number: 2, .. }));
    assert!(matches!(got[3], Block::Paragraph { .. }));
}

#[test]
fn plain_snippet_words() {
    assert_eq!(plain("*Ship* it :rocket: <https://x.y|now>"), "Ship it 🚀 now");
    assert_eq!(snippet("one two three four", 9), "one two …");
    assert_eq!(words("Hello, World! it's"), vec!["hello", "world", "it's"]);
}

#[test]
fn hostile_input_never_panics() {
    let nasty =
        ["", "*", "**", "***", "_*~`", "<", "<>", "<|>", ":", "::", "@", "#", "```", "```\n", "h", "http://", "a*b*c", "\u{0301}*x*", "🙂*🙂*"];
    for s in nasty {
        let _ = parse(s);
        let _ = plain(s);
        let _ = snippet(s, 1);
    }
    let deep = "*_~".repeat(200) + "x" + &"~_*".repeat(200);
    let _ = parse(&deep);
    let long = "<".repeat(10_000);
    let _ = parse(&long);
}
