//! Code highlighting: tree-sitter through gpui-component's `SyntaxHighlighter`, with
//! capture colors ported from Shiki's `pierre-dark` and `pierre-light` themes. Light
//! mode darkens each color the way T3's `readableSyntaxTheme.ts` does, so it reads on
//! light code surfaces. Results are cached per (language, code, appearance).

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use gpui::{FontStyle, HighlightStyle, Hsla, Rgba};
use gpui_component::Rope;
use gpui_component::highlighter::SyntaxHighlighter;

pub type Highlights = Rc<Vec<(Range<usize>, HighlightStyle)>>;

/// Languages that render without highlighting (`text`, `txt`, `plain`, `plaintext`).
pub fn is_plain(lang: Option<&str>) -> bool {
    matches!(
        lang.map(str::to_lowercase).as_deref(),
        None | Some("text" | "txt" | "plain" | "plaintext" | "" | "math" | "mermaid")
    )
}

/// Fence names → gpui-component grammar names.
fn grammar(lang: &str) -> &str {
    match lang {
        "shell" | "zsh" | "fish" | "console" | "shellscript" | "sh" => "bash",
        "py" | "python3" => "python",
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "yml" => "yaml",
        "md" => "markdown",
        "jsonc" | "json5" => "json",
        "golang" => "go",
        "rb" => "ruby",
        "c++" | "cc" | "hpp" => "cpp",
        "cs" => "csharp",
        "patch" => "diff",
        other => other,
    }
}

fn hex(rgb: u32) -> Hsla {
    Rgba {
        r: ((rgb >> 16) & 0xff) as f32 / 255.,
        g: ((rgb >> 8) & 0xff) as f32 / 255.,
        b: (rgb & 0xff) as f32 / 255.,
        a: 1.,
    }
    .into()
}

fn channel_luminance(c: u8) -> f64 {
    let v = c as f64 / 255.;
    if v <= 0.03928 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// `readableOnLightCode`: darkens a color in 4% steps until it reaches `min` contrast
/// against the darkest light code surface (luminance 0.69).
pub fn readable_on_light(rgb: u32, min: f64) -> u32 {
    let mut c = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    let contrast = |c: &[u8; 3]| {
        let l = 0.2126 * channel_luminance(c[0])
            + 0.7152 * channel_luminance(c[1])
            + 0.0722 * channel_luminance(c[2]);
        (0.69 + 0.05) / (l + 0.05)
    };
    while contrast(&c) < min {
        c = c.map(|v| (v as f64 * 0.96).floor() as u8);
    }
    ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32
}

/// A capture's pierre color: (dark, light). `None` = inherit the code foreground.
fn pierre(capture: &str) -> Option<(u32, u32)> {
    let base = capture.split('.').next().unwrap_or(capture);
    Some(match capture {
        "comment" | "comment.doc" => (0x737373, 0x737373),
        "string.escape" => (0x61d5c0, 0x16a994),
        "string.regex" => (0x64d1db, 0x17a5af),
        "string.special.symbol" => (0x68cdf2, 0x1ca1c7),
        "number" | "boolean" | "constant.builtin" => (0x68cdf2, 0x1ca1c7),
        "variable.special" | "variable.builtin" => (0xffab16, 0xd5901c),
        "variable.parameter" => (0xa3a3a3, 0x636363),
        "variable" => return None,
        "punctuation.special" => (0xff678d, 0xd32a61),
        "punctuation.list_marker" => (0xff855e, 0xd5512f),
        "function.builtin" | "function.method" | "function.call" => (0x9d6afb, 0x693acf),
        "type.builtin" => (0xd568ea, 0xa631be),
        "tag.doctype" => (0x636363, 0x636363),
        "text.literal" | "text.code.span" => (0x5ecc71, 0x199f43),
        "link_uri" => (0xff678d, 0xd32a61),
        "link_text" => (0x9d6afb, 0x693acf),
        "emphasis.strong" => (0xffd452, 0xd5a910),
        _ => match base {
            "comment" => (0x737373, 0x737373),
            "string" => (0x5ecc71, 0x199f43),
            "constant" => (0xffd452, 0xd5a910),
            "keyword" | "preproc" => (0xff678d, 0xd32a61),
            "operator" => (0x08c0ef, 0x08c0ef),
            "function" | "constructor" => (0x9d6afb, 0x693acf),
            "type" | "enum" | "variant" => (0xd568ea, 0xa631be),
            "property" => (0xffa359, 0xd47628),
            "attribute" => (0x60d199, 0x18a46c),
            "tag" | "title" => (0xff855e, 0xd5512f),
            "punctuation" => (0x636363, 0x636363),
            "label" => (0xffab16, 0xd5901c),
            "embedded" => return None,
            "emphasis" => (0xff678d, 0xd32a61),
            _ => return None,
        },
    })
}

/// Pairs that differ only in lightness go further so they keep their order.
const FURTHER: [u32; 3] = [0x1ca1c7, 0x636363, 0x1a85d4];

struct Pierre {
    dark: bool,
}

impl gpui_base::input::HighlightStyleResolver for Pierre {
    fn style(&self, name: &str) -> Option<HighlightStyle> {
        let (dark, light) = pierre(name)?;
        let rgb = if self.dark {
            dark
        } else {
            readable_on_light(light, if FURTHER.contains(&light) { 6. } else { 4.5 })
        };
        Some(HighlightStyle {
            color: Some(hex(rgb)),
            font_style: name.starts_with("emphasis").then_some(FontStyle::Italic),
            ..Default::default()
        })
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<u64, Highlights>> = RefCell::new(HashMap::new());
}

/// Highlight ranges for `code` (byte offsets). Empty for plain fences.
pub fn highlight(lang: Option<&str>, code: &str, dark: bool) -> Highlights {
    if is_plain(lang) || code.is_empty() {
        return Rc::new(Vec::new());
    }
    let lang = grammar(&lang.unwrap_or_default().to_lowercase()).to_string();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (&lang, code, dark).hash(&mut hasher);
    let key = hasher.finish();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }
    let rope = Rope::from(code);
    let mut highlighter = SyntaxHighlighter::new(&lang);
    highlighter.update(None, &rope, None);
    let styles: Vec<(Range<usize>, HighlightStyle)> = highlighter
        .styles(&(0..code.len()), &Pierre { dark })
        .into_iter()
        .filter(|(_, s)| s.color.is_some())
        .collect();
    let styles = Rc::new(styles);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 2_000 {
            c.clear();
        }
        c.insert(key, styles.clone());
    });
    styles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_strings_darken_like_t3() {
        // pierre-light #199f43 renders as rgb(16, 106, 43) in T3 (computed styles).
        assert_eq!(readable_on_light(0x199f43, 4.5), 0x106a2b);
    }

    #[test]
    fn highlights_python() {
        let h = highlight(Some("python"), "def f():\n    return \"x\"\n", true);
        assert!(!h.is_empty());
        assert!(highlight(Some("text"), "plain", true).is_empty());
    }
}
