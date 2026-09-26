//! The Rust tokens the marker scan reads: comments, string literals and
//! identifiers, with their byte ranges. Everything else is skipped.
//!
//! A hand-written walk rather than `rustc_lexer`: the published copy of that
//! lexer (`ra-ap-rustc_lexer`) asserts at compile time that its
//! `unicode-properties` and the workspace's `unicode-ident` carry the same
//! Unicode version, so every `unicode-ident` release breaks the build until a
//! new lexer is published. The walk only has to tell these tokens apart from
//! code, which is a small part of the grammar: nested block comments, the raw
//! and prefixed string forms, and a character literal apart from a lifetime.
//! A source that does not lex (an unterminated literal) yields its tokens up
//! to that point, and the rest as one string.

use std::ops::Range;

/// Whether a doc comment documents the next item or the enclosing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DocStyle {
    /// `///`, `/**`.
    Outer,
    /// `//!`, `/*!`.
    Inner,
}

/// One token the scan reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Token {
    LineComment(Option<DocStyle>),
    BlockComment(Option<DocStyle>),
    /// A string literal of any form, quotes and prefix included.
    Str,
    /// An identifier; a raw identifier's range leaves out its `r#`.
    Ident,
}

/// The comments, string literals and identifiers of `src`, in order.
pub(super) fn tokens(src: &str) -> Vec<(Token, Range<usize>)> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let rest = &bytes[at..];
        if rest.starts_with(b"//") {
            let end = src[at..].find('\n').map_or(src.len(), |n| at + n);
            let doc = match rest.get(2) {
                Some(b'/') if rest.get(3) != Some(&b'/') => Some(DocStyle::Outer),
                Some(b'!') => Some(DocStyle::Inner),
                _ => None,
            };
            out.push((Token::LineComment(doc), start..end));
            at = end;
        } else if rest.starts_with(b"/*") {
            let doc = match rest.get(2) {
                // `/**/` is empty, `/***` a plain comment
                Some(b'*') if !matches!(rest.get(3), Some(b'*' | b'/')) => Some(DocStyle::Outer),
                Some(b'!') => Some(DocStyle::Inner),
                _ => None,
            };
            at = block_comment_end(bytes, at);
            out.push((Token::BlockComment(doc), start..at));
        } else if rest[0] == b'"' {
            at = string_end(bytes, at + 1);
            out.push((Token::Str, start..at));
        } else if rest[0] == b'\'' {
            at = quote_end(src, at);
        } else if rest[0].is_ascii_digit() {
            at += 1;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
        } else if let Some(word_end) = ident_end(src, at) {
            let word = &src[at..word_end];
            let next = bytes.get(word_end).copied();
            let raw_ident = (word == "r" && next == Some(b'#'))
                .then(|| ident_end(src, word_end + 1))
                .flatten();
            match (word, next) {
                ("b" | "c", Some(b'"')) => {
                    at = string_end(bytes, word_end + 1);
                    out.push((Token::Str, start..at));
                }
                ("b", Some(b'\'')) => at = quote_end(src, word_end),
                ("r" | "br" | "cr", Some(b'"' | b'#')) if raw_string_opens(bytes, word_end) => {
                    at = raw_string_end(bytes, word_end);
                    out.push((Token::Str, start..at));
                }
                _ => {
                    let range = raw_ident.map_or(start..word_end, |end| word_end + 1..end);
                    at = range.end;
                    out.push((Token::Ident, range));
                }
            }
        } else {
            at += src[at..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// The end of the identifier starting at `at`, if one starts there.
fn ident_end(src: &str, at: usize) -> Option<usize> {
    let mut chars = src[at..].char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_alphabetic()) {
        return None;
    }
    let len = chars
        .find(|&(_, c)| !(c == '_' || c.is_alphanumeric()))
        .map_or(src.len() - at, |(n, _)| n);
    Some(at + len)
}

/// The end of a (nested) block comment opening at `at`.
fn block_comment_end(bytes: &[u8], mut at: usize) -> usize {
    let mut depth = 0usize;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"/*") {
            depth += 1;
            at += 2;
        } else if bytes[at..].starts_with(b"*/") {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += 1;
        }
    }
    bytes.len()
}

/// The end of a string whose contents start at `at` (after the opening quote).
fn string_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// Whether `#`s then a `"` start at `at`.
fn raw_string_opens(bytes: &[u8], at: usize) -> bool {
    bytes[at..].iter().find(|&&b| b != b'#') == Some(&b'"')
}

/// The end of a raw string whose `#`s (or quote) start at `at`.
fn raw_string_end(bytes: &[u8], at: usize) -> usize {
    let hashes = bytes[at..].iter().take_while(|&&b| b == b'#').count();
    let mut close = vec![b'"'];
    close.extend(std::iter::repeat_n(b'#', hashes));
    let body = at + hashes + 1;
    bytes[body..]
        .windows(close.len())
        .position(|window| window == close.as_slice())
        .map_or(bytes.len(), |n| body + n + close.len())
}

/// Past a `'` at `at`: a character literal, or a lifetime or label (whose
/// name is skipped too; it is never prose).
fn quote_end(src: &str, at: usize) -> usize {
    let bytes = src.as_bytes();
    let after = at + 1;
    if bytes.get(after) == Some(&b'\\') {
        // an escaped character: up to the closing quote
        return bytes
            .get(after + 2..)
            .and_then(|rest| rest.iter().position(|&b| b == b'\''))
            .map_or(bytes.len(), |n| after + 2 + n + 1);
    }
    let Some(ch) = src[after..].chars().next() else {
        return after;
    };
    let past = after + ch.len_utf8();
    if bytes.get(past) == Some(&b'\'') {
        return past + 1;
    }
    // a lifetime or a label: skip its name
    let mut end = past;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    end
}
