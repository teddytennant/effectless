//! The palette's search scorer (`packages/shared/src/paletteSearch.ts`, spec 02 §4.4),
//! shared with the sidebar's thread search. Server-side message search and the synced pick
//! history are left out (spec 02 §7); everything else ranks the same way.

use std::ops::Range;

const WHOLE_WORD: f32 = 1.0;
const WHOLE_NAME: f32 = 1.5;
const MID_WORD: f32 = 0.3;
const TYPO: f32 = 0.45;
const JOINED_WORDS: f32 = 0.6;
const KEYWORD_QUALITY: f32 = 0.65;
const KEYWORD_QUERY_MIN_LENGTH: usize = 4;
const PICK_PREFIX_MIN_LENGTH: usize = 3;
const USE_BOOST_LIMIT: f32 = 2.5;
const HALF_LIFE_MS: f64 = 14.0 * 24.0 * 3600.0 * 1000.0;

/// Palette limits.
pub const THREAD_LIMIT: usize = 20;
pub const SECONDARY_LIMIT: usize = 3;

const FILLER_WORDS: [&str; 10] = [
    "a", "an", "and", "for", "in", "of", "on", "the", "to", "with",
];

fn is_filler(token: &str) -> bool {
    FILLER_WORDS.contains(&token)
}

fn is_alnum(c: char) -> bool {
    c.is_alphanumeric()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff)
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

/// Folds one character: lowercase, Latin accents stripped, curly apostrophes straight. A
/// character that would fold to several stays as it is, so offsets line up with the source.
fn fold_char(c: char) -> char {
    let c = match c {
        '\u{2018}' | '\u{2019}' => '\'',
        'ς' => 'σ',
        _ => c,
    };
    let base = match c {
        'à'..='å' | 'À'..='Å' | 'ā' | 'Ā' | 'ă' | 'ą' => 'a',
        'ç' | 'Ç' | 'ć' | 'č' | 'Č' => 'c',
        'è'..='ë' | 'È'..='Ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ì'..='ï' | 'Ì'..='Ï' | 'ī' | 'į' => 'i',
        'ñ' | 'Ñ' | 'ń' | 'ň' => 'n',
        'ò'..='ö' | 'Ò'..='Ö' | 'ø' | 'Ø' | 'ō' | 'ő' => 'o',
        'ù'..='ü' | 'Ù'..='Ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' | 'Ý' => 'y',
        'š' | 'Š' | 'ś' => 's',
        'ž' | 'Ž' | 'ź' | 'ż' => 'z',
        'ř' => 'r',
        'ď' => 'd',
        'ť' => 't',
        'ł' => 'l',
        _ => c,
    };
    let mut lower = base.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => base,
    }
}

/// A folded field with its word starts.
#[derive(Clone, Debug)]
pub struct Field {
    text: Vec<char>,
    starts: Vec<bool>,
}

impl Field {
    pub fn new(source: &str) -> Self {
        let chars: Vec<char> = source.chars().collect();
        let starts = (0..chars.len())
            .map(|i| {
                let get = |j: isize| -> Option<char> {
                    (j >= 0).then(|| chars.get(j as usize).copied()).flatten()
                };
                let c = chars[i];
                let prev = get(i as isize - 1);
                let before = get(i as isize - 2);
                let next = get(i as isize + 1);
                is_word_start(before, prev, c, next)
            })
            .collect();
        Self {
            text: chars.into_iter().map(fold_char).collect(),
            starts,
        }
    }

    fn len(&self) -> usize {
        self.text.len()
    }

    fn starts_at(&self, i: usize) -> bool {
        self.starts.get(i).copied().unwrap_or(false)
    }

    fn find_from(&self, token: &[char], from: usize) -> Option<usize> {
        if token.is_empty() || token.len() > self.text.len() {
            return None;
        }
        (from..=self.text.len() - token.len()).find(|&i| self.text[i..i + token.len()] == *token)
    }

    fn word_end(&self, index: usize) -> usize {
        let mut end = index + 1;
        while end < self.len() && !self.starts_at(end) && is_alnum(self.text[end]) {
            end += 1;
        }
        end
    }
}

fn is_word_start(before: Option<char>, prev: Option<char>, c: char, next: Option<char>) -> bool {
    let symbol = !c.is_alphanumeric() && !c.is_whitespace() && !c.is_ascii_punctuation() && {
        // Emoji and other symbols (\p{So}) start words.
        let u = c as u32;
        (0x2600..=0x27bf).contains(&u) || u >= 0x1f300
    };
    if !(is_alnum(c) || symbol) {
        return false;
    }
    let after_apostrophe = prev.is_some_and(is_apostrophe) && before.is_some_and(is_alnum);
    if after_apostrophe {
        return false;
    }
    let Some(prev) = prev else {
        return true;
    };
    !is_alnum(prev)
        || (prev.is_lowercase() && c.is_uppercase())
        || (prev.is_uppercase() && c.is_uppercase() && next.is_some_and(char::is_lowercase))
        || prev.is_numeric() != c.is_numeric()
        || is_cjk(c)
}

/// The folded, whitespace-collapsed query; an exact query keeps its double quotes.
pub fn fold_query(query: &str) -> String {
    let trimmed = query.trim();
    let quote = |c: char| matches!(c, '"' | '\u{201c}' | '\u{201d}');
    let exact = trimmed.chars().count() > 2
        && trimmed.chars().next().is_some_and(quote)
        && trimmed.chars().last().is_some_and(quote);
    let unwrapped = trimmed.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '`' | '\u{201c}' | '\u{201d}' | '\u{2018}' | '\u{2019}'
        ) || c.is_whitespace()
    });
    let words: Vec<String> = unwrapped
        .split_whitespace()
        .map(|w| w.chars().map(fold_char).collect())
        .collect();
    let words = words.join(" ");
    if !words.is_empty() && exact {
        format!("\"{words}\"")
    } else {
        words
    }
}

fn exact_phrase(query: &str) -> Option<&str> {
    (query.chars().count() > 2 && query.starts_with('"') && query.ends_with('"'))
        .then(|| &query[1..query.len() - 1])
}

#[derive(Clone, Debug)]
struct TokenMatch {
    score: f32,
    range: Range<usize>,
    letters: Option<Vec<Range<usize>>>,
}

fn within_one_edit(left: &[char], right: &[char], swap_only: bool) -> bool {
    let limit = if swap_only { 0 } else { 1 };
    if left.len().abs_diff(right.len()) > limit {
        return false;
    }
    let mut i = 0;
    while i < left.len() && i < right.len() && left[i] == right[i] {
        i += 1;
    }
    let (l, r) = (&left[i..], &right[i..]);
    let swapped = l.len() > 1 && r.len() > 1 && l[0] == r[1] && l[1] == r[0] && l[2..] == r[2..];
    if swap_only {
        return swapped;
    }
    swapped
        || l == r
        || (!l.is_empty() && !r.is_empty() && l[1..] == r[1..])
        || (!l.is_empty() && l[1..] == *r)
        || (!r.is_empty() && *l == r[1..])
}

fn match_token(
    field: &Field,
    token: &[char],
    typos: bool,
    primary: bool,
    mid_word: bool,
) -> Option<TokenMatch> {
    if token.is_empty() {
        return None;
    }
    let mut best: Option<TokenMatch> = None;
    let own_boundary = !is_alnum(token[0]);
    let mut from = 0;
    while let Some(index) = field.find_from(token, from) {
        from = index + 1;
        if field.starts_at(index) || own_boundary {
            let end = index + token.len();
            let whole = end >= field.len() || field.starts_at(end) || !is_alnum(field.text[end]);
            let mut run_end = end;
            while run_end < field.len() && is_alnum(field.text[run_end]) {
                run_end += 1;
            }
            let score = if whole {
                WHOLE_WORD
            } else {
                0.7 + 0.2 * token.len() as f32 / (run_end - index) as f32
            };
            if best.as_ref().is_none_or(|b| score > b.score) {
                best = Some(TokenMatch {
                    score,
                    range: index..end,
                    letters: None,
                });
            }
            if score == WHOLE_WORD {
                return best;
            }
        } else if mid_word && token.len() >= 4 && best.is_none() {
            best = Some(TokenMatch {
                score: MID_WORD,
                range: index..index + token.len(),
                letters: None,
            });
        }
    }
    if best.as_ref().is_none_or(|b| b.score <= MID_WORD)
        && primary
        && let Some(joined) = match_joined_words(field, token)
    {
        return Some(joined);
    }
    if best.as_ref().is_some_and(|b| b.score > MID_WORD) || !typos || token.len() < 3 {
        return best;
    }
    if token.iter().any(|c| c.is_alphabetic()) {
        for index in 0..field.len() {
            if !field.starts_at(index) {
                continue;
            }
            let first = field.text[index];
            let kept = first == token[0]
                || (token.len() > 3
                    && first == token[1]
                    && field.text.get(index + 1) == Some(&token[0]));
            if !kept {
                continue;
            }
            let mut run_end = index + 1;
            while run_end < field.len() && is_alnum(field.text[run_end]) {
                run_end += 1;
            }
            let word = &field.text[index..run_end];
            let short = token.len() <= 4;
            let lengths: Vec<usize> = if short {
                vec![token.len(), token.len() + 1]
            } else {
                vec![token.len(), token.len() - 1, token.len() + 1]
            };
            for length in lengths {
                if length > word.len() {
                    continue;
                }
                if within_one_edit(token, &word[..length], short && length == token.len()) {
                    return Some(TokenMatch {
                        score: TYPO,
                        range: index..index + length,
                        letters: None,
                    });
                }
            }
        }
    }
    best
}

/// "vscode" for "VS Code": whole words typed together.
fn match_joined_words(field: &Field, token: &[char]) -> Option<TokenMatch> {
    let first_start = (0..field.len()).find(|&i| field.starts_at(i));
    for start in 0..field.len() {
        if field.text[start] != token[0] || !field.starts_at(start) {
            continue;
        }
        let mut runs: Vec<Range<usize>> = Vec::new();
        let mut at = start;
        let mut rest: &[char] = token;
        while !rest.is_empty() && at < field.len() {
            let end = field.word_end(at);
            if rest.len() <= end - at && field.text[at..].starts_with(rest) {
                runs.push(at..at + rest.len());
                rest = &[];
            } else if rest.starts_with(&field.text[at..end]) {
                runs.push(at..end);
                rest = &rest[end - at..];
                at = end;
                while at < field.len() && !field.starts_at(at) {
                    at += 1;
                }
            } else {
                break;
            }
        }
        let Some(last) = runs.last().cloned() else {
            continue;
        };
        if !rest.is_empty() || runs.len() < 2 {
            continue;
        }
        if last.end == field.word_end(last.start)
            || last.end - last.start >= 3
            || Some(start) == first_start
            || runs[..runs.len() - 1].iter().all(|r| r.end - r.start >= 3)
        {
            return Some(TokenMatch {
                score: JOINED_WORDS,
                range: start..last.end,
                letters: Some(runs),
            });
        }
    }
    None
}

/// A field with its weight and matching rules.
#[derive(Clone, Debug)]
pub struct Weighted {
    pub field: Field,
    pub weight: f32,
    pub whole_words: bool,
    pub keyword: bool,
    pub context: bool,
    pub mid_word: bool,
}

impl Weighted {
    pub fn new(value: &str, weight: f32) -> Self {
        Self {
            field: Field::new(value),
            weight,
            whole_words: false,
            keyword: false,
            context: false,
            mid_word: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldsMatch {
    pub quality: f32,
    pub whole_words: bool,
    pub strong: bool,
    /// Matched char ranges in the first field, for highlighting.
    pub ranges: Vec<Range<usize>>,
}

fn merge_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

fn find_whole_phrase(field: &Field, words: &[Vec<char>]) -> Option<Range<usize>> {
    let first = &words[0];
    let own_boundary = !is_alnum(first[0]);
    let mut from = 0;
    while let Some(start) = field.find_from(first, from) {
        from = start + 1;
        if !field.starts_at(start) && !own_boundary {
            continue;
        }
        let mut end = Some(start + first.len());
        for word in &words[1..] {
            let Some(e) = end else { break };
            let mut next = e;
            while next < field.len() && field.text[next].is_whitespace() {
                next += 1;
            }
            end = (next > e && field.text[next..].starts_with(word)).then(|| next + word.len());
        }
        let Some(end) = end else { continue };
        if end >= field.len() || field.starts_at(end) || !is_alnum(field.text[end]) {
            return Some(start..end);
        }
    }
    None
}

fn match_exact_phrase(phrase: &str, fields: &[Weighted]) -> Option<FieldsMatch> {
    let words: Vec<Vec<char>> = phrase
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().collect())
        .collect();
    if words.is_empty() {
        return None;
    }
    let mut best = 0.0f32;
    let mut primary: Option<Range<usize>> = None;
    for (i, w) in fields.iter().enumerate() {
        if w.field.len() == 0 {
            continue;
        }
        let Some(range) = find_whole_phrase(&w.field, &words) else {
            continue;
        };
        best = best.max(w.weight);
        if i == 0 {
            primary = Some(range);
        }
    }
    if best == 0.0 {
        return None;
    }
    let whole_name = primary
        .as_ref()
        .is_some_and(|r| r.start == 0 && r.end == fields[0].field.len());
    let bonus = if whole_name {
        WHOLE_NAME - WHOLE_WORD
    } else if primary.is_some() && words.len() > 1 {
        0.25
    } else {
        0.0
    };
    Some(FieldsMatch {
        quality: best + bonus,
        whole_words: best >= WHOLE_WORD,
        strong: true,
        ranges: primary.into_iter().collect(),
    })
}

/// Every token must match some field; quality is the mean of each token's best weighted
/// match plus the phrase bonuses.
pub fn match_fields(query: &str, fields: &[Weighted], typos: bool) -> Option<FieldsMatch> {
    if query.is_empty() {
        return None;
    }
    if let Some(phrase) = exact_phrase(query) {
        return match_exact_phrase(phrase, fields);
    }
    let tokens: Vec<&str> = query.split(' ').filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return None;
    }
    let filler_optional = tokens.iter().any(|t| !is_filler(t));
    let mut ranges = Vec::new();
    let (mut total, mut counted) = (0.0f32, 0usize);
    let mut not_whole = false;
    let mut strong = true;
    for token in &tokens {
        let chars: Vec<char> = token.chars().collect();
        let mut best = 0.0f32;
        let mut best_primary: Option<TokenMatch> = None;
        let mut at_word_start = false;
        for (i, w) in fields.iter().enumerate() {
            if w.field.len() == 0 {
                continue;
            }
            if best >= w.weight && i != 0 && at_word_start {
                continue;
            }
            let Some(m) = match_token(&w.field, &chars, typos && !w.keyword, i == 0, w.mid_word)
            else {
                continue;
            };
            let floor = if w.whole_words {
                WHOLE_WORD
            } else if w.keyword {
                0.7
            } else {
                0.0
            };
            if m.score < floor {
                continue;
            }
            if m.score >= 0.7 && !w.context {
                at_word_start = true;
            }
            if m.score * w.weight > best {
                best = m.score * w.weight;
            }
            if i == 0 {
                best_primary = Some(m);
            }
        }
        if best == 0.0 {
            if filler_optional && is_filler(token) {
                continue;
            }
            return None;
        }
        strong &= at_word_start;
        if let Some(m) = best_primary {
            match m.letters {
                Some(letters) => ranges.extend(letters),
                None => ranges.push(m.range),
            }
        }
        total += best;
        counted += 1;
        if is_filler(token) {
            not_whole |= !filler_optional;
        } else {
            not_whole |= chars.len() < PICK_PREFIX_MIN_LENGTH || best < WHOLE_WORD;
        }
    }
    let mut quality = total / counted as f32;
    if let Some(primary) = fields.first() {
        let text: String = primary.field.text.iter().collect();
        if text == query {
            quality += WHOLE_NAME - WHOLE_WORD;
        } else if tokens.len() > 1 && text.contains(query) {
            quality += 0.25;
        }
    }
    Some(FieldsMatch {
        quality,
        whole_words: !not_whole,
        strong,
        ranges: merge_ranges(ranges),
    })
}

/// A scored result: what [`rank_groups`] sorts by.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    pub score: f32,
    pub quality: f32,
    /// The quality the group ranks by, when lower.
    pub group_quality: Option<f32>,
    pub whole_words: bool,
    pub strong: bool,
    pub ranges: Vec<Range<usize>>,
}

/// Commands, projects and settings: the title, then keywords that count less
/// (`scoreSearchTerms`).
pub fn score_terms(query: &str, terms: &[&str], typos: bool) -> Option<Score> {
    let words = exact_phrase(query).unwrap_or(query);
    let meaningful: Vec<&str> = words.split(' ').filter(|t| !is_filler(t)).collect();
    let meaningful = meaningful.join(" ");
    let short = if meaningful.is_empty() {
        words.chars().count()
    } else {
        meaningful.chars().count()
    } < KEYWORD_QUERY_MIN_LENGTH;
    let fields: Vec<Weighted> = terms
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if i == 0 {
                Weighted::new(t, 1.0)
            } else {
                let mut w = Weighted::new(t, if short { 0.8 } else { 0.6 });
                w.whole_words = short;
                w.keyword = true;
                w
            }
        })
        .collect();
    let m = match_fields(query, &fields, typos)?;
    Some(Score {
        score: m.quality,
        quality: m.quality,
        group_quality: m
            .ranges
            .is_empty()
            .then_some(m.quality.min(KEYWORD_QUALITY)),
        whole_words: m.whole_words,
        strong: m.strong,
        ranges: m.ranges,
    })
}

/// What a thread is searched by.
#[derive(Clone, Debug, Default)]
pub struct ThreadSubject<'a> {
    pub title: &'a str,
    pub branch: Option<&'a str>,
    pub project_title: Option<&'a str>,
    pub pull_request: Option<(u64, &'a str)>,
    /// Latest user activity, epoch ms.
    pub activity_at_ms: Option<i64>,
}

fn decay(age_ms: f64) -> f64 {
    2f64.powf(-(age_ms.max(0.0)) / HALF_LIFE_MS)
}

/// Strips the worktree branch prefix (`t3/`, `tern/`).
fn searchable_branch(branch: &str) -> &str {
    branch
        .strip_prefix("t3/")
        .or_else(|| branch.strip_prefix("t3code/"))
        .or_else(|| branch.strip_prefix("tern/"))
        .unwrap_or(branch)
}

/// Thread search (`scoreThreadSearch`): title 1 (mid-word allowed), "#PR" 1, PR title 0.5,
/// branch 0.6, project 0.25; score = quality x recency boost.
pub fn score_thread(
    query: &str,
    thread: &ThreadSubject,
    now_ms: i64,
    typos: bool,
) -> Option<Score> {
    let mut fields = vec![{
        let mut w = Weighted::new(thread.title, 1.0);
        w.mid_word = true;
        w
    }];
    if let Some((number, title)) = thread.pull_request {
        fields.push(Weighted::new(&format!("#{number}"), 1.0));
        if !title.is_empty() {
            fields.push(Weighted::new(title, 0.5));
        }
    }
    if let Some(branch) = thread.branch {
        fields.push(Weighted::new(searchable_branch(branch), 0.6));
    }
    if let Some(project) = thread.project_title {
        let mut w = Weighted::new(project, 0.25);
        w.context = true;
        fields.push(w);
    }
    let phrase = exact_phrase(query);
    let base = phrase.unwrap_or(query);
    let stripped = base
        .strip_prefix("pull request")
        .or_else(|| base.strip_prefix("pull"))
        .or_else(|| base.strip_prefix("pr"))
        .filter(|rest| {
            rest.trim_start()
                .trim_start_matches('#')
                .starts_with(|c: char| c.is_ascii_digit())
        })
        .unwrap_or(base)
        .trim_start();
    let stripped = stripped.strip_prefix('#').unwrap_or(stripped).trim_start();
    let field_query = match phrase {
        Some(_) if !stripped.is_empty() => format!("\"{stripped}\""),
        _ => stripped.to_string(),
    };
    let m = match_fields(&field_query, &fields, typos)?;
    // A multi-word match the title cannot show must at least sit in one other field.
    if m.ranges.is_empty() && field_query.contains(' ') {
        let explained = fields[1..]
            .iter()
            .any(|w| match_fields(&field_query, std::slice::from_ref(w), typos).is_some());
        if !explained {
            return None;
        }
    }
    let recency = thread
        .activity_at_ms
        .map_or(0.0, |at| decay((now_ms - at) as f64));
    let boost = (1.0 + recency as f32).min(USE_BOOST_LIMIT);
    Some(Score {
        score: m.quality * boost,
        quality: m.quality,
        group_quality: Some(m.quality.min(WHOLE_NAME - 0.01)),
        whole_words: m.whole_words,
        strong: m.strong,
        ranges: m.ranges,
    })
}

/// Match class: 3 whole name, 2 word start, 1 weak, 0 junk.
pub fn match_class(quality: f32) -> u8 {
    if quality >= WHOLE_NAME {
        3
    } else if quality >= 0.7 {
        2
    } else if quality >= 0.4 {
        1
    } else {
        0
    }
}

/// Ranks items inside groups and the groups themselves (`rankSearchGroups`). The typo
/// pass runs only when nothing matched strongly. Returns (group index, ranked item
/// indices with scores); empty groups are dropped.
pub fn rank_groups<T>(
    groups: &[Vec<T>],
    score: impl Fn(usize, &T, bool) -> Option<Score>,
) -> Vec<(usize, Vec<(usize, Score)>)> {
    let pass = |typos: bool| -> Vec<Vec<(usize, Score)>> {
        groups
            .iter()
            .enumerate()
            .map(|(g, items)| {
                items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, item)| score(g, item, typos).map(|s| (i, s)))
                    .collect()
            })
            .collect()
    };
    let mut ranked = pass(false);
    if !ranked.iter().flatten().any(|(_, s)| s.strong) {
        ranked = pass(true);
    }
    let mut out: Vec<(usize, u8, Vec<(usize, Score)>)> = ranked
        .into_iter()
        .enumerate()
        .filter(|(_, items)| !items.is_empty())
        .map(|(g, mut items)| {
            let word = |s: &Score| match_class(s.quality).min(2);
            items.sort_by(|(_, a), (_, b)| {
                word(b)
                    .cmp(&word(a))
                    .then(match_class(b.quality).cmp(&match_class(a.quality)))
                    .then(b.whole_words.cmp(&a.whole_words))
                    .then(b.score.total_cmp(&a.score))
            });
            let best = items
                .iter()
                .map(|(_, s)| match_class(s.group_quality.unwrap_or(s.quality)))
                .max()
                .unwrap_or(0);
            (g, best, items)
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.into_iter().map(|(g, _, items)| (g, items)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> String {
        fold_query(text)
    }

    #[test]
    fn folding() {
        assert_eq!(fold_query("  Fix   Sidebar "), "fix sidebar");
        assert_eq!(fold_query("\"fix sidebar\""), "\"fix sidebar\"");
        assert_eq!(fold_query("Café"), "cafe");
        assert_eq!(fold_query("it’s"), "it's");
    }

    #[test]
    fn word_starts() {
        let f = Field::new("JSONSchema camelCase t3code");
        let starts: Vec<usize> = (0..f.len()).filter(|&i| f.starts_at(i)).collect();
        // J, S(chema), c(amel), C(ase), t, 3, c(ode)
        assert_eq!(starts, vec![0, 4, 11, 16, 21, 22, 23]);
    }

    #[test]
    fn token_scores() {
        let title = [Weighted::new("Change theme", 1.0)];
        let whole = match_fields(&q("theme"), &title, false).unwrap();
        assert_eq!(whole.quality, 1.0);
        assert!(whole.strong && whole.whole_words);
        let prefix = match_fields(&q("the"), &title, false).unwrap();
        assert!((prefix.quality - (0.7 + 0.2 * 3.0 / 5.0)).abs() < 1e-5);
        let exact = match_fields(&q("change theme"), &title, false).unwrap();
        assert_eq!(exact.quality, 1.5);
        assert_eq!(exact.ranges, vec![0..12]);
        assert!(
            match_fields(&q("heme"), &title, false).is_none(),
            "no mid-word in commands"
        );
        assert!(match_fields(&q("xyz"), &title, false).is_none());
    }

    #[test]
    fn mid_word_typos_and_joined() {
        let mut title = Weighted::new("Stream markdown in the chat", 1.0);
        title.mid_word = true;
        let m = match_fields(&q("down"), std::slice::from_ref(&title), false).unwrap();
        assert!((m.quality - MID_WORD).abs() < 1e-5);
        let code = [Weighted::new("Open in VS Code", 1.0)];
        let m = match_fields(&q("vscode"), &code, false).unwrap();
        assert!((m.quality - JOINED_WORDS).abs() < 1e-5);
        let typo = [Weighted::new("GitHub repository", 1.0)];
        assert!(match_fields(&q("githbu"), &typo, false).is_none());
        let m = match_fields(&q("githbu"), &typo, true).unwrap();
        assert!((m.quality - TYPO).abs() < 1e-5);
        // Short tokens only forgive swaps or one inserted letter.
        assert!(match_fields(&q("cron"), &[Weighted::new("console", 1.0)], true).is_none());
    }

    #[test]
    fn filler_words_are_optional() {
        let title = [Weighted::new("New thread in demo-notes", 1.0)];
        assert!(match_fields(&q("new thread the"), &title, false).is_some());
        assert!(match_fields(&q("new thread zebra"), &title, false).is_none());
    }

    #[test]
    fn keyword_terms_rank_below_names() {
        let named = score_terms(&q("theme"), &["Change theme", "appearance"], false).unwrap();
        assert!(named.group_quality.is_none());
        let keyword =
            score_terms(&q("appearance"), &["Open settings", "appearance"], false).unwrap();
        assert_eq!(keyword.group_quality, Some(KEYWORD_QUALITY));
        // Short queries need whole keywords.
        assert!(score_terms(&q("app"), &["Open settings", "appearance"], false).is_none());
    }

    #[test]
    fn thread_scoring() {
        let now = 1_000_000_000_000;
        let t = ThreadSubject {
            title: "List the files in this repo and summarize them",
            branch: Some("t3/sidebar-nav"),
            project_title: Some("demo-notes"),
            pull_request: Some((42, "Fix the sidebar")),
            activity_at_ms: Some(now),
        };
        let m = score_thread(&q("list"), &t, now, false).unwrap();
        assert_eq!(m.quality, 1.0);
        assert!((m.score - 2.0).abs() < 1e-5, "fresh threads double");
        assert_eq!(m.ranges, vec![0..4]);
        assert!(score_thread(&q("#42"), &t, now, false).is_some());
        assert!(score_thread(&q("pr 42"), &t, now, false).is_some());
        assert!(score_thread(&q("sidebar"), &t, now, false).is_some());
        let project = score_thread(&q("demo"), &t, now, false).unwrap();
        assert!(project.quality < 0.5);
        assert!(score_thread(&q("zebra"), &t, now, false).is_none());
    }

    #[test]
    fn ranking_orders_groups_by_best_class() {
        let groups = vec![
            vec!["Open settings"],
            vec!["Change theme", "Toggle theme editor"],
        ];
        let ranked = rank_groups(&groups, |_, title, typos| {
            score_terms(&q("theme"), &[title], typos)
        });
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].0, 1);
        assert_eq!(ranked[0].1.len(), 2);
        assert_eq!(
            ranked[0].1[0].0, 0,
            "shorter whole-word title first by score tie"
        );
    }
}
