//! Markdown → a small block tree the renderer lays out (GFM via the `markdown` crate:
//! tables, task lists, strikethrough, autolinks, footnotes), plus T3's extras: GitHub
//! alerts (`> [!NOTE]`), file links from link destinations and inline code
//! (`markdown-links.ts`, `fileLinks.ts`), and the streaming split that only reparses
//! the unfinished tail.

use std::ops::Range;

use markdown::mdast::{self, Node};

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Paragraph(Inline),
    Heading {
        depth: u8,
        text: Inline,
    },
    List {
        ordered: bool,
        start: u32,
        items: Vec<ListItem>,
    },
    Code {
        lang: Option<String>,
        title: Option<String>,
        code: String,
    },
    Quote(Vec<Block>),
    Alert {
        kind: AlertKind,
        blocks: Vec<Block>,
    },
    Table {
        align: Vec<Align>,
        header: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
    Rule,
    Footnotes(Vec<(String, Vec<Block>)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// A run of text with styled ranges (byte offsets into `text`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: SpanStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpanStyle {
    Strong,
    Emphasis,
    Strike,
    Code,
    Link(String),
    File(FileLink),
    FootnoteRef,
}

/// A path the chip opens (`MarkdownFileLink`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLink {
    pub path: String,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub column: Option<u32>,
}

impl FileLink {
    /// Chip label: basename plus " · L12", " · L12–20" or " · L12:C4".
    pub fn label(&self) -> String {
        let base = self
            .path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&self.path)
            .to_string();
        match (self.line, self.end_line, self.column) {
            (Some(l), Some(e), _) => format!("{base} · L{l}–{e}"),
            (Some(l), None, Some(c)) => format!("{base} · L{l}:C{c}"),
            (Some(l), None, None) => format!("{base} · L{l}"),
            _ => base,
        }
    }
}

impl Inline {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    fn push_styled(&mut self, text: &str, style: SpanStyle) {
        let start = self.text.len();
        self.text.push_str(text);
        self.spans.push(Span {
            range: start..self.text.len(),
            style,
        });
    }

    /// Plain text with markers dropped (copy, find).
    pub fn plain(&self) -> &str {
        &self.text
    }
}

pub struct ParseOptions {
    /// User messages keep soft line breaks and never parse raw HTML.
    pub preserve_breaks: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            preserve_breaks: false,
        }
    }
}

pub fn parse(text: &str, opts: &ParseOptions) -> Document {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.math_flow = false;
    options.constructs.math_text = false;
    let Ok(root) = markdown::to_mdast(text, &options) else {
        return Document {
            blocks: vec![Block::Paragraph(Inline {
                text: text.to_string(),
                spans: vec![],
            })],
        };
    };
    let mut cx = Cx {
        opts,
        footnotes: Vec::new(),
    };
    let mut blocks = match &root {
        Node::Root(r) => cx.blocks(&r.children),
        _ => Vec::new(),
    };
    if !cx.footnotes.is_empty() {
        blocks.push(Block::Footnotes(std::mem::take(&mut cx.footnotes)));
    }
    Document { blocks }
}

struct Cx<'a> {
    opts: &'a ParseOptions,
    footnotes: Vec<(String, Vec<Block>)>,
}

impl Cx<'_> {
    fn blocks(&mut self, nodes: &[Node]) -> Vec<Block> {
        let mut out = Vec::new();
        for node in nodes {
            match node {
                Node::Paragraph(p) => {
                    let inline = self.inline(&p.children);
                    if !inline.text.is_empty() {
                        out.push(Block::Paragraph(inline));
                    }
                }
                Node::Heading(h) => out.push(Block::Heading {
                    depth: h.depth,
                    text: self.inline(&h.children),
                }),
                Node::List(l) => out.push(Block::List {
                    ordered: l.ordered,
                    start: l.start.unwrap_or(1),
                    items: l
                        .children
                        .iter()
                        .filter_map(|c| match c {
                            Node::ListItem(li) => Some(ListItem {
                                checked: li.checked,
                                blocks: self.blocks(&li.children),
                            }),
                            _ => None,
                        })
                        .collect(),
                }),
                Node::Code(c) => {
                    let (lang, title) = code_info(c);
                    out.push(Block::Code {
                        lang,
                        title,
                        code: c.value.clone(),
                    });
                }
                Node::Math(m) => out.push(Block::Code {
                    lang: Some("math".into()),
                    title: None,
                    code: m.value.clone(),
                }),
                Node::Blockquote(q) => out.push(self.quote(q)),
                Node::Table(t) => out.push(self.table(t)),
                Node::ThematicBreak(_) => out.push(Block::Rule),
                Node::Html(h) => out.push(Block::Paragraph(Inline {
                    text: h.value.clone(),
                    spans: vec![],
                })),
                Node::FootnoteDefinition(f) => {
                    let blocks = self.blocks(&f.children);
                    self.footnotes.push((f.identifier.clone(), blocks));
                }
                Node::Definition(_) | Node::Yaml(_) | Node::Toml(_) => {}
                other => {
                    // Stray phrasing content at block level.
                    let inline = self.inline(std::slice::from_ref(other));
                    if !inline.text.is_empty() {
                        out.push(Block::Paragraph(inline));
                    }
                }
            }
        }
        out
    }

    fn quote(&mut self, q: &mdast::Blockquote) -> Block {
        if let Some(Node::Paragraph(p)) = q.children.first()
            && let Some(Node::Text(t)) = p.children.first()
        {
            let head = t.value.trim_start();
            let kinds = [
                ("[!NOTE]", AlertKind::Note),
                ("[!TIP]", AlertKind::Tip),
                ("[!IMPORTANT]", AlertKind::Important),
                ("[!WARNING]", AlertKind::Warning),
                ("[!CAUTION]", AlertKind::Caution),
            ];
            for (marker, kind) in kinds {
                if head.len() >= marker.len() && head[..marker.len()].eq_ignore_ascii_case(marker) {
                    let rest = head[marker.len()..].trim_start_matches([' ', '\t']);
                    let rest = rest.strip_prefix('\n').unwrap_or(rest);
                    let mut para = p.clone();
                    if rest.is_empty() {
                        para.children.remove(0);
                        if let Some(Node::Break(_)) = para.children.first() {
                            para.children.remove(0);
                        }
                    } else if let Node::Text(t) = &mut para.children[0] {
                        t.value = rest.to_string();
                    }
                    let mut children = q.children.clone();
                    children[0] = Node::Paragraph(para);
                    return Block::Alert {
                        kind,
                        blocks: self.blocks(&children),
                    };
                }
            }
        }
        Block::Quote(self.blocks(&q.children))
    }

    fn table(&mut self, t: &mdast::Table) -> Block {
        let align = t
            .align
            .iter()
            .map(|a| match a {
                mdast::AlignKind::Center => Align::Center,
                mdast::AlignKind::Right => Align::Right,
                _ => Align::Left,
            })
            .collect();
        let mut rows: Vec<Vec<Inline>> = t
            .children
            .iter()
            .filter_map(|r| match r {
                Node::TableRow(r) => Some(
                    r.children
                        .iter()
                        .map(|c| match c {
                            Node::TableCell(c) => self.inline(&c.children),
                            _ => Inline::default(),
                        })
                        .collect(),
                ),
                _ => None,
            })
            .collect();
        let header = if rows.is_empty() {
            Vec::new()
        } else {
            rows.remove(0)
        };
        Block::Table {
            align,
            header,
            rows,
        }
    }

    fn inline(&mut self, nodes: &[Node]) -> Inline {
        let mut out = Inline::default();
        self.inline_into(nodes, &mut out);
        out
    }

    fn inline_into(&mut self, nodes: &[Node], out: &mut Inline) {
        for node in nodes {
            match node {
                Node::Text(t) => {
                    if self.opts.preserve_breaks {
                        out.text.push_str(&t.value);
                    } else {
                        // Soft breaks render as spaces.
                        out.text.push_str(&t.value.replace('\n', " "));
                    }
                }
                Node::Break(_) => out.text.push('\n'),
                Node::InlineCode(c) => match file_link_from_code(&c.value) {
                    Some(link) => {
                        let label = link.label();
                        out.push_styled(&label, SpanStyle::File(link));
                    }
                    None => out.push_styled(&c.value, SpanStyle::Code),
                },
                Node::InlineMath(m) => out.push_styled(&m.value, SpanStyle::Code),
                Node::Strong(s) => self.wrap(&s.children, out, SpanStyle::Strong),
                Node::Emphasis(e) => self.wrap(&e.children, out, SpanStyle::Emphasis),
                Node::Delete(d) => self.wrap(&d.children, out, SpanStyle::Strike),
                Node::Link(l) => match file_link_from_destination(&l.url) {
                    Some(link) => {
                        let mut label = Inline::default();
                        self.inline_into(&l.children, &mut label);
                        let text = if label.text.trim().is_empty() || label.text == l.url {
                            link.label()
                        } else {
                            label.text
                        };
                        out.push_styled(&text, SpanStyle::File(link));
                    }
                    None => self.wrap(&l.children, out, SpanStyle::Link(l.url.clone())),
                },
                Node::LinkReference(l) => self.inline_into(&l.children, out),
                Node::Image(i) => {
                    let alt = if i.alt.is_empty() { &i.url } else { &i.alt };
                    out.push_styled(alt, SpanStyle::Link(i.url.clone()));
                }
                Node::ImageReference(i) => out.text.push_str(&i.alt),
                Node::FootnoteReference(f) => {
                    out.push_styled(&f.label.clone().unwrap_or(f.identifier.clone()), SpanStyle::FootnoteRef)
                }
                Node::Html(h) => out.text.push_str(&h.value),
                Node::Paragraph(p) => self.inline_into(&p.children, out),
                _ => {}
            }
        }
    }

    fn wrap(&mut self, children: &[Node], out: &mut Inline, style: SpanStyle) {
        let start = out.text.len();
        self.inline_into(children, out);
        if out.text.len() > start {
            // Outer spans come first so inner ones override them when laid out.
            let insert_at = out
                .spans
                .iter()
                .position(|s| s.range.start >= start)
                .unwrap_or(out.spans.len());
            out.spans.insert(
                insert_at,
                Span {
                    range: start..out.text.len(),
                    style,
                },
            );
        }
    }
}

/// Fence info: language and an optional `title="a.ts"`.
fn code_info(c: &mdast::Code) -> (Option<String>, Option<String>) {
    let lang = c.lang.clone().filter(|l| !l.is_empty());
    let title = c.meta.as_deref().and_then(|meta| {
        let i = meta.find("title=")?;
        let rest = &meta[i + 6..];
        let rest = rest.trim_start_matches(['"', '\'']);
        let end = rest.find(['"', '\'', ' ']).unwrap_or(rest.len());
        Some(rest[..end].to_string()).filter(|s| !s.is_empty())
    });
    (lang, title)
}

const KNOWN_EXTENSIONLESS: [&str; 12] = [
    "Makefile",
    "Dockerfile",
    "Containerfile",
    "Justfile",
    "Rakefile",
    "Gemfile",
    "Procfile",
    "Vagrantfile",
    "LICENSE",
    "README",
    "CODEOWNERS",
    "Brewfile",
];

/// Splits a `:line`, `:line:col` or `:line-end` suffix.
fn split_position(s: &str) -> (&str, Option<u32>, Option<u32>, Option<u32>) {
    let mut parts = s.rsplitn(3, ':').collect::<Vec<_>>();
    parts.reverse();
    let num = |p: &str| p.parse::<u32>().ok().filter(|n| *n > 0);
    match parts.as_slice() {
        [path, a, b] if num(a).is_some() && num(b).is_some() => (path, num(a), None, num(b)),
        [path, a, b] => {
            // `path:12-20` with a colon inside the path is rare; only take the last part.
            let joined_len = path.len() + 1 + a.len();
            if let Some((l, e)) = b.split_once('-')
                && num(l).is_some()
                && num(e).is_some()
            {
                return (&s[..joined_len], num(l), num(e), None);
            }
            if let Some(n) = num(b) {
                return (&s[..joined_len], Some(n), None, None);
            }
            (s, None, None, None)
        }
        [path, a] => {
            if let Some((l, e)) = a.split_once('-')
                && num(l).is_some()
                && num(e).is_some()
            {
                return (path, num(l), num(e), None);
            }
            match num(a) {
                Some(n) => (path, Some(n), None, None),
                None => (s, None, None, None),
            }
        }
        _ => (s, None, None, None),
    }
}

fn has_extension(segment: &str) -> bool {
    match segment.rsplit_once('.') {
        Some((stem, ext)) => {
            !ext.is_empty()
                && ext.len() <= 10
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().any(|c| c.is_ascii_alphabetic())
                && !(stem.is_empty() && ext.len() > 8)
        }
        None => false,
    }
}

fn looks_like_version(s: &str) -> bool {
    let s = s.trim_start_matches('v');
    s.split('.').count() >= 2 && s.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn looks_like_host(s: &str) -> bool {
    !s.contains('/')
        && s.split('.').count() >= 2
        && ["com", "org", "net", "io", "dev", "app", "co", "ai", "sh"]
            .iter()
            .any(|tld| s.ends_with(&format!(".{tld}")))
}

/// Inline code that is a path (`src/app.ts:12`) becomes a file chip.
pub fn file_link_from_code(code: &str) -> Option<FileLink> {
    let t = code.trim();
    if t.is_empty() || t.contains('`') || t.contains("...") || t.contains('…') {
        return None;
    }
    if t.contains(char::is_whitespace) {
        return None;
    }
    if t.contains("://") || looks_like_version(t) || looks_like_host(t) {
        return None;
    }
    let (path, line, end_line, column) = split_position(t);
    let has_sep = path.contains('/');
    if !has_sep && line.is_none() {
        return None;
    }
    let explicit = path.starts_with('/')
        || path.starts_with("~/")
        || path.starts_with("./")
        || path.starts_with("../");
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    if !(explicit || has_extension(last) || KNOWN_EXTENSIONLESS.contains(&last)) {
        return None;
    }
    if path.split('/').any(|seg| seg.contains(['<', '>', '*', '?', '|', '"', '{', '}', '(', ')', '=', ','])) {
        return None;
    }
    Some(FileLink {
        path: path.to_string(),
        line,
        end_line,
        column,
    })
}

/// A markdown link destination that is a file path.
pub fn file_link_from_destination(url: &str) -> Option<FileLink> {
    let u = url.trim();
    if u.is_empty() || u.starts_with('#') || u.starts_with("//") {
        return None;
    }
    let u = if let Some(rest) = u.strip_prefix("file://") {
        rest
    } else if u.contains("://") || u.starts_with("mailto:") {
        return None;
    } else if let Some((scheme, _)) = u.split_once(':')
        && scheme.len() > 1
        && scheme.chars().all(|c| c.is_ascii_alphabetic())
        && !u.starts_with("C:")
        && !scheme.contains('.')
        && !scheme.contains('/')
        && split_position(u).1.is_none()
    {
        return None;
    } else {
        u
    };
    let (path, line, end_line, column) = split_position(u);
    // `#L12` anchors on paths.
    let (path, line) = match path.split_once("#L") {
        Some((p, l)) => (p, line.or(l.parse().ok())),
        None => (path, line),
    };
    let explicit = path.starts_with('/')
        || path.starts_with("~/")
        || path.starts_with("./")
        || path.starts_with("../");
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    let relative_ok = path.split('/').all(|s| !s.is_empty() && !s.contains(' ') || s.is_empty())
        && (has_extension(last) || KNOWN_EXTENSIONLESS.contains(&last));
    if !(explicit || relative_ok) {
        return None;
    }
    Some(FileLink {
        path: path.to_string(),
        line,
        end_line,
        column,
    })
}

/// Byte offset after the last blank line that is not inside a code fence: text before it
/// is made of finished blocks that later deltas cannot change.
pub fn stable_prefix_len(text: &str) -> usize {
    let mut fence: Option<(char, usize)> = None;
    let mut boundary = 0;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent < 4 {
            let marker = trimmed.chars().next();
            if let Some(c @ ('`' | '~')) = marker {
                let run = trimmed.chars().take_while(|ch| *ch == c).count();
                if run >= 3 {
                    match fence {
                        None => fence = Some((c, run)),
                        Some((fc, fr)) if fc == c && run >= fr && trimmed[run..].trim().is_empty() => {
                            fence = None
                        }
                        _ => {}
                    }
                }
            }
        }
        // Only a complete line counts; the last line may still be growing.
        if fence.is_none() && line.ends_with('\n') && line.trim().is_empty() && start > 0 {
            boundary = offset;
        }
    }
    // A blank line inside a list or quote continues it; keep the boundary only when
    // the next block cannot be a continuation (no indentation after it).
    if boundary > 0
        && let Some(next) = text[boundary..].lines().find(|l| !l.trim().is_empty())
        && (next.starts_with([' ', '\t']) || next.starts_with('>'))
    {
        return 0;
    }
    boundary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Vec<Block> {
        parse(text, &ParseOptions::default()).blocks
    }

    #[test]
    fn paragraphs_headings_lists() {
        let blocks = p("# Title\n\nHello **bold** and `code`.\n\n- a\n- [x] b\n\n1. one\n2. two\n");
        assert!(matches!(&blocks[0], Block::Heading { depth: 1, text } if text.text == "Title"));
        let Block::Paragraph(para) = &blocks[1] else { panic!() };
        assert_eq!(para.text, "Hello bold and code.");
        assert!(para.spans.iter().any(|s| s.style == SpanStyle::Strong && &para.text[s.range.clone()] == "bold"));
        assert!(para.spans.iter().any(|s| s.style == SpanStyle::Code));
        let Block::List { ordered: false, items, .. } = &blocks[2] else { panic!() };
        assert_eq!(items[1].checked, Some(true));
        assert!(matches!(&blocks[3], Block::List { ordered: true, start: 1, .. }));
    }

    #[test]
    fn code_fences_and_tables() {
        let blocks = p("```python title=\"notes.py\"\nprint(1)\n```\n\n| File | Summary |\n|---|:-:|\n| `a.py` | x |\n");
        assert_eq!(
            blocks[0],
            Block::Code {
                lang: Some("python".into()),
                title: Some("notes.py".into()),
                code: "print(1)".into()
            }
        );
        let Block::Table { header, rows, align } = &blocks[1] else { panic!() };
        assert_eq!(header[0].text, "File");
        assert_eq!(align[1], Align::Center);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn alerts() {
        let blocks = p("> [!WARNING]\n> Careful here.\n");
        let Block::Alert { kind, blocks } = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(*kind, AlertKind::Warning);
        assert!(matches!(&blocks[0], Block::Paragraph(i) if i.text == "Careful here."));
    }

    #[test]
    fn inline_code_file_links() {
        assert_eq!(
            file_link_from_code("src/app.ts:12"),
            Some(FileLink { path: "src/app.ts".into(), line: Some(12), end_line: None, column: None })
        );
        assert!(file_link_from_code("crates/app/src/chat/markdown.rs").is_some());
        assert!(file_link_from_code("notes.py").is_none());
        assert!(file_link_from_code("notes.py:4").is_some());
        assert!(file_link_from_code("1.2.3").is_none());
        assert!(file_link_from_code("example.com").is_none());
        assert!(file_link_from_code("a/b c").is_none());
        assert!(file_link_from_code("src/...").is_none());
        assert!(file_link_from_code("load()").is_none());
        assert!(file_link_from_code("~/dotfiles").is_some());
        assert!(file_link_from_code("docker/Dockerfile").is_some());
        assert_eq!(file_link_from_code("src/a.rs:3-9").unwrap().label(), "a.rs · L3–9");
        assert_eq!(file_link_from_code("src/a.rs:3:7").unwrap().label(), "a.rs · L3:C7");
    }

    #[test]
    fn link_destinations() {
        assert!(file_link_from_destination("src/app.rs").is_some());
        assert!(file_link_from_destination("./README.md#L4").unwrap().line == Some(4));
        assert!(file_link_from_destination("https://x.dev/a.rs").is_none());
        assert!(file_link_from_destination("#section").is_none());
        assert!(file_link_from_destination("/abs/path").is_some());
        assert!(file_link_from_destination("mailto:a@b.c").is_none());
    }

    #[test]
    fn links_and_file_links_in_text() {
        let blocks = p("See [docs](https://example.com) and [the file](src/lib.rs:10).");
        let Block::Paragraph(i) = &blocks[0] else { panic!() };
        assert!(i.spans.iter().any(|s| matches!(&s.style, SpanStyle::Link(u) if u == "https://example.com")));
        assert!(i.spans.iter().any(|s| matches!(&s.style, SpanStyle::File(f) if f.line == Some(10))));
    }

    #[test]
    fn stable_prefix() {
        assert_eq!(stable_prefix_len("one\n\ntwo"), 5);
        assert_eq!(stable_prefix_len("```\na\n\nb"), 0);
        assert_eq!(stable_prefix_len("```\na\n```\n\nnext"), 11);
        assert_eq!(stable_prefix_len("- a\n\n  b"), 0);
        assert_eq!(stable_prefix_len("single line"), 0);
    }

    #[test]
    fn user_variant_keeps_breaks() {
        let d = parse("a\nb", &ParseOptions { preserve_breaks: true });
        assert!(matches!(&d.blocks[0], Block::Paragraph(i) if i.text == "a\nb"));
        let d = parse("a\nb", &ParseOptions::default());
        assert!(matches!(&d.blocks[0], Block::Paragraph(i) if i.text == "a b"));
    }
}
