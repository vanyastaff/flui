//! Files turned into the chunks of text the marker patterns run over.
//!
//! Rust is read as tokens ([`super::tokens`]): comments and string literals
//! are prose, identifiers are matched only by the identifier patterns, and
//! every other token is ignored, so `Phase::B` or a `W400` constant in code is
//! never a marker. Doc comments are Markdown once their common indent is
//! removed (rustdoc renders them so), which makes an inline code span
//! quotation. A Markdown file is read through a CommonMark
//! parser: text and HTML are prose; code spans, code blocks, autolinks and
//! link destinations are not. Any other file (TOML, YAML, WGSL) is prose whole.
//!
//! Before matching, a prose chunk loses its bare URLs and every path that
//! starts at an archival root, so a link into the dated records does not
//! carry their names into the gate.

use std::ops::Range;
use std::sync::LazyLock;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
use regex::Regex;

use super::classes::Reading;
use super::tokens::{DocStyle, Token, tokens};
use crate::docs_links::ARCHIVAL_ROOTS;

/// A run of text and where each part of it sits in the file.
#[derive(Debug)]
pub(super) struct Chunk {
    pub(super) text: String,
    /// `(offset in text, offset in file)`, ascending: text from each offset on
    /// was copied from the file at the paired offset.
    pieces: Vec<(usize, usize)>,
    pub(super) reading: Reading,
}

impl Chunk {
    /// A chunk copied from `file[range]` in one piece.
    fn slice(file: &str, range: Range<usize>, reading: Reading) -> Self {
        Self {
            text: file[range.clone()].to_owned(),
            pieces: vec![(0, range.start)],
            reading,
        }
    }

    /// The file offset of `offset` in the chunk's text.
    pub(super) fn file_offset(&self, offset: usize) -> usize {
        let index = self.pieces.partition_point(|&(text, _)| text <= offset);
        let (text, file) = self.pieces[index.saturating_sub(1)];
        file + (offset - text)
    }
}

/// How a file is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Rust,
    Markdown,
    Text,
}

impl Kind {
    /// The reader for a path, by its extension.
    pub(super) fn of(path: &str) -> Self {
        match std::path::Path::new(path).extension() {
            Some(ext) if ext.eq_ignore_ascii_case("rs") => Self::Rust,
            Some(ext) if ext.eq_ignore_ascii_case("md") => Self::Markdown,
            _ => Self::Text,
        }
    }
}

/// The chunks of `text`, read as `kind`, prose already masked.
pub(super) fn chunks(text: &str, kind: Kind) -> Vec<Chunk> {
    let mut chunks = match kind {
        Kind::Rust => rust(text),
        Kind::Markdown => markdown(text, 0..text.len(), Reading::Markdown),
        Kind::Text => vec![Chunk::slice(text, 0..text.len(), Reading::Prose)],
    };
    for chunk in &mut chunks {
        if chunk.reading != Reading::Ident {
            mask_references(&mut chunk.text);
        }
    }
    chunks
}

fn rust(src: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    // consecutive `///` (or `//!`) lines: the content of each line
    let mut doc: Option<(DocStyle, Vec<Range<usize>>)> = None;
    for (token, range) in tokens(src) {
        let doc_line = match token {
            Token::LineComment(Some(style)) => Some(style),
            _ => None,
        };
        let continues = doc_line.is_some() && doc.as_ref().map(|d| d.0) == doc_line;
        if !continues && let Some((_, lines)) = doc.take() {
            chunks.extend(doc_chunks(&unindented(src, &lines)));
        }
        match token {
            Token::LineComment(Some(style)) => {
                // `///` or `//!`: the content starts after three bytes
                let content = range.start + 3..range.end;
                doc.get_or_insert_with(|| (style, Vec::new()))
                    .1
                    .push(content);
            }
            Token::BlockComment(Some(_)) => {
                let end = if src[range.clone()].ends_with("*/") {
                    range.end - 2
                } else {
                    range.end
                };
                let content = (range.start + 3).min(end)..end;
                chunks.extend(doc_chunks(&unindented(src, &block_lines(src, content))));
            }
            Token::LineComment(None) | Token::BlockComment(None) | Token::Str => {
                chunks.push(Chunk::slice(src, range, Reading::Prose));
            }
            Token::Ident => chunks.push(Chunk::slice(src, range, Reading::Ident)),
        }
    }
    if let Some((_, lines)) = doc {
        chunks.extend(doc_chunks(&unindented(src, &lines)));
    }
    chunks
}

/// The lines of a `/** */` body, each without the `*` that starts every line
/// after the first when all of them have one (rustc's doc-string trim).
fn block_lines(src: &str, content: Range<usize>) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = content.start;
    for line in src[content.clone()].split('\n') {
        lines.push(start..start + line.len());
        start += line.len() + 1;
    }
    let starred = |line: &Range<usize>| {
        let text = &src[line.clone()];
        let body = text.trim_start_matches([' ', '\t']);
        body.starts_with('*')
            .then(|| line.start + text.len() - body.len() + 1)
    };
    let rest = lines.get(1..).unwrap_or_default();
    let blank = |line: &Range<usize>| src[line.clone()].trim().is_empty();
    if rest.iter().any(|line| !blank(line))
        && rest
            .iter()
            .all(|line| blank(line) || starred(line).is_some())
    {
        for line in lines.iter_mut().skip(1) {
            if let Some(after) = starred(line) {
                line.start = after;
            }
        }
    }
    lines
}

/// Doc lines joined as rustdoc reads them: the smallest indent of the
/// non-blank lines is removed from each, so a paragraph written `///    text`
/// beside `/// text` is prose, not an indented code block. Each line is one piece.
fn unindented(src: &str, lines: &[Range<usize>]) -> Chunk {
    let blank = |line: &Range<usize>| src[line.clone()].trim().is_empty();
    let indent = lines
        .iter()
        .filter(|line| !blank(line))
        .map(|line| {
            src[line.clone()]
                .bytes()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count()
        })
        .min()
        .unwrap_or(0);
    let mut joined = Chunk {
        text: String::new(),
        pieces: Vec::new(),
        reading: Reading::Prose,
    };
    for line in lines {
        // a blank line is dropped whole; every other one has `indent` ASCII bytes to lose
        let start = if blank(line) {
            line.end
        } else {
            line.start + indent
        };
        joined.pieces.push((joined.text.len(), start));
        joined.text.push_str(&src[start..line.end]);
        joined.text.push('\n');
    }
    joined
}

/// A doc comment's text read as Markdown, mapped back through its pieces.
fn doc_chunks(doc: &Chunk) -> Vec<Chunk> {
    let text = doc.text.as_str();
    markdown(text, 0..text.len(), Reading::Prose)
        .into_iter()
        .map(|chunk| {
            // each Markdown chunk is one run of `text`; split it where the doc's pieces start
            let start = chunk.pieces[0].1;
            let end = start + chunk.text.len();
            let mut mapped = vec![(0, doc.file_offset(start))];
            for &(at, file) in &doc.pieces {
                if at > start && at < end {
                    mapped.push((at - start, file));
                }
            }
            Chunk {
                text: chunk.text,
                pieces: mapped,
                reading: Reading::Prose,
            }
        })
        .collect()
}

/// The prose of `src[range]` read as Markdown: text and HTML events outside
/// code blocks and autolinks. Offsets are into `src`.
fn markdown(src: &str, range: Range<usize>, reading: Reading) -> Vec<Chunk> {
    let text = &src[range.clone()];
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH;
    let mut chunks = Vec::new();
    let mut in_code_block = 0usize;
    // for each open link, whether it is an autolink or an e-mail link
    let mut links: Vec<bool> = Vec::new();
    for (event, span) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block += 1,
            Event::End(TagEnd::CodeBlock) => in_code_block = in_code_block.saturating_sub(1),
            Event::Start(Tag::Link { link_type, .. }) => {
                links.push(matches!(link_type, LinkType::Autolink | LinkType::Email));
            }
            Event::End(TagEnd::Link) => {
                links.pop();
            }
            Event::Text(_) | Event::Html(_) | Event::InlineHtml(_)
                if in_code_block == 0 && !links.iter().any(|&auto| auto) =>
            {
                let span = span.start + range.start..span.end + range.start;
                chunks.push(Chunk::slice(src, span, reading));
            }
            _ => {}
        }
    }
    chunks
}

/// Bare URLs, and path tokens that start at an archival root (after any
/// `../`), at the start of the text or after a character no path contains.
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    let roots: Vec<String> = ARCHIVAL_ROOTS
        .iter()
        .map(|root| regex::escape(root))
        .collect();
    Regex::new(&format!(
        r"https?://\S+|(?:^|[^\w./-])((?:\.\./)*(?:{})\S*)",
        roots.join("|")
    ))
    .expect("BUG: the archival roots make a valid regex")
});

/// Replaces every reference in `text` with spaces of the same byte length.
fn mask_references(text: &mut String) {
    let ranges: Vec<Range<usize>> = REFERENCE
        .captures_iter(text)
        .filter_map(|caps| caps.get(1).or_else(|| caps.get(0)))
        .map(|m| m.range())
        .collect();
    if !ranges.is_empty() {
        *text = blank(text, &ranges);
    }
}

/// `text` with each of `ranges` (on character boundaries) replaced by spaces
/// of the same byte length, so every other offset stays where it was.
pub(super) fn blank(text: &str, ranges: &[Range<usize>]) -> String {
    let mut bytes = text.as_bytes().to_vec();
    for range in ranges {
        bytes[range.clone()].fill(b' ');
    }
    String::from_utf8(bytes).expect("BUG: whole characters were replaced by ASCII spaces")
}
