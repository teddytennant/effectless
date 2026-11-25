//! Syntax highlighting for code surfaces (diff panel, file viewer, chat code blocks):
//! gpui-component's tree-sitter highlighter with the colors T3 uses (spec 05 §2.6).
//!
//! T3 highlights with Pierre's `highlights` lexers and the Zed-style `pierre-dark`
//! palette in dark mode, and `t3-light` in light mode: `pierre-light` with every
//! syntax color darkened until it reads on the darkest light code surface
//! (`lib/readableSyntaxTheme.ts`). The palettes below are Pierre's Zed syntax maps
//! (`@pierre/highlights` 0.5.1 `themes/pierre-{dark,light}.js`); their keys are
//! tree-sitter capture names, so they plug straight into the highlighter.
//!
//! ```ignore
//! let lang = syntax::language_for_path("src/main.rs");      // "rust"
//! let spans = syntax::highlight(code, lang, cx.tern_theme().is_dark());
//! let lines = syntax::highlight_lines(code, lang, dark);   // per-line (range, style)
//! ```
//!
//! Highlighting is synchronous and can take a few ms per thousand lines: run it in
//! `cx.background_spawn` for anything large. Lines longer than 1000 bytes are left
//! plain, as T3 does.

use std::ops::Range;
use std::sync::OnceLock;

use gpui::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba};
use gpui_base::input::HighlightStyleResolver;
use gpui_component::Rope;
use gpui_component::highlighter::SyntaxHighlighter;

/// Lines longer than this are not tokenized.
pub const MAX_HIGHLIGHT_LINE_BYTES: usize = 1000;

#[derive(Clone, Copy)]
struct Entry {
    name: &'static str,
    dark: u32,
    light: u32,
    bold: bool,
    italic: bool,
}

const fn e(name: &'static str, dark: u32, light: u32) -> Entry {
    Entry {
        name,
        dark,
        light,
        bold: false,
        italic: false,
    }
}

const fn bold(mut entry: Entry) -> Entry {
    entry.bold = true;
    entry
}

const fn italic(mut entry: Entry) -> Entry {
    entry.italic = true;
    entry
}

/// `pierre-dark` / `pierre-light` syntax colors (0xRRGGBB). The light values are
/// raw `pierre-light`; [`readable_on_light_code`] darkens them at lookup.
const PALETTE: &[Entry] = &[
    e("comment", 0x737373, 0x737373),
    e("comment.doc", 0x737373, 0x737373),
    e("string", 0x5ecc71, 0x199f43),
    e("string.escape", 0x61d5c0, 0x16a994),
    e("string.regex", 0x64d1db, 0x17a5af),
    e("string.special", 0x61d5c0, 0x16a994),
    e("string.special.symbol", 0xffd452, 0xd5a910),
    e("number", 0x68cdf2, 0x1ca1c7),
    e("constant", 0xffd452, 0xd5a910),
    e("boolean", 0x68cdf2, 0x1ca1c7),
    e("keyword", 0xff678d, 0xd32a61),
    e("keyword.operator", 0x08c0ef, 0x08c0ef),
    e("function", 0x9d6afb, 0x693acf),
    e("function.method", 0x9d6afb, 0x693acf),
    e("function.builtin", 0x9d6afb, 0x693acf),
    e("function.special.definition", 0x9d6afb, 0x693acf),
    e("function.call", 0x9d6afb, 0x693acf),
    e("type", 0xd568ea, 0xa631be),
    e("type.builtin", 0xd568ea, 0xa631be),
    e("constructor", 0xd568ea, 0xa631be),
    e("variable", 0xffa359, 0xd47628),
    e("variable.builtin", 0xffab16, 0xd5901c),
    e("variable.member", 0xffa359, 0xd47628),
    e("variable.parameter", 0xa3a3a3, 0x636363),
    e("variable.special", 0xffab16, 0xd5901c),
    e("variable.language", 0xffab16, 0xd5901c),
    e("property", 0xffa359, 0xd47628),
    e("property.css", 0x009fff, 0x009fff),
    e("property.definition", 0x009fff, 0x009fff),
    e("tag.css", 0xff855e, 0xd5512f),
    e("class", 0x60d199, 0x18a46c),
    e("selector.class", 0x60d199, 0x18a46c),
    e("selector.id", 0x9d6afb, 0x693acf),
    e("selector.pseudo", 0x08c0ef, 0x08c0ef),
    e("keyword.directive", 0xff678d, 0xd32a61),
    e("unit", 0x68cdf2, 0x1ca1c7),
    e("color", 0xffd452, 0xd5a910),
    e("this", 0xffab16, 0xd5901c),
    e("self", 0xffab16, 0xd5901c),
    e("type.class", 0xd568ea, 0xa631be),
    e("string.template", 0x5ecc71, 0x199f43),
    e("tag.jsx", 0xff855e, 0xd5512f),
    e("tag.component", 0xd568ea, 0xa631be),
    e("punctuation", 0x636363, 0x636363),
    e("punctuation.bracket", 0x636363, 0x636363),
    e("punctuation.delimiter", 0x636363, 0x636363),
    e("punctuation.list_marker", 0x636363, 0x636363),
    e("punctuation.special", 0xff678d, 0xd32a61),
    e("operator", 0x08c0ef, 0x08c0ef),
    e("tag", 0xff855e, 0xd5512f),
    e("attribute", 0x60d199, 0x18a46c),
    e("label", 0xffab16, 0xd5901c),
    e("namespace", 0xffab16, 0xd5901c),
    e("module", 0xffab16, 0xd5901c),
    e("decorator", 0x69b1ff, 0x1a85d4),
    e("attribute.builtin", 0x69b1ff, 0x1a85d4),
    e("embedded", 0xfafafa, 0x0a0a0a),
    e("preproc", 0xff678d, 0xd32a61),
    e("text.literal", 0x5ecc71, 0x199f43),
    e("text.code.span", 0x5ecc71, 0x199f43),
    bold(e("markup.heading", 0xff855e, 0xd5512f)),
    bold(e("markup.bold", 0xffd452, 0xd5a910)),
    italic(e("markup.italic", 0xff678d, 0xd32a61)),
    e("markup.strikethrough", 0x737373, 0x737373),
    e("markup.link.url", 0x009fff, 0x009fff),
    e("markup.link.text", 0x9d6afb, 0x693acf),
    italic(e("markup.quote", 0x737373, 0x737373)),
    e("markup.list", 0xff855e, 0xd5512f),
    e("markup.raw", 0x5ecc71, 0x199f43),
    e("diff.plus", 0x07c480, 0x18a46c),
    e("diff.minus", 0xff2e3f, 0xd52c36),
    e("diff.delta", 0xffca00, 0xd5a910),
    e("link_text", 0x009fff, 0x009fff),
    e("link_uri", 0xff678d, 0xd32a61),
    e("primary", 0x009fff, 0x009fff),
    bold(e("title", 0xff855e, 0xd5512f)),
    italic(e("predictive", 0x636363, 0x8a8a8a)),
    e("selector", 0xff855e, 0xd5512f),
    e("enum", 0xd568ea, 0xa631be),
    e("hint", 0x737373, 0x737373),
    e("variant", 0xffd452, 0xd5a910),
];

/// The darkest light code surface (a changed word on a deleted line), rounded down.
const DARKEST_SURFACE_LUMINANCE: f32 = 0.69;
const MIN_CONTRAST: f32 = 4.5;
/// Pairs that differ only in lightness go further so they keep their order.
const FURTHER_COLORS: [u32; 3] = [0x1ca1c7, 0x636363, 0x1a85d4];
const FURTHER_MIN_CONTRAST: f32 = 6.;

fn channel_luminance(channel: u8) -> f32 {
    let v = channel as f32 / 255.;
    if v <= 0.03928 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn contrast_on_light_code(rgb: [u8; 3]) -> f32 {
    let l = 0.2126 * channel_luminance(rgb[0])
        + 0.7152 * channel_luminance(rgb[1])
        + 0.0722 * channel_luminance(rgb[2]);
    (DARKEST_SURFACE_LUMINANCE + 0.05) / (l + 0.05)
}

/// `t3-light`: darkens a pierre-light color in 4% steps (keeping its hue) until it
/// reaches the contrast floor on light code (`readableSyntaxColor`).
pub fn readable_on_light_code(rgb: u32) -> u32 {
    let min = if FURTHER_COLORS.contains(&rgb) {
        FURTHER_MIN_CONTRAST
    } else {
        MIN_CONTRAST
    };
    let mut c = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    while contrast_on_light_code(c) < min {
        c = c.map(|ch| (ch as f32 * 0.96).floor() as u8);
    }
    (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
}

fn hsla(rgb: u32) -> Hsla {
    Rgba {
        r: ((rgb >> 16) & 0xff) as f32 / 255.,
        g: ((rgb >> 8) & 0xff) as f32 / 255.,
        b: (rgb & 0xff) as f32 / 255.,
        a: 1.,
    }
    .into()
}

struct Resolved {
    name: &'static str,
    style: HighlightStyle,
}

fn table(dark: bool) -> &'static [Resolved] {
    static DARK: OnceLock<Vec<Resolved>> = OnceLock::new();
    static LIGHT: OnceLock<Vec<Resolved>> = OnceLock::new();
    let cell = if dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| {
        PALETTE
            .iter()
            .map(|entry| {
                let rgb = if dark {
                    entry.dark
                } else {
                    readable_on_light_code(entry.light)
                };
                Resolved {
                    name: entry.name,
                    style: HighlightStyle {
                        color: Some(hsla(rgb)),
                        font_weight: entry.bold.then_some(FontWeight::BOLD),
                        font_style: entry.italic.then_some(FontStyle::Italic),
                        ..Default::default()
                    },
                }
            })
            .collect()
    })
}

/// The style for a tree-sitter capture name, falling back from `a.b.c` to `a.b` to
/// `a`, as Zed themes do.
pub fn syntax_style(name: &str, dark: bool) -> Option<HighlightStyle> {
    let table = table(dark);
    let mut name = name;
    loop {
        if let Some(found) = table.iter().find(|r| r.name == name) {
            return Some(found.style);
        }
        name = &name[..name.rfind('.')?];
    }
}

/// The Pierre palette as a gpui-component highlight resolver.
#[derive(Clone, Copy, Debug)]
pub struct PierreSyntax {
    pub dark: bool,
}

impl HighlightStyleResolver for PierreSyntax {
    fn style(&self, name: &str) -> Option<HighlightStyle> {
        syntax_style(name, self.dark)
    }
}

/// gpui-component language name for a file path, or "text".
pub fn language_for_path(path: &str) -> &'static str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "makefile" | "gnumakefile" => return "make",
        "cmakelists.txt" => return "cmake",
        "dockerfile" => return "bash",
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" => return "bash",
        _ => {}
    }
    let ext = match lower.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => return "text",
    };
    language_for_tag(ext)
}

/// gpui-component language name for a fence tag or extension (`rs`, `py`, `tsx`, …).
pub fn language_for_tag(tag: &str) -> &'static str {
    match tag.trim().to_ascii_lowercase().as_str() {
        "rs" | "rust" => "rust",
        "py" | "pyi" | "python" => "python",
        "js" | "mjs" | "cjs" | "jsx" | "javascript" => "javascript",
        "ts" | "mts" | "cts" | "typescript" => "typescript",
        "tsx" => "tsx",
        "json" | "jsonc" | "json5" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" | "mdx" => "markdown",
        "sh" | "bash" | "zsh" | "shell" | "fish" | "console" => "bash",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "c++" => "cpp",
        "cs" | "csharp" => "csharp",
        "css" | "scss" => "css",
        "html" | "htm" => "html",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" | "kotlin" => "kotlin",
        "lua" | "luau" => "lua",
        "rb" | "ruby" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "scala" => "scala",
        "sql" => "sql",
        "zig" => "zig",
        "ex" | "exs" | "elixir" => "elixir",
        "graphql" | "gql" => "graphql",
        "proto" => "proto",
        "svelte" => "svelte",
        "astro" => "astro",
        "diff" | "patch" => "diff",
        "mk" | "make" => "make",
        "cmake" => "cmake",
        _ => "text",
    }
}

/// Highlights `text`. Returns sorted, non-overlapping byte ranges with their styles;
/// unstyled gaps are left out.
pub fn highlight(text: &str, language: &str, dark: bool) -> Vec<(Range<usize>, HighlightStyle)> {
    if language == "text" || text.is_empty() {
        return Vec::new();
    }
    let rope = Rope::from(text);
    let mut highlighter = SyntaxHighlighter::new(language);
    highlighter.update(None, &rope, None);
    highlighter
        .styles(&(0..text.len()), &PierreSyntax { dark })
        .into_iter()
        .filter(|(range, style)| !range.is_empty() && *style != HighlightStyle::default())
        .collect()
}

/// Highlights `text` and splits the spans per line (`\n`-separated). Ranges are
/// relative to each line's start. Lines over [`MAX_HIGHLIGHT_LINE_BYTES`] get none.
pub fn highlight_lines(
    text: &str,
    language: &str,
    dark: bool,
) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
    let spans = highlight(text, language, dark);
    split_spans_by_line(text, &spans)
}

/// Splits document-level spans into per-line spans (see [`highlight_lines`]).
pub fn split_spans_by_line(
    text: &str,
    spans: &[(Range<usize>, HighlightStyle)],
) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
    let mut out = Vec::new();
    let mut span_ix = 0;
    let mut start = 0;
    for line in text.split('\n') {
        let end = start + line.len();
        let mut line_spans = Vec::new();
        while span_ix < spans.len() && spans[span_ix].0.end <= start {
            span_ix += 1;
        }
        if line.len() <= MAX_HIGHLIGHT_LINE_BYTES {
            let mut ix = span_ix;
            while ix < spans.len() && spans[ix].0.start < end {
                let (range, style) = &spans[ix];
                let s = range.start.max(start);
                let e = range.end.min(end);
                if s < e {
                    line_spans.push((s - start..e - start, *style));
                }
                ix += 1;
            }
        }
        out.push(line_spans);
        start = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_parent_capture() {
        let style = syntax_style("keyword.control.import", true).unwrap();
        assert_eq!(style.color, Some(hsla(0xff678d)));
        assert!(syntax_style("nonsense", true).is_none());
    }

    #[test]
    fn light_colors_are_darkened_until_readable() {
        // Already readable: unchanged.
        assert_eq!(readable_on_light_code(0x0a0a0a), 0x0a0a0a);
        let keyword = readable_on_light_code(0xd32a61);
        assert!(
            contrast_on_light_code([(keyword >> 16) as u8, (keyword >> 8) as u8, keyword as u8])
                >= 4.5
        );
        // The "further" set clears 6.
        let number = readable_on_light_code(0x1ca1c7);
        assert!(
            contrast_on_light_code([(number >> 16) as u8, (number >> 8) as u8, number as u8]) >= 6.
        );
        // Keeps the hue: red stays the largest channel.
        assert!((keyword >> 16) & 0xff > (keyword >> 8) & 0xff);
    }

    #[test]
    fn languages_from_paths() {
        assert_eq!(language_for_path("crates/app/src/lib.rs"), "rust");
        assert_eq!(language_for_path("notes.py"), "python");
        assert_eq!(language_for_path("pyproject.toml"), "toml");
        assert_eq!(language_for_path("README.md"), "markdown");
        assert_eq!(language_for_path(".gitignore"), "text");
        assert_eq!(language_for_path("Makefile"), "make");
    }

    #[test]
    fn highlights_python() {
        let code = "import json\nSTORE = Path(\"x\")\n";
        let lines = highlight_lines(code, "python", true);
        assert_eq!(lines.len(), 3);
        let import = lines[0].iter().find(|(r, _)| *r == (0..6)).unwrap();
        assert_eq!(import.1.color, Some(hsla(0xff678d)));
        let string = lines[1]
            .iter()
            .find(|(r, _)| code[12..][r.clone()].starts_with('"'));
        assert!(string.is_some());
    }

    #[test]
    fn splits_spans_across_lines() {
        let style = HighlightStyle::default();
        let spans = vec![(2..9, style)];
        let lines = split_spans_by_line("abcd\nefgh", &spans);
        assert_eq!(lines[0], vec![(2..4, style)]);
        assert_eq!(lines[1], vec![(0..4, style)]);
    }
}
