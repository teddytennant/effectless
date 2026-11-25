//! Markdown → GPUI, styled like T3's `ChatMarkdown` (spec 03 §4). Usable anywhere:
//!
//! ```ignore
//! let doc = markdown::parse(text, &ParseOptions::default());
//! markdown::render(&doc, &markdown::Options::new("plan-1"), window, cx)
//! ```
//!
//! Streaming text goes through [`Cache`], which keeps the parsed blocks of a message
//! and only reparses the unfinished tail when the text grows.

pub mod highlight;
pub mod parse;
pub mod render;

use std::collections::HashMap;
use std::rc::Rc;

pub use parse::{Block, Document, FileLink, Inline, ParseOptions, parse};
pub use render::{FileHandler, Options, Variant, chip_color, plain_text, render, render_inline};

struct Entry {
    text_hash: u64,
    text_len: usize,
    /// Parsed blocks of `text[..stable_len]`, reused while the text only grows.
    stable_len: usize,
    stable_prefix_hash: u64,
    stable: Vec<Block>,
    doc: Rc<Document>,
    preserve_breaks: bool,
    used: u64,
}

/// Parsed documents keyed by an owner id (message id, plan id). Unchanged text
/// returns the same `Rc`; appended text reparses only after the last finished block.
#[derive(Default)]
pub struct Cache {
    entries: HashMap<String, Entry>,
    tick: u64,
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl Cache {
    pub fn get(&mut self, key: &str, text: &str, preserve_breaks: bool) -> Rc<Document> {
        self.tick += 1;
        let tick = self.tick;
        let text_hash = hash(text);
        let opts = ParseOptions { preserve_breaks };
        if let Some(e) = self.entries.get_mut(key) {
            if e.text_hash == text_hash && e.text_len == text.len() && e.preserve_breaks == preserve_breaks {
                e.used = tick;
                return e.doc.clone();
            }
            // Growing text: keep the finished prefix if it is unchanged.
            if e.preserve_breaks == preserve_breaks
                && e.stable_len > 0
                && text.len() >= e.stable_len
                && text.is_char_boundary(e.stable_len)
                && hash(&text[..e.stable_len]) == e.stable_prefix_hash
            {
                let stable_len = parse::stable_prefix_len(text).max(e.stable_len);
                if stable_len > e.stable_len {
                    let more = parse(&text[e.stable_len..stable_len], &opts);
                    e.stable.extend(more.blocks.into_iter().filter(|b| !matches!(b, Block::Footnotes(_))));
                    e.stable_len = stable_len;
                    e.stable_prefix_hash = hash(&text[..stable_len]);
                }
                let tail = parse(&text[e.stable_len..], &opts);
                let mut blocks = e.stable.clone();
                blocks.extend(tail.blocks);
                e.doc = Rc::new(Document { blocks });
                e.text_hash = text_hash;
                e.text_len = text.len();
                e.used = tick;
                return e.doc.clone();
            }
        }
        let stable_len = parse::stable_prefix_len(text);
        let (stable, doc) = if stable_len > 0 && !text.contains("[^") {
            let head = parse(&text[..stable_len], &opts);
            let tail = parse(&text[stable_len..], &opts);
            let mut blocks = head.blocks.clone();
            blocks.extend(tail.blocks);
            (head.blocks, Document { blocks })
        } else {
            (Vec::new(), parse(text, &opts))
        };
        let doc = Rc::new(doc);
        // Footnote definitions can sit anywhere, so those documents always reparse.
        let stable_len = if text.contains("[^") { 0 } else { stable_len };
        self.entries.insert(
            key.to_string(),
            Entry {
                text_hash,
                text_len: text.len(),
                stable_len,
                stable_prefix_hash: hash(&text[..stable_len]),
                stable,
                doc: doc.clone(),
                preserve_breaks,
                used: tick,
            },
        );
        if self.entries.len() > 400 {
            let mut ages: Vec<u64> = self.entries.values().map(|e| e.used).collect();
            ages.sort_unstable();
            let cutoff = ages[ages.len() - 300];
            self.entries.retain(|_, e| e.used >= cutoff);
        }
        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reuses_unchanged_and_extends_growing_text() {
        let mut c = Cache::default();
        let a = c.get("m", "First para.\n\nSecond", false);
        let b = c.get("m", "First para.\n\nSecond", false);
        assert!(Rc::ptr_eq(&a, &b));
        let grown = c.get("m", "First para.\n\nSecond para.\n\n- item", false);
        assert_eq!(grown.blocks.len(), 3);
        assert_eq!(*grown, parse("First para.\n\nSecond para.\n\n- item", &ParseOptions::default()));
        // A rewrite (not an append) parses from scratch.
        let other = c.get("m", "Different", false);
        assert_eq!(other.blocks.len(), 1);
    }
}
