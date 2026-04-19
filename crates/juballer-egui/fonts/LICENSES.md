# Bundled font licenses

All bundled fonts in this directory are licensed under the SIL Open Font
License (OFL) 1.1, which permits redistribution (including bundled with
software). Full license texts are in this directory alongside each font.

| File                             | Upstream                                                                 | Version      | License                           |
|----------------------------------|--------------------------------------------------------------------------|--------------|-----------------------------------|
| `NotoEmoji-Variable.ttf`         | https://github.com/google/fonts/tree/main/ofl/notoemoji                  | variable wght | OFL 1.1 (`LICENSE-NotoEmoji.txt`) |
| `NotoSansSymbols2-Regular.ttf`   | https://github.com/notofonts/notofonts.github.io/tree/main/fonts/NotoSansSymbols2 | 2.x          | OFL 1.1 (`LICENSE-NotoSansSymbols2.txt`) |

## Why these specifically

egui's text rasterizer (powered by `ab_glyph`) can only render outline
glyphs. It does NOT support color-bitmap (`CBDT`) or `COLR` tables, which
means `NotoColorEmoji.ttf` renders as blank tofu. The two fonts bundled
here are outline-only:

- **Noto Emoji** (the variable, monochrome outline variant — distinct from
  `NotoColorEmoji`) covers the bulk of the Unicode emoji block, including
  things like U+1F9EA test tube, U+1F916 robot, U+1F4AC speech balloon,
  U+1F6D1 stop sign, U+1F9D1 person, U+1F4BB laptop, U+1F49C purple heart.
- **Noto Sans Symbols 2** covers symbol ranges outside the emoji block:
  arrows (U+2B06..U+2B07), geometric shapes, dingbats (U+2794), music,
  mathematical operators, etc. It is loaded BEFORE Noto Emoji so its
  cleaner, more uniform glyphs win for codepoints both fonts cover.

Bundling the fonts in-crate (via `include_bytes!`) makes rendering
deterministic: we no longer depend on what the user happens to have
installed at `/usr/share/fonts`, and the overlay renders the same on
Arch, Debian, WSL, and a fresh steamdeck.
