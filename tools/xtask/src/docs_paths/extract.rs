//! What a doc names: the repository paths in its code spans, the packages its
//! cargo commands select, and its link destinations. Pure functions over text,
//! so the rules below are pinned by unit tests without a checkout.
//!
//! A code span is a path when one of its words starts at a [`ROOTS`] directory
//! and continues past it (`crates/flui-view/src/lib.rs`, `docs/adr/`): the `/`
//! and the known first segment are what tell a path from an identifier, a
//! module path (`crate::x`) or a type. A word that is a pattern or a
//! placeholder (`*`, `{a,b}`, `<name>`, `$VAR`, `…`) names no one file and is
//! not read. A `:line` or `#anchor` suffix is dropped, and so is a `::item`
//! after a `.rs` file.
//!
//! A package is the word after `-p`/`--package` (or joined by `=`) in a command
//! that `cargo` starts, read in code spans and in code blocks, where a line
//! that ends in `\` continues on the next. A cargo command ends at `&&`, `||`,
//! `;` or `|`, so `mkdir -p` after it is not read.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// The top-level directories a path in a code span must start at.
pub(super) const ROOTS: [&str; 15] = [
    ".cargo",
    ".config",
    ".github",
    "book",
    "changelog.d",
    "crates",
    "design",
    "docs",
    "examples",
    "packages",
    "platforms",
    "specs",
    "src",
    "tests",
    "tools",
];

/// Whether `path` names another repository's layout: Flutter's sources
/// (`packages/flutter/lib/src/rendering/object.dart`, any `.dart` file), or a
/// crate whose name is not a FLUI one (GPUI's `crates/gpui/src/window.rs`).
fn foreign(path: &str) -> bool {
    path.starts_with("packages/flutter")
        || has_extension(path, "dart")
        || path
            .strip_prefix("crates/")
            .is_some_and(|rest| !rest.starts_with("flui"))
}

/// Whether `path`'s extension is `extension`, in any case.
pub(super) fn has_extension(path: &str, extension: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

/// A piece of code in a Markdown file.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Code {
    /// The 1-based line it starts on.
    pub(super) line: usize,
    pub(super) text: String,
    /// A code block (fenced or indented), not an inline code span.
    pub(super) block: bool,
}

/// The code spans and code blocks of `markdown`, in order.
pub(super) fn code(markdown: &str) -> Vec<Code> {
    let lines = LineIndex::new(markdown);
    let mut found = Vec::new();
    let mut block: Option<Code> = None;
    for (event, range) in Parser::new_ext(markdown, Options::all()).into_offset_iter() {
        match event {
            Event::Code(text) => found.push(Code {
                line: lines.line(range.start),
                text: text.into_string(),
                block: false,
            }),
            Event::Start(Tag::CodeBlock(_)) => {
                block = Some(Code {
                    line: 0,
                    text: String::new(),
                    block: true,
                });
            }
            Event::Text(text) => {
                if let Some(block) = &mut block {
                    if block.text.is_empty() {
                        block.line = lines.line(range.start);
                    }
                    block.text.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                found.extend(block.take().filter(|block| !block.text.is_empty()));
            }
            _ => {}
        }
    }
    found
}

/// The destination of every link and image in `markdown`, with its line.
pub(super) fn links(markdown: &str) -> Vec<(usize, String)> {
    let lines = LineIndex::new(markdown);
    Parser::new_ext(markdown, Options::all())
        .into_offset_iter()
        .filter_map(|(event, range)| match event {
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                Some((lines.line(range.start), dest_url.into_string()))
            }
            _ => None,
        })
        .collect()
}

/// The repository paths a code span names, each without its `:line`,
/// `#anchor` or `::item` suffix; a trailing `/` is kept (a directory).
pub(super) fn paths(span: &str) -> Vec<&str> {
    span.split_whitespace().filter_map(path).collect()
}

/// `word` as a repository path, when it is one.
fn path(word: &str) -> Option<&str> {
    let word = word.trim_start_matches(['(', '[', '"', '\'']);
    let word = word.split_once('#').map_or(word, |(path, _)| path);
    let word = match word.split_once("::") {
        Some((file, _)) if has_extension(file, "rs") => file,
        Some(_) => return None,
        None => word,
    };
    let word = strip_line(word.trim_end_matches([')', ']', ',', ';', '"', '\'', '.']));
    let (first, rest) = word.split_once('/')?;
    if !ROOTS.contains(&first) || rest.is_empty() || foreign(word) {
        return None;
    }
    let plain = word
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte));
    let dotted = word
        .trim_end_matches('/')
        .split('/')
        .any(|segment| segment.is_empty() || segment.chars().all(|c| c == '.'));
    (plain && !dotted).then_some(word)
}

/// `word` without a `:12`, `:12:5` or `:12-40` suffix.
fn strip_line(word: &str) -> &str {
    let mut word = word;
    while let Some((head, tail)) = word.rsplit_once(':') {
        if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            break;
        }
        word = head;
    }
    word
}

/// The packages the cargo commands in `code` select, each with the 0-based
/// line of `code` it is on.
pub(super) fn packages(code: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut continued = false;
    let mut in_cargo = false;
    let mut after_flag = false;
    for (index, line) in code.lines().enumerate() {
        if !continued {
            in_cargo = false;
            after_flag = false;
        }
        let body = line.trim_end();
        continued = body.ends_with('\\');
        for word in body.trim_end_matches('\\').split_whitespace() {
            let word = word.trim_matches(['`', '"', '\'']);
            if after_flag {
                after_flag = false;
                found.extend(package(word).map(|name| (index, name)));
                continue;
            }
            if matches!(word, "&&" | "||" | ";" | "|") {
                in_cargo = false;
                continue;
            }
            if word == "cargo" || word.ends_with("/cargo") {
                in_cargo = true;
            } else if in_cargo {
                if matches!(word, "-p" | "--package") {
                    after_flag = true;
                } else if let Some(name) = word
                    .strip_prefix("--package=")
                    .or_else(|| word.strip_prefix("-p="))
                {
                    found.extend(package(name).map(|name| (index, name)));
                }
            }
            if word.ends_with(';') {
                in_cargo = false;
            }
        }
    }
    found
}

/// `word` as a package name, without a `@version`, when it is not a placeholder.
fn package(word: &str) -> Option<&str> {
    let word = word.trim_end_matches([';', ',', ')']);
    let name = word.split_once('@').map_or(word, |(name, _)| name);
    let plain = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte));
    plain.then_some(name)
}

/// Byte offsets to 1-based line numbers.
struct LineIndex(Vec<usize>);

impl LineIndex {
    fn new(text: &str) -> Self {
        Self(
            std::iter::once(0)
                .chain(text.match_indices('\n').map(|(at, _)| at + 1))
                .collect(),
        )
    }

    fn line(&self, offset: usize) -> usize {
        self.0.partition_point(|&start| start <= offset)
    }
}
