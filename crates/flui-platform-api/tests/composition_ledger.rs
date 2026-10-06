//! `CompositionLedger` against a reference built differently: random edit,
//! mark and session sequences, and the named counterexamples the reference
//! found in the ledger's earlier range-based form.
//!
//! The reference keeps one token per character (and a mark per removal or
//! empty composition) and computes each composition's origin as a string
//! when the composition forms, from the origins of the regions it takes in;
//! the ledger keeps what every piece stands for and derives the origin when
//! asked. A narrowing is computed by cutting the leaving text's share off
//! the front and back of the origin string, which the reference checks is
//! there to cut. ADR-0090 amendment item 1 states the rules both follow.

use std::ops::Range;

use flui_platform_api::text_store::{CompositionLedger, committed_text};
use proptest::prelude::*;

// ----------------------------------------------------------------------------
// The reference
// ----------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Token {
    /// The character shown; `None` for a mark that shows nothing.
    ch: Option<char>,
    /// Text the session inserted, which stands for nothing.
    fresh: bool,
    group: Option<usize>,
    /// For a mark: the removal it records, an index into `Reference::removed`.
    removal: Option<usize>,
    /// The replacement a character or a removal belongs to.
    replacement: Option<usize>,
}

impl Token {
    fn shown(ch: char, fresh: bool) -> Self {
        Self {
            ch: Some(ch),
            fresh,
            group: None,
            removal: None,
            replacement: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Reference {
    tokens: Vec<Token>,
    /// Each group's origin, fixed when the group formed or narrowed.
    origins: Vec<String>,
    /// The text each removal removed.
    removed: Vec<String>,
    /// The replacement each replacement was joined into (its own index when
    /// not joined).
    joined: Vec<usize>,
    live: Vec<usize>,
    composition: Option<usize>,
}

impl Reference {
    fn open(text: &str, composition: Option<(Range<usize>, String)>) -> Self {
        let mut reference = Self::default();
        let Some((range, origin)) = composition else {
            reference.tokens = text.chars().map(|ch| Token::shown(ch, false)).collect();
            return reference;
        };
        let group = reference.group(origin.clone());
        let shown = &text[range.clone()];
        let itself = !origin.is_empty() && origin == shown;
        for (at, ch) in text.char_indices() {
            if at == range.start {
                reference.open_mark(group, shown, &origin);
            }
            let mut token = Token::shown(ch, range.contains(&at) && !itself);
            if range.contains(&at) {
                token.group = Some(group);
                token.replacement = reference.tokens.last().and_then(|mark| mark.replacement);
            }
            reference.tokens.push(token);
        }
        if range.start == text.len() {
            reference.open_mark(group, shown, &origin);
        }
        reference.live.push(group);
        reference.composition = Some(group);
        reference
    }

    /// The mark a reopened composition starts with, when it needs one.
    fn open_mark(&mut self, group: usize, shown: &str, origin: &str) {
        if !origin.is_empty() && origin != shown {
            let replacement = self.joined.len();
            self.joined.push(replacement);
            self.removed.push(origin.to_owned());
            self.tokens.push(Token {
                ch: None,
                fresh: false,
                group: Some(group),
                removal: Some(self.removed.len() - 1),
                replacement: Some(replacement),
            });
        } else if shown.is_empty() {
            self.tokens.push(Token {
                ch: None,
                fresh: false,
                group: Some(group),
                removal: None,
                replacement: None,
            });
        }
    }

    fn group(&mut self, origin: String) -> usize {
        self.origins.push(origin);
        self.origins.len() - 1
    }

    fn text(&self) -> String {
        self.tokens.iter().filter_map(|token| token.ch).collect()
    }

    fn at(&self) -> Vec<usize> {
        let mut at = 0;
        self.tokens
            .iter()
            .map(|token| {
                let here = at;
                at += token.ch.map_or(0, char::len_utf8);
                here
            })
            .collect()
    }

    fn extent(&self, group: usize) -> Range<usize> {
        let at = self.at();
        let members: Vec<usize> = (0..self.tokens.len())
            .filter(|&index| self.tokens[index].group == Some(group))
            .collect();
        let start = at[members[0]];
        let len: usize = members
            .iter()
            .filter_map(|&index| self.tokens[index].ch)
            .map(char::len_utf8)
            .sum();
        start..start + len
    }

    fn origin(&self) -> Option<String> {
        self.composition.map(|group| self.origins[group].clone())
    }

    /// What `tokens` stand for: the origin of each group in `whole`, once,
    /// and every other committed character.
    fn view(&self, tokens: &[Token], whole: &[usize]) -> String {
        let mut out = String::new();
        let mut seen = Vec::new();
        for token in tokens {
            match token.group {
                Some(group) if whole.contains(&group) => {
                    if !seen.contains(&group) {
                        seen.push(group);
                        out.push_str(&self.origins[group]);
                    }
                }
                _ => {
                    if let Some(ch) = token.ch
                        && !token.fresh
                    {
                        out.push(ch);
                    }
                }
            }
        }
        out
    }

    /// What one token stands for on its own.
    fn share(&self, token: &Token) -> String {
        match (token.ch, token.removal) {
            (Some(ch), _) if !token.fresh => ch.to_string(),
            (None, Some(removal)) => self.removed[removal].clone(),
            _ => String::new(),
        }
    }

    fn root(&self, mut replacement: usize) -> usize {
        while self.joined[replacement] != replacement {
            replacement = self.joined[replacement];
        }
        replacement
    }

    fn dissolve(&mut self, group: usize) {
        self.tokens
            .retain(|token| !(token.group == Some(group) && token.ch.is_none()));
        for token in &mut self.tokens {
            if token.group == Some(group) {
                token.group = None;
                token.fresh = false;
                token.replacement = None;
            }
        }
        self.live.retain(|&live| live != group);
        if self.composition == Some(group) {
            self.composition = None;
        }
    }

    fn replace(&mut self, edit: Range<usize>, inserted: &str) {
        let hit: Vec<usize> = self
            .live
            .iter()
            .copied()
            .filter(|&group| {
                let extent = self.extent(group);
                !(edit.end <= extent.start || edit.start >= extent.end)
            })
            .collect();
        let at = self.at();
        let fresh: Vec<Token> = inserted.chars().map(|ch| Token::shown(ch, true)).collect();
        if hit.is_empty() {
            let mut out = Vec::new();
            let mut pending = Some(fresh);
            for (index, token) in self.tokens.iter().enumerate() {
                // A store's composition moves with an insertion at its start
                // and stays put with one at its end.
                let before = match token.ch {
                    Some(_) => at[index] >= edit.start,
                    None => token
                        .group
                        .is_some_and(|group| self.extent(group).start >= edit.end),
                };
                if before && let Some(fresh) = pending.take() {
                    out.extend(fresh);
                }
                if token.ch.is_some() && edit.contains(&at[index]) {
                    continue;
                }
                out.push(token.clone());
            }
            out.extend(pending.into_iter().flatten());
            self.tokens = out;
            return;
        }
        let mut span = edit.clone();
        for &group in &hit {
            let extent = self.extent(group);
            span.start = span.start.min(extent.start);
            span.end = span.end.max(extent.end);
        }
        let inside = |index: usize, token: &Token| match token.ch {
            Some(_) => span.contains(&at[index]),
            None => {
                token.group.is_some_and(|group| hit.contains(&group))
                    || (at[index] > span.start && at[index] < span.end)
            }
        };
        let gone = |index: usize, token: &Token| match token.ch {
            Some(_) => edit.contains(&at[index]),
            None => at[index] > edit.start && at[index] < edit.end,
        };
        let members: Vec<Token> = self
            .tokens
            .iter()
            .enumerate()
            .filter(|&(index, token)| inside(index, token))
            .map(|(_, token)| token.clone())
            .collect();
        let origin = self.view(&members, &hit);
        let group = self.group(origin);
        let replacement = self.joined.len();
        self.joined.push(replacement);
        let mut removed_text = String::new();
        for (index, token) in self.tokens.iter().enumerate() {
            if inside(index, token) && gone(index, token) {
                removed_text.push_str(&self.share(token));
            }
        }
        let gone_replacements: Vec<usize> = self
            .tokens
            .iter()
            .enumerate()
            .filter(|&(index, token)| inside(index, token) && gone(index, token))
            .filter_map(|(_, token)| token.replacement)
            .collect();
        for other in gone_replacements {
            let root = self.root(other);
            if root != replacement {
                self.joined[root] = replacement;
            }
        }
        self.removed.push(removed_text);
        let removal = self.removed.len() - 1;
        let mut insertion = vec![Token {
            ch: None,
            fresh: false,
            group: Some(group),
            removal: Some(removal),
            replacement: Some(replacement),
        }];
        insertion.extend(fresh.into_iter().map(|token| Token {
            group: Some(group),
            replacement: Some(replacement),
            ..token
        }));
        let mut out = Vec::new();
        let mut placed = false;
        let mut last_member = None;
        for (index, token) in self.tokens.iter().enumerate() {
            if !inside(index, token) {
                out.push(token.clone());
                continue;
            }
            if gone(index, token) {
                last_member = Some(out.len());
                continue;
            }
            let after_edit = match token.ch {
                Some(_) => at[index] >= edit.start,
                None => at[index] >= edit.end,
            };
            if !placed && after_edit {
                out.extend(insertion.iter().cloned());
                placed = true;
            }
            out.push(Token {
                group: Some(group),
                ..token.clone()
            });
            last_member = Some(out.len());
        }
        if !placed {
            let end = last_member.unwrap_or(out.len());
            out.splice(end..end, insertion);
        }
        self.tokens = out;
        self.live.retain(|live| !hit.contains(live));
        if self
            .composition
            .is_some_and(|composition| hit.contains(&composition))
        {
            self.composition = None;
        }
        self.live.push(group);
    }

    fn mark(&mut self, range: Option<Range<usize>>) {
        let Some(range) = range else {
            for group in self.live.clone() {
                self.dissolve(group);
            }
            return;
        };
        let touched: Vec<usize> = self
            .live
            .iter()
            .copied()
            .filter(|&group| {
                let extent = self.extent(group);
                if extent.is_empty() || range.is_empty() {
                    extent.start <= range.end && extent.end >= range.start
                } else {
                    extent.start < range.end && extent.end > range.start
                }
            })
            .collect();
        if let Some(composition) = self.composition
            && !touched.contains(&composition)
        {
            self.dissolve(composition);
        }
        for &group in &touched {
            let extent = self.extent(group);
            if extent.start < range.start || extent.end > range.end {
                self.narrow(group, &range);
            }
        }
        // A mark that stays with the composition sits where the composition
        // does: those left before the range go to its start, those after it
        // to its end. Rebuilt from the characters: every character in order,
        // with the composition's marks placed among them by where they go.
        let at = self.at();
        let stays = |token: &Token| {
            token.ch.is_none() && token.group.is_some_and(|group| touched.contains(&group))
        };
        let mut to_start = Vec::new();
        let mut to_end = Vec::new();
        let mut rest = Vec::new();
        for (token, at) in self.tokens.drain(..).zip(at) {
            if stays(&token) && at < range.start {
                to_start.push(token);
            } else if stays(&token) && at > range.end {
                to_end.push(token);
            } else {
                rest.push((token, at));
            }
        }
        let mut tokens = Vec::new();
        for (token, at) in rest {
            if at >= range.start && !to_start.is_empty() {
                tokens.append(&mut to_start);
            }
            if (at > range.end || (at == range.end && token.ch.is_some())) && !to_end.is_empty() {
                tokens.append(&mut to_end);
            }
            tokens.push(token);
        }
        tokens.append(&mut to_start);
        tokens.append(&mut to_end);
        self.tokens = tokens;
        let at = self.at();
        let inside = |index: usize, token: &Token| match token.ch {
            Some(_) => range.contains(&at[index]),
            None => token.group.is_some_and(|group| touched.contains(&group)),
        };
        let members: Vec<Token> = self
            .tokens
            .iter()
            .enumerate()
            .filter(|&(index, token)| inside(index, token))
            .map(|(_, token)| token.clone())
            .collect();
        let origin = self.view(&members, &touched);
        let group = self.group(origin);
        let mut any = false;
        for index in 0..self.tokens.len() {
            if inside(index, &self.tokens[index]) {
                self.tokens[index].group = Some(group);
                any = true;
            }
        }
        if !any {
            let index = (0..self.tokens.len())
                .find(|&index| at[index] >= range.start)
                .unwrap_or(self.tokens.len());
            self.tokens.insert(
                index,
                Token {
                    ch: None,
                    fresh: false,
                    group: Some(group),
                    removal: None,
                    replacement: None,
                },
            );
        }
        self.live.retain(|live| !touched.contains(live));
        self.live.push(group);
        self.composition = Some(group);
    }

    /// The part of `group` outside `range` commits as shown; the origin left
    /// is the group's origin with the leaving parts' share cut off its ends,
    /// unless a replacement that removed text is split, when the group left
    /// stands for its own visible text.
    ///
    /// Which tokens leave is decided per replacement, from the replacement's
    /// own characters: a replacement whose characters all lie outside the
    /// range leaves whole, removal included, and one with characters on both
    /// sides is split. Only a removal whose replacement has no characters (a
    /// deletion), or a group's marker, is placed by where it sits: it stays
    /// strictly inside the range. Which leaving tokens come before the range
    /// and which after is read from their order around the staying tokens.
    fn narrow(&mut self, group: usize, range: &Range<usize>) {
        let at = self.at();
        let members: Vec<usize> = (0..self.tokens.len())
            .filter(|&index| self.tokens[index].group == Some(group))
            .collect();
        let char_inside = |index: usize| range.contains(&at[index]);
        // Each replacement in the group, and where its characters are.
        let mut replacements: Vec<(usize, Vec<usize>)> = Vec::new();
        for &index in &members {
            if let Some(replacement) = self.tokens[index].replacement {
                let root = self.root(replacement);
                match replacements.iter_mut().find(|(seen, _)| *seen == root) {
                    Some((_, tokens)) => tokens.push(index),
                    None => replacements.push((root, vec![index])),
                }
            }
        }
        let characters = |tokens: &[usize]| -> (bool, bool) {
            let shown: Vec<usize> = tokens
                .iter()
                .copied()
                .filter(|&index| self.tokens[index].ch.is_some())
                .collect();
            (
                shown.iter().any(|&index| char_inside(index)),
                shown.iter().any(|&index| !char_inside(index)),
            )
        };
        let mut leaves = vec![false; self.tokens.len()];
        let mut split = false;
        for (_, tokens) in &replacements {
            let (inside, outside) = characters(tokens);
            let removed_text = tokens.iter().any(|&index| {
                self.tokens[index]
                    .removal
                    .is_some_and(|removal| !self.removed[removal].is_empty())
            });
            split |= inside && outside && removed_text;
            for &index in tokens {
                leaves[index] = if self.tokens[index].ch.is_some() {
                    !char_inside(index)
                } else if inside || outside {
                    outside
                } else {
                    !(at[index] > range.start && at[index] < range.end)
                };
            }
        }
        for &index in &members {
            let token = &self.tokens[index];
            if token.replacement.is_none() {
                leaves[index] = match token.ch {
                    Some(_) => !char_inside(index),
                    None => !(at[index] > range.start && at[index] < range.end),
                };
            }
        }
        let staying: Vec<usize> = members.iter().copied().filter(|&i| !leaves[i]).collect();
        let origin = if split {
            staying
                .iter()
                .filter_map(|&index| self.tokens[index].ch)
                .collect()
        } else {
            let (first, last) = (staying.first().copied(), staying.last().copied());
            let mut before = String::new();
            let mut after = String::new();
            for &index in members.iter().filter(|&&i| leaves[i]) {
                let share = self.share(&self.tokens[index]);
                // A leaving token that stands for nothing takes no share.
                if share.is_empty() {
                    continue;
                }
                match (first, last) {
                    (Some(first), _) if index < first => before.push_str(&share),
                    (_, Some(last)) if index > last => after.push_str(&share),
                    (None, None) => before.push_str(&share),
                    _ => panic!(
                        "reference: a leaving token {index} lies between staying tokens of group {group}"
                    ),
                }
            }
            let whole = &self.origins[group];
            let rest = whole
                .strip_prefix(before.as_str())
                .and_then(|rest| rest.strip_suffix(after.as_str()));
            assert!(
                rest.is_some(),
                "reference: the leaving parts {before:?} / {after:?} are not the ends of the origin {whole:?}"
            );
            rest.expect("asserted").to_owned()
        };
        self.origins[group] = origin;
        let mut out = Vec::new();
        for (index, token) in self.tokens.iter().enumerate() {
            if token.group != Some(group) {
                out.push(token.clone());
                continue;
            }
            match (token.ch, leaves[index], split) {
                (Some(ch), true, _) => out.push(Token::shown(ch, false)),
                // A split group stands for its visible text: its marks go.
                (None, true, _) | (None, false, true) => {}
                (Some(ch), false, true) => out.push(Token {
                    group: Some(group),
                    ..Token::shown(ch, false)
                }),
                (_, false, false) => out.push(token.clone()),
            }
        }
        self.tokens = out;
    }
}

// ----------------------------------------------------------------------------
// Driving both
// ----------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Op {
    /// Replace a byte range with text.
    Replace(Range<usize>, &'static str),
    /// Mark a byte range as the composition, or end it.
    Mark(Option<Range<usize>>),
    /// End the session: a new one opens on the store's text and composition.
    Reopen,
}

/// The composing range a store keeps through an edit (ADR-0090
/// "Divergences").
fn shifted(
    composition: Option<Range<usize>>,
    edit: &Range<usize>,
    inserted: usize,
) -> Option<Range<usize>> {
    let composition = composition?;
    if edit.end <= composition.start {
        let shift = |at: usize| at + inserted - edit.len();
        Some(shift(composition.start)..shift(composition.end))
    } else if edit.start >= composition.end {
        Some(composition)
    } else {
        None
    }
}

/// Run `ops` on `text` with `composition`, checking the ledger against the
/// reference after each; the first difference.
fn check(text: &str, composition: Option<(Range<usize>, &str)>, ops: &[Op]) -> Result<(), String> {
    let composition = composition.map(|(range, origin)| (range, origin.to_owned()));
    let mut text = text.to_owned();
    let mut composing = composition.as_ref().map(|(range, _)| range.clone());
    let mut ledger = CompositionLedger::open(&text, composition.clone());
    let mut reference = Reference::open(&text, composition);
    for (step, op) in ops.iter().enumerate() {
        match op {
            Op::Replace(edit, inserted) => {
                ledger.replace(&text, edit.clone(), inserted);
                reference.replace(edit.clone(), inserted);
                composing = shifted(composing, edit, inserted.len());
                text.replace_range(edit.clone(), inserted);
            }
            Op::Mark(range) => {
                ledger.set_composition(range.clone());
                reference.mark(range.clone());
                composing.clone_from(range);
            }
            Op::Reopen => {
                let ledger_composition = composing
                    .clone()
                    .map(|range| (range, ledger.origin(&text).unwrap_or_default()));
                let reference_composition = composing
                    .clone()
                    .map(|range| (range, reference.origin().unwrap_or_default()));
                ledger = CompositionLedger::open(&text, ledger_composition);
                reference = Reference::open(&text, reference_composition);
            }
        }
        if reference.text() != text {
            return Err(format!(
                "after step {step} {op:?}: text {text:?}, reference {:?}",
                reference.text()
            ));
        }
        // What the store's committed text is: its composing range replaced by the
        // origin the reference computed.
        let reference_origin = reference.origin();
        let want = committed_text(&text, composing.clone().zip(reference_origin.as_deref()));
        let got = ledger.committed(&text);
        if got != want || ledger.origin(&text) != reference.origin() {
            return Err(format!(
                "after step {step} {op:?}: committed {got:?} (origin {:?}), reference {want:?} (origin {:?})",
                ledger.origin(&text),
                reference.origin()
            ));
        }
    }
    Ok(())
}

/// The cases a property run found against the range-based ledger, and the
/// narrowing rule (ADR-0090 amendment item 1), each a sequence and the
/// committed text it ends with.
#[allow(clippy::type_complexity)]
fn named_cases() -> Vec<(
    &'static str,
    &'static str,
    Option<(Range<usize>, &'static str)>,
    Vec<Op>,
    &'static str,
)> {
    use Op::{Mark, Replace};
    vec![
        (
            "a replace that clears a composition keeps an earlier cleared one",
            "abcdefgh",
            None,
            vec![
                Mark(Some(0..2)),
                Replace(0..2, "X"),
                Mark(Some(3..5)),
                Replace(3..5, "Y"),
                Mark(Some(0..1)),
            ],
            "abcdYgh",
        ),
        (
            "an empty region where another removal starts keeps its origin",
            "abcd",
            Some((3..4, "pq")),
            vec![
                Replace(2..4, ""),
                Mark(Some(0..1)),
                Replace(0..1, ""),
                Mark(Some(0..1)),
            ],
            "abcpq",
        ),
        (
            "an insertion does not merge across an empty composition",
            "a",
            Some((0..0, "pq")),
            vec![Replace(0..0, "B"), Replace(1..2, "CD"), Mark(Some(0..3))],
            "pq",
        ),
        (
            "a partial mark keeps the rest of an insertion new",
            "",
            None,
            vec![Replace(0..0, "abcd"), Mark(Some(0..2)), Mark(Some(2..4))],
            "ab",
        ),
        (
            "an empty mark inside an insertion keeps both sides new",
            "",
            None,
            vec![Replace(0..0, "abcd"), Mark(Some(2..2)), Mark(Some(2..4))],
            "ab",
        ),
        (
            "a partial edit of a cleared composition keeps the rest of an insertion new",
            "",
            None,
            vec![
                Replace(0..0, "abcd"),
                Mark(Some(0..2)),
                Replace(1..2, "X"),
                Mark(Some(2..4)),
            ],
            "aX",
        ),
        (
            "narrowing an unchanged reconversion commits the rest as itself",
            "abcdef",
            None,
            vec![Mark(Some(0..6)), Mark(Some(0..3))],
            "abcdef",
        ),
        (
            "narrowing a conversion of committed text commits what is shown",
            "abcdef",
            None,
            vec![
                Mark(Some(0..6)),
                Replace(0..6, "ABCDEF"),
                Mark(Some(0..6)),
                Mark(Some(0..3)),
            ],
            "ABCDEF",
        ),
        (
            "narrowing new preedit commits the part left behind",
            "",
            None,
            vec![Replace(0..0, "abcdef"), Mark(Some(0..6)), Mark(Some(0..3))],
            "def",
        ),
        (
            "narrowing a reopened conversion commits what is shown",
            "ABCDEF",
            Some((0..6, "abcdef")),
            vec![Mark(Some(0..3))],
            "ABCDEF",
        ),
        (
            "a removal leaves with its replacement's text",
            "abcd",
            Some((0..3, "")),
            vec![Replace(2..4, "B"), Mark(Some(0..2))],
            "B",
        ),
        (
            "a deletion at the end of the narrowed range commits",
            "abcdefghi",
            None,
            vec![Mark(Some(0..9)), Replace(3..6, ""), Mark(Some(0..3))],
            "abcghi",
        ),
    ]
}

pub(crate) fn the_named_cases_hold() {
    let mut failures = Vec::new();
    for (name, text, composition, ops, committed) in named_cases() {
        let mut ledger = CompositionLedger::open(
            text,
            composition
                .clone()
                .map(|(range, origin)| (range, origin.to_owned())),
        );
        let mut current = text.to_owned();
        for op in &ops {
            match op {
                Op::Replace(edit, inserted) => {
                    ledger.replace(&current, edit.clone(), inserted);
                    current.replace_range(edit.clone(), inserted);
                }
                Op::Mark(range) => ledger.set_composition(range.clone()),
                Op::Reopen => unreachable!("no named case reopens"),
            }
        }
        if ledger.committed(&current) != committed {
            failures.push(format!(
                "{name}: committed {:?}, want {committed:?}",
                ledger.committed(&current)
            ));
        }
        if let Err(difference) = check(text, composition, &ops) {
            failures.push(format!("{name}: {difference}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ----------------------------------------------------------------------------
// Random sequences
// ----------------------------------------------------------------------------

/// Characters of one, two and three UTF-8 bytes.
const INSERTS: &[&str] = &["", "x", "yz", "é", "東京", "aé東"];

/// How a generated mark relates to the composition, or to a random range
/// when there is none.
#[derive(Clone, Copy, Debug)]
enum MarkKind {
    Overlapping,
    Adjacent,
    Contained,
    Containing,
    Disjoint,
    Empty,
    End,
}

#[derive(Clone, Debug)]
enum Step {
    Replace(u8, u8, usize),
    Mark(MarkKind, u8, u8),
    Reopen,
}

fn step() -> impl Strategy<Value = Step> {
    let kind = prop_oneof![
        Just(MarkKind::Overlapping),
        Just(MarkKind::Adjacent),
        Just(MarkKind::Contained),
        Just(MarkKind::Containing),
        Just(MarkKind::Disjoint),
        Just(MarkKind::Empty),
        Just(MarkKind::End),
    ];
    prop_oneof![
        5 => (any::<u8>(), any::<u8>(), 0..INSERTS.len()).prop_map(|(a, b, i)| Step::Replace(a, b, i)),
        5 => (kind, any::<u8>(), any::<u8>()).prop_map(|(k, a, b)| Step::Mark(k, a, b)),
        1 => Just(Step::Reopen),
    ]
}

/// The char boundaries of `text`, end included.
fn boundaries(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(at, _)| at)
        .chain([text.len()])
        .collect()
}

fn pick(points: &[usize], seed: u8) -> usize {
    points[usize::from(seed) % points.len()]
}

/// An ordered range of boundaries from two seeds.
fn range_of(points: &[usize], a: u8, b: u8) -> Range<usize> {
    let (a, b) = (pick(points, a), pick(points, b));
    a.min(b)..a.max(b)
}

/// Resolve generated steps against the text and composing range they meet.
fn resolve(text: &str, composition: Option<Range<usize>>, steps: &[Step]) -> Vec<Op> {
    let mut text = text.to_owned();
    let mut composing = composition;
    let mut ops = Vec::new();
    for step in steps {
        let points = boundaries(&text);
        let op = match *step {
            Step::Replace(a, b, i) => Op::Replace(range_of(&points, a, b), INSERTS[i]),
            Step::Reopen => Op::Reopen,
            Step::Mark(MarkKind::End, ..) => Op::Mark(None),
            Step::Mark(kind, a, b) => {
                let range = match composing.clone() {
                    None => range_of(&points, a, b),
                    Some(at) => {
                        let before: Vec<usize> =
                            points.iter().copied().filter(|&p| p <= at.start).collect();
                        let after: Vec<usize> =
                            points.iter().copied().filter(|&p| p >= at.end).collect();
                        let within: Vec<usize> = points
                            .iter()
                            .copied()
                            .filter(|&p| p >= at.start && p <= at.end)
                            .collect();
                        match kind {
                            MarkKind::Overlapping => pick(&within, a)..pick(&after, b),
                            MarkKind::Adjacent => {
                                if a % 2 == 0 {
                                    at.end..pick(&after, b)
                                } else {
                                    pick(&before, b)..at.start
                                }
                            }
                            MarkKind::Contained => range_of(&within, a, b),
                            MarkKind::Containing => pick(&before, a)..pick(&after, b),
                            MarkKind::Disjoint => range_of(&after, a, b),
                            MarkKind::Empty => {
                                let at = pick(&points, a);
                                at..at
                            }
                            MarkKind::End => unreachable!("handled above"),
                        }
                    }
                };
                Op::Mark(Some(range))
            }
        };
        match &op {
            Op::Replace(edit, inserted) => {
                composing = shifted(composing, edit, inserted.len());
                text.replace_range(edit.clone(), inserted);
            }
            Op::Mark(range) => composing.clone_from(range),
            Op::Reopen => {}
        }
        ops.push(op);
    }
    ops
}

fn start() -> impl Strategy<Value = (String, Option<(u8, u8)>, usize)> {
    (
        prop::sample::select(vec!["", "ab", "abcd", "aé東d", "東京", "abcdef"]),
        prop::option::of((any::<u8>(), any::<u8>())),
        0..INSERTS.len(),
    )
        .prop_map(|(text, composition, origin)| (text.to_owned(), composition, origin))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 3000,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn the_ledger_follows_the_reference(
        (text, composition, origin) in start(),
        steps in prop::collection::vec(step(), 1..12),
    ) {
        let points = boundaries(&text);
        let composition = composition.map(|(a, b)| (range_of(&points, a, b), INSERTS[origin]));
        let ops = resolve(&text, composition.clone().map(|(range, _)| range), &steps);
        // A reference that cannot follow a sequence panics; report it with the
        // sequence like any other difference.
        let outcome = std::panic::catch_unwind(|| check(&text, composition.clone(), &ops))
            .unwrap_or_else(|payload| {
                Err(format!("panicked: {:?}", payload.downcast_ref::<String>()))
            });
        prop_assert!(outcome.is_ok(), "{:?} on {:?} with {:?}: {:?}", ops, text, composition, outcome);
    }
}

#[test]
fn composition_ledger_named_cases() {
    the_named_cases_hold();
}
