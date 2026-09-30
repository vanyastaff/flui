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
//! A package is the word after `-p`/`--package` (or joined to it, `-px`,
//! `-p=x`, `--package=x`) in a command that `cargo` starts, with the
//! subcommand before it, read in code spans and in code blocks, where a line
//! that ends in `\` continues on the next. A cargo command ends at `&&`, `||`,
//! `;` or `|`, so `mkdir -p` after it is not read.

use std::collections::{BTreeMap, BTreeSet};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// The top-level directories a path in a code span must start at.
pub(super) const ROOTS: [&str; 16] = [
    ".cargo",
    ".config",
    ".github",
    ".rust-studio",
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

/// The code spans and code blocks of `markdown`, in order, but for a code
/// span that is (part of) a link's text.
pub(super) fn code(markdown: &str) -> Vec<Code> {
    let lines = LineIndex::new(markdown);
    let mut found = Vec::new();
    let mut block: Option<Code> = None;
    // a code span in a link's text labels the link, which lychee checks; a
    // permalink to a commit cites a file as it was, deleted since or not
    let mut links = 0_usize;
    for (event, range) in Parser::new_ext(markdown, Options::all()).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { .. }) => links += 1,
            Event::End(TagEnd::Link) => links = links.saturating_sub(1),
            Event::Code(_) if links > 0 => {}
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

/// The `#anchor`s the headings of `markdown` give, as GitHub makes them: an
/// explicit `{#id}`, or the text lowercased, spaces turned to `-`, other
/// punctuation but `-` and `_` dropped, and `-1`, `-2`, … after a repeat.
pub(super) fn anchors(markdown: &str) -> BTreeSet<String> {
    let mut anchors = BTreeSet::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut heading: Option<(Option<String>, String)> = None;
    for event in Parser::new_ext(markdown, Options::all()) {
        match event {
            Event::Start(Tag::Heading { id, .. }) => {
                heading = Some((id.map(pulldown_cmark::CowStr::into_string), String::new()));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, heading)) = &mut heading {
                    heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                let Some((id, text)) = heading.take() else {
                    continue;
                };
                if let Some(id) = id {
                    anchors.insert(id);
                    continue;
                }
                let slug: String = text
                    .to_lowercase()
                    .chars()
                    .filter_map(|c| match c {
                        ' ' => Some('-'),
                        c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
                        _ => None,
                    })
                    .collect();
                let repeat = seen.entry(slug.clone()).or_default();
                anchors.insert(if *repeat == 0 {
                    slug
                } else {
                    format!("{slug}-{repeat}")
                });
                *repeat += 1;
            }
            _ => {}
        }
    }
    anchors
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

/// A package a cargo command selects.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Selected<'a> {
    /// The 0-based line of the code it is on.
    pub(super) line: usize,
    /// The cargo subcommand (`test`, `update`), when one came before it.
    pub(super) subcommand: Option<&'a str>,
    pub(super) name: &'a str,
}

/// The packages the cargo commands in `code` select, in every spelling cargo
/// takes: `-p x`, `--package x`, `-p=x`, `--package=x` and `-px`.
pub(super) fn packages(code: &str) -> Vec<Selected<'_>> {
    let mut found = Vec::new();
    let mut continued = false;
    // `Some(subcommand)` inside a cargo command
    let mut cargo: Option<Option<&str>> = None;
    let mut after_flag = false;
    for (index, line) in code.lines().enumerate() {
        if !continued {
            cargo = None;
            after_flag = false;
        }
        let body = line.trim_end();
        continued = body.ends_with('\\');
        for word in body.trim_end_matches('\\').split_whitespace() {
            let word = word.trim_matches(['`', '"', '\'']);
            let mut select = |subcommand, word| {
                found.extend(package(word).map(|name| Selected {
                    line: index,
                    subcommand,
                    name,
                }));
            };
            if after_flag {
                after_flag = false;
                select(cargo.flatten(), word);
                continue;
            }
            if matches!(word, "&&" | "||" | ";" | "|") {
                cargo = None;
                continue;
            }
            if word == "cargo" || word.ends_with("/cargo") {
                cargo = Some(None);
            } else if let Some(subcommand) = &mut cargo {
                if matches!(word, "-p" | "--package") {
                    after_flag = true;
                } else if let Some(name) = word
                    .strip_prefix("--package=")
                    .or_else(|| word.strip_prefix("-p="))
                    .or_else(|| word.strip_prefix("-p").filter(|_| !word.starts_with("--")))
                {
                    select(*subcommand, name);
                } else if subcommand.is_none() && !word.starts_with(['-', '+']) {
                    *subcommand = Some(word.trim_end_matches(';'));
                }
            }
            if word.ends_with(';') {
                cargo = None;
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
