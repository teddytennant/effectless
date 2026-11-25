//! Markdown list continuation for Enter and Tab (`composer-list-continuation.ts`),
//! as pure string functions. Offsets are byte offsets.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEdit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
    /// Caret offset after the edit is applied.
    pub cursor_after: usize,
}

impl ListEdit {
    pub fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + self.replacement.len());
        out.push_str(&text[..self.start]);
        out.push_str(&self.replacement);
        out.push_str(&text[self.end..]);
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Marker {
    Ordered {
        indent: String,
        number: String,
        delim: char,
        space: String,
    },
    Task {
        indent: String,
        space: String,
    },
    Bullet {
        indent: String,
        bullet: char,
        space: String,
    },
}

impl Marker {
    fn indent(&self) -> &str {
        match self {
            Self::Ordered { indent, .. }
            | Self::Task { indent, .. }
            | Self::Bullet { indent, .. } => indent,
        }
    }
    fn space(&self) -> &str {
        match self {
            Self::Ordered { space, .. } | Self::Task { space, .. } | Self::Bullet { space, .. } => {
                space
            }
        }
    }
}

struct Parsed {
    marker: Marker,
    marker_end: usize,
    content_column: usize,
}

fn indent_width(s: &str, from: usize) -> usize {
    let mut col = from;
    for c in s.chars() {
        match c {
            ' ' => col += 1,
            '\t' => col += 4 - (col % 4),
            _ => break,
        }
    }
    col - from
}

fn content_column(indent: &str, marker_width: usize, padding: &str) -> usize {
    let marker_end = indent_width(indent, 0) + marker_width;
    let space = indent_width(padding, marker_end);
    marker_end + if (1..=4).contains(&space) { space } else { 1 }
}

/// Spaces/tabs run at the start of `s`, and whether the rest of the line is blank.
fn padding(s: &str) -> Option<String> {
    let pad: String = s.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    if !pad.is_empty() || s.trim().is_empty() {
        Some(if pad.is_empty() { String::new() } else { pad })
    } else {
        None
    }
}

fn parse_marker(line: &str) -> Option<Parsed> {
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let rest = &line[indent.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let after = &rest[digits.len()..];
        let delim = after.chars().next()?;
        if delim == '.' || delim == ')' {
            let space = padding(&after[1..])?;
            let marker_end = indent.len() + digits.len() + 1 + space.len();
            let content_column = content_column(&indent, digits.len() + 1, &space);
            return Some(Parsed {
                marker: Marker::Ordered {
                    indent,
                    number: digits,
                    delim,
                    space,
                },
                marker_end,
                content_column,
            });
        }
        return None;
    }
    if let Some(after) = rest.strip_prefix('-')
        && let Some(sep) = after.chars().next()
        && sep.is_whitespace()
        && sep != '\n'
    {
        let box_part = &after[sep.len_utf8()..];
        if box_part.len() >= 3
            && box_part.starts_with('[')
            && matches!(box_part.as_bytes()[1], b' ' | b'x' | b'X')
            && box_part.as_bytes()[2] == b']'
            && let Some(space) = padding(&box_part[3..])
        {
            let marker_end = indent.len() + 1 + sep.len_utf8() + 3 + space.len();
            let content_column = content_column(&indent, 1, &sep.to_string());
            return Some(Parsed {
                marker: Marker::Task { indent, space },
                marker_end,
                content_column,
            });
        }
    }
    let bullet = rest.chars().next()?;
    if matches!(bullet, '-' | '*' | '+') {
        let space = padding(&rest[1..])?;
        let marker_end = indent.len() + 1 + space.len();
        let content_column = content_column(&indent, 1, &space);
        return Some(Parsed {
            marker: Marker::Bullet {
                indent,
                bullet,
                space,
            },
            marker_end,
            content_column,
        });
    }
    None
}

fn next_ordered(number: &str) -> String {
    match number.parse::<u64>() {
        Ok(n) => format!("{:0width$}", n + 1, width = number.len()),
        Err(_) => number.to_string(),
    }
}

fn next_marker_text(marker: &Marker) -> String {
    match marker {
        Marker::Ordered {
            indent,
            number,
            delim,
            ..
        } => format!("{indent}{}{delim} ", next_ordered(number)),
        Marker::Task { indent, .. } => format!("{indent}- [ ] "),
        Marker::Bullet { indent, bullet, .. } => format!("{indent}{bullet} "),
    }
}

struct Line {
    start: usize,
    end: usize,
}

fn current_line(value: &str, cursor: usize) -> Line {
    let start = value[..cursor].rfind('\n').map_or(0, |i| i + 1);
    let end = value[cursor..]
        .find('\n')
        .map_or(value.len(), |i| cursor + i);
    Line { start, end }
}

/// Markers of the contiguous list lines above `line_start`, nearest first.
fn markers_above(value: &str, mut line_start: usize) -> Vec<Parsed> {
    let mut out = Vec::new();
    while line_start > 0 {
        let line_end = line_start - 1;
        line_start = value[..line_end].rfind('\n').map_or(0, |i| i + 1);
        match parse_marker(&value[line_start..line_end]) {
            Some(p) => out.push(p),
            None => break,
        }
    }
    out
}

fn parent_marker(value: &str, line_start: usize, marker: &Marker) -> Option<Marker> {
    let width = indent_width(marker.indent(), 0);
    markers_above(value, line_start)
        .into_iter()
        .find(|p| indent_width(p.marker.indent(), 0) < width)
        .map(|p| p.marker)
}

/// Renumbers ordered siblings after `after_line_end`. Returns (end, text) covering
/// `after_line_end..end`.
fn renumber_following(
    value: &str,
    after_line_end: usize,
    marker: &Marker,
) -> Option<(usize, String)> {
    let Marker::Ordered {
        indent,
        number,
        delim,
        ..
    } = marker
    else {
        return None;
    };
    let base = indent_width(indent, 0);
    let mut expected = number.parse::<u64>().ok()? + 2;
    let mut position = after_line_end;
    let mut copied = after_line_end;
    let mut out = String::new();
    let mut changed = false;
    while position < value.len() {
        let line_start = position + 1;
        let line_end = value[line_start..]
            .find('\n')
            .map_or(value.len(), |i| line_start + i);
        position = line_end;
        let line = &value[line_start..line_end];
        let width = indent_width(line, 0);
        if line.trim().is_empty() || width < base {
            break;
        }
        if width > base {
            continue;
        }
        let Some(Parsed {
            marker:
                Marker::Ordered {
                    indent: ci,
                    number: cn,
                    delim: cd,
                    ..
                },
            ..
        }) = parse_marker(line)
        else {
            break;
        };
        if cd != *delim {
            break;
        }
        let num_start = line_start + ci.len();
        let num_end = num_start + cn.len();
        let replacement = if cn.starts_with('0') {
            format!("{:0width$}", expected, width = cn.len())
        } else {
            expected.to_string()
        };
        if replacement != cn {
            out.push_str(&value[copied..num_start]);
            out.push_str(&replacement);
            copied = num_end;
            changed = true;
        }
        expected += 1;
    }
    changed.then_some((copied, out))
}

/// Enter inside a list item: continue the list, or end it on an empty item.
pub fn continuation_for_enter(value: &str, cursor: usize) -> Option<ListEdit> {
    if cursor > value.len() || !value.is_char_boundary(cursor) {
        return None;
    }
    let line = current_line(value, cursor);
    let parsed = parse_marker(&value[line.start..line.end])?;
    if parsed.marker.space().is_empty() {
        return None;
    }
    let marker_end = line.start + parsed.marker_end;
    if cursor < marker_end {
        return None;
    }
    if value[marker_end..line.end].trim().is_empty() {
        let parent = parent_marker(value, line.start, &parsed.marker);
        let replacement = parent.as_ref().map(next_marker_text).unwrap_or_default();
        let cursor_after = line.start + replacement.len();
        let end = cursor.max(marker_end);
        if let Some(parent) = &parent
            && let Some((renum_end, renum)) = renumber_following(value, line.end, parent)
        {
            return Some(ListEdit {
                start: line.start,
                end: renum_end,
                replacement: format!("{replacement}{}{renum}", &value[end..line.end]),
                cursor_after,
            });
        }
        return Some(ListEdit {
            start: line.start,
            end,
            replacement,
            cursor_after,
        });
    }
    let insertion = format!("\n{}", next_marker_text(&parsed.marker));
    let cursor_after = cursor + insertion.len();
    if matches!(parsed.marker, Marker::Ordered { .. })
        && let Some((renum_end, renum)) = renumber_following(value, line.end, &parsed.marker)
    {
        return Some(ListEdit {
            start: cursor,
            end: renum_end,
            replacement: format!("{insertion}{}{renum}", &value[cursor..line.end]),
            cursor_after,
        });
    }
    Some(ListEdit {
        start: cursor,
        end: cursor,
        replacement: insertion,
        cursor_after,
    })
}

fn nested_number(marker: &Marker, previous: Option<&Marker>) -> (usize, String) {
    let Marker::Ordered { number, delim, .. } = marker else {
        return (0, String::new());
    };
    let text = match previous {
        Some(Marker::Ordered {
            number: pn,
            delim: pd,
            ..
        }) if pd == delim => next_ordered(pn),
        _ => "1".to_string(),
    };
    (number.len(), text)
}

/// Tab in a list item: nest it under the item above.
pub fn indent_for_tab(value: &str, cursor: usize) -> Option<ListEdit> {
    if cursor > value.len() || !value.is_char_boundary(cursor) {
        return None;
    }
    let line = current_line(value, cursor);
    let parsed = parse_marker(&value[line.start..line.end])?;
    let marker = parsed.marker;
    let width = indent_width(marker.indent(), 0);
    let mut previous_child: Option<Marker> = None;
    for above in markers_above(value, line.start) {
        let above_width = indent_width(above.marker.indent(), 0);
        if above_width < width {
            break;
        }
        if above_width > width {
            if previous_child
                .as_ref()
                .is_none_or(|p| above_width < indent_width(p.indent(), 0))
            {
                previous_child = Some(above.marker.clone());
            }
            continue;
        }
        let indent = previous_child
            .as_ref()
            .map(|p| p.indent().to_string())
            .unwrap_or_else(|| {
                format!(
                    "{}{}",
                    above.marker.indent(),
                    " ".repeat(above.content_column - above_width)
                )
            });
        let (replaced_len, number) = nested_number(&marker, previous_child.as_ref());
        let replaced_end = line.start + marker.indent().len() + replaced_len;
        let replacement = format!("{indent}{number}");
        let cursor_after = if cursor < replaced_end {
            line.start + indent.len()
        } else {
            cursor - replaced_end + line.start + replacement.len()
        };
        return Some(ListEdit {
            start: line.start,
            end: replaced_end,
            replacement,
            cursor_after,
        });
    }
    Some(ListEdit {
        start: line.start,
        end: line.start,
        replacement: "  ".into(),
        cursor_after: cursor + 2,
    })
}

/// Shift+Tab in a nested list item: move it out one level.
pub fn outdent_for_tab(value: &str, cursor: usize) -> Option<ListEdit> {
    if cursor > value.len() || !value.is_char_boundary(cursor) {
        return None;
    }
    let line = current_line(value, cursor);
    let parsed = parse_marker(&value[line.start..line.end])?;
    let marker = parsed.marker;
    if marker.indent().is_empty() {
        return None;
    }
    let parent = parent_marker(value, line.start, &marker);
    let indent = match &parent {
        Some(p) => p.indent().to_string(),
        None => {
            let i = marker.indent();
            if let Some(rest) = i.strip_prefix('\t') {
                rest.to_string()
            } else if let Some(rest) = i.strip_prefix("  ") {
                rest.to_string()
            } else {
                i.strip_prefix(' ').unwrap_or(i).to_string()
            }
        }
    };
    let continues = matches!(
        (&parent, &marker),
        (Some(Marker::Ordered { delim: a, .. }), Marker::Ordered { delim: b, .. }) if a == b
    );
    let (replaced_len, number) = if continues {
        nested_number(&marker, parent.as_ref())
    } else {
        (0, String::new())
    };
    let replaced_end = line.start + marker.indent().len() + replaced_len;
    let replacement = format!("{indent}{number}");
    let cursor_after = if cursor < replaced_end {
        line.start + indent.len()
    } else {
        cursor - replaced_end + line.start + replacement.len()
    };
    if continues
        && let Some(parent) = &parent
        && let Some((renum_end, renum)) = renumber_following(value, line.end, parent)
    {
        return Some(ListEdit {
            start: line.start,
            end: renum_end,
            replacement: format!("{replacement}{}{renum}", &value[replaced_end..line.end]),
            cursor_after,
        });
    }
    Some(ListEdit {
        start: line.start,
        end: replaced_end,
        replacement,
        cursor_after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enter(text: &str) -> (String, usize) {
        let cursor = text.find('|').unwrap();
        let value = text.replacen('|', "", 1);
        let edit = continuation_for_enter(&value, cursor).expect("edit");
        (edit.apply(&value), edit.cursor_after)
    }

    fn show(text: &str, cursor: usize) -> String {
        format!("{}|{}", &text[..cursor], &text[cursor..])
    }

    #[test]
    fn continues_bullets_and_tasks() {
        let (t, c) = enter("- one|");
        assert_eq!(show(&t, c), "- one\n- |");
        let (t, c) = enter("- [x] done|");
        assert_eq!(show(&t, c), "- [x] done\n- [ ] |");
        let (t, c) = enter("  * nested|");
        assert_eq!(show(&t, c), "  * nested\n  * |");
    }

    #[test]
    fn increments_ordered_with_padding_and_renumbers() {
        let (t, c) = enter("09. nine|");
        assert_eq!(show(&t, c), "09. nine\n10. |");
        let (t, c) = enter("1. a|\n2. b\n3. c");
        assert_eq!(show(&t, c), "1. a\n2. |\n3. b\n4. c");
    }

    #[test]
    fn empty_item_ends_the_list_or_drops_to_parent() {
        let (t, c) = enter("- a\n- |");
        assert_eq!(show(&t, c), "- a\n|");
        let (t, c) = enter("1. a\n   - |");
        assert_eq!(show(&t, c), "1. a\n2. |");
    }

    #[test]
    fn no_continuation_outside_lists() {
        assert_eq!(continuation_for_enter("plain", 5), None);
        assert_eq!(continuation_for_enter("-nope", 5), None);
        assert_eq!(continuation_for_enter("- a", 1), None);
    }

    #[test]
    fn tab_nests_and_shift_tab_outdents() {
        let value = "- a\n- b";
        let edit = indent_for_tab(value, value.len()).unwrap();
        assert_eq!(edit.apply(value), "- a\n  - b");
        let nested = edit.apply(value);
        let out = outdent_for_tab(&nested, nested.len()).unwrap();
        assert_eq!(out.apply(&nested), "- a\n- b");
        let ordered = "1. a\n2. b";
        let edit = indent_for_tab(ordered, ordered.len()).unwrap();
        assert_eq!(edit.apply(ordered), "1. a\n   1. b");
        assert_eq!(indent_for_tab("plain", 2), None);
    }
}
