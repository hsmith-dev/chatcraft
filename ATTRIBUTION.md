# Asset attribution

Every non-code asset in this repository (fonts, icons, images, sounds, example files) is listed here with its author, source and licence. `cargo xtask assets` (part of `cargo xtask ci`) fails if an asset file is missing from this table.

**Policy (mandatory):** ChatCraft contains **no Slack, Salesforce, Microsoft or other vendor iconography, images, sounds, emoji artwork, themes, fonts or other assets**. Every asset is original work by ChatCraft contributors or third-party material under an open licence (OSI open source, public domain / CC0, or a Creative Commons licence that allows redistribution). Screenshots of other vendors' software are never committed.

Generated-in-code art is original and has no file to list: the icon set (`crates/ui-egui/src/icons.rs`), avatars (`crates/ui-egui/src/avatar.rs`), the colour themes (`crates/ui-egui/src/theme.rs`) and the demo workspace's people and conversations (`crates/hub/src/sample.rs`). Emoji are Unicode characters drawn by open fonts.

| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `egui` dependency: `epaint_default_fonts/fonts/Ubuntu-Light.ttf` | Dalton Maag for Canonical | https://crates.io/crates/epaint_default_fonts | Ubuntu Font Licence 1.0 | Embedded by egui's `default_fonts` feature; not a file in this repo |
| `egui` dependency: `epaint_default_fonts/fonts/Hack-Regular.ttf` | Source Foundry | https://crates.io/crates/epaint_default_fonts | MIT + Bitstream Vera | Monospace UI font |
| `egui` dependency: `epaint_default_fonts/fonts/NotoEmoji-Regular.ttf` | Google | https://crates.io/crates/epaint_default_fonts | SIL OFL 1.1 | Monochrome emoji (`monochrome_emoji_fonts`) |
| `egui` dependency: `epaint_default_fonts/fonts/emoji-icon-font.ttf` | John Slegers | https://github.com/jslegers/emoji-icon-font | MIT | |
| `egui` dependency: `epaint_default_fonts/fonts/egui-icons.ttf` | the egui authors | https://github.com/emilk/egui | MIT OR Apache-2.0 | |
| `docs/images/hero.png` | ChatCraft contributors | Rendered by `cargo run -p chatcraft-ui-egui --example shot` (ChatCraft's own UI and fictional demo workspace) | MIT OR Apache-2.0 | README screenshot |
| `docs/images/dark.png` | ChatCraft contributors | Rendered by the `shot` example with `--dark` | MIT OR Apache-2.0 | README screenshot |
