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
//! subcommand before it, read in code spans and in code blocks as a shell
//! splits them ([`super::shell`]): `mkdir -p` after `&&` is not cargo's, nor is
//! `echo "cargo test -p x"`, nor a `-p` after `--`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::shell;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

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

/// The crate directories of other repositories the docs cite as references:
/// GPUI's (`crates/gpui/src/window.rs`, `crates/gpui_macos/…`) and Bevy's.
const FOREIGN_CRATES: [&str; 2] = ["crates/gpui", "crates/bevy_"];

/// Whether `path` names another repository's layout: Flutter's sources
/// (`packages/flutter/lib/src/rendering/object.dart`, any `.dart` file), or a
/// [`FOREIGN_CRATES`] one. Any other `crates/` path is this repository's, so a
/// misspelt crate name is a finding.
fn foreign(path: &str) -> bool {
    path.starts_with("packages/flutter")
        || has_extension(path, "dart")
        || FOREIGN_CRATES.iter().any(|prefix| path.starts_with(prefix))
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
    /// The shell the code is written for: PowerShell for a `powershell`,
    /// `pwsh` or `ps1` fence, POSIX otherwise.
    pub(super) dialect: shell::Dialect,
}

/// The shell a code fence's info string (`bash`, `powershell title=x`) names.
fn dialect(info: &str) -> shell::Dialect {
    let language = info.split_whitespace().next().unwrap_or_default();
    if ["powershell", "pwsh", "ps1", "ps"]
        .iter()
        .any(|name| language.eq_ignore_ascii_case(name))
    {
        shell::Dialect::PowerShell
    } else {
        shell::Dialect::Posix
    }
}

/// The code spans and code blocks of `markdown`, in order, but for a code
/// span labelling a [`pinned`] permalink.
pub(super) fn code(markdown: &str) -> Vec<Code> {
    let lines = LineIndex::new(markdown);
    let mut found = Vec::new();
    let mut block: Option<Code> = None;
    // CommonMark links do not nest
    let mut in_pinned = false;
    for (event, range) in Parser::new_ext(markdown, Options::all()).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => in_pinned = pinned(&dest_url),
            Event::End(TagEnd::Link) => in_pinned = false,
            Event::Code(_) if in_pinned => {}
            Event::Code(text) => found.push(Code {
                line: lines.line(range.start),
                text: text.into_string(),
                block: false,
                dialect: shell::Dialect::Posix,
            }),
            Event::Start(Tag::CodeBlock(kind)) => {
                let dialect = match kind {
                    CodeBlockKind::Fenced(info) => dialect(&info),
                    CodeBlockKind::Indented => shell::Dialect::Posix,
                };
                block = Some(Code {
                    line: 0,
                    text: String::new(),
                    block: true,
                    dialect,
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

/// Whether `dest` (its scheme and host in any case) is a permalink to this repository at a commit (a hex hash of
/// 7 to 40 digits): it cites a file as it was then, deleted since or not. A
/// branch, `main` or a misspelling of it, is not a pinned revision.
fn pinned(dest: &str) -> bool {
    const GITHUB: &str = "https://github.com/";
    let Some(rest) = dest
        .get(..GITHUB.len())
        .filter(|origin| origin.eq_ignore_ascii_case(GITHUB))
        .map(|_| &dest[GITHUB.len()..])
    else {
        return false;
    };
    const REPO: &str = "vanyastaff/flui/";
    let Some(rest) = rest
        .get(..REPO.len())
        .filter(|repo| repo.eq_ignore_ascii_case(REPO))
        .map(|_| &rest[REPO.len()..])
    else {
        return false;
    };
    ["blob", "tree"].iter().any(|kind| {
        rest.strip_prefix(&format!("{kind}/"))
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(reference, _)| {
                (7..=40).contains(&reference.len())
                    && reference
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            })
    })
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

/// The `#anchor`s the headings of `markdown` give, as GitHub makes them: the
/// text lowercased, spaces turned to `-`, other punctuation but `-` and `_`
/// dropped, and after a repeat the first of `-1`, `-2`, … no heading has. GitHub has no `{#id}` heading
/// attribute: it renders the braces as text, so the parser's extension is off.
pub(super) fn anchors(markdown: &str) -> BTreeSet<String> {
    let mut anchors = BTreeSet::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut heading: Option<String> = None;
    // GitHub keeps `--` and quotes as written: no smart punctuation
    let options = Options::all()
        .difference(Options::ENABLE_HEADING_ATTRIBUTES)
        .difference(Options::ENABLE_SMART_PUNCTUATION);
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            // `$x$` is text in GitHub's heading slug
            Event::Text(text)
            | Event::Code(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => {
                if let Some(heading) = &mut heading {
                    heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                let Some(text) = heading.take() else {
                    continue;
                };
                let slug: String = text
                    .to_lowercase()
                    .chars()
                    .filter_map(|c| match c {
                        ' ' => Some('-'),
                        c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
                        _ => None,
                    })
                    .collect();
                // a repeat takes the next `-N` no heading has taken yet
                let mut anchor = slug.clone();
                while anchors.contains(&anchor) {
                    let repeat = seen.entry(slug.clone()).or_default();
                    *repeat += 1;
                    anchor = format!("{slug}-{repeat}");
                }
                anchors.insert(anchor);
            }
            _ => {}
        }
    }
    anchors
}

/// The repository paths a code span names, each without its `:line`,
/// `#anchor` or `::item` suffix and with its `.` and `..` segments resolved;
/// a trailing `/` is kept (a directory).
pub(super) fn paths(span: &str) -> Vec<String> {
    span.split_whitespace().filter_map(path).collect()
}

/// `word` as a repository path, when it is one. A path that climbs above the
/// root with `..` names nothing in the checkout and is not one.
fn path(word: &str) -> Option<String> {
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
    if !plain {
        return None;
    }
    let mut segments = Vec::new();
    for segment in word.trim_end_matches('/').split('/') {
        match segment {
            "." => {}
            ".." => {
                segments.pop()?;
            }
            // `docs//x`, or an ellipsis standing for elided segments
            segment if segment.chars().all(|c| c == '.') => return None,
            segment => segments.push(segment),
        }
    }
    if segments.is_empty() {
        return None;
    }
    let mut path = segments.join("/");
    if word.ends_with('/') {
        path.push('/');
    }
    Some(path)
}

/// `word` without a `:12`, `:12:5`, `:12-40` or `:53,67` suffix.
fn strip_line(word: &str) -> &str {
    let mut word = word;
    while let Some((head, tail)) = word.rsplit_once(':') {
        if tail.is_empty()
            || !tail
                .bytes()
                .all(|b| b.is_ascii_digit() || b"-,".contains(&b))
        {
            break;
        }
        word = head;
    }
    word
}

/// A package a cargo command selects.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Selected {
    /// The 0-based line of the code it is on.
    pub(super) line: usize,
    /// The cargo subcommand (`test`, `update`), when one came before it.
    pub(super) subcommand: Option<String>,
    pub(super) name: String,
    /// The version after `@` (`bitflags@2`), when the spec names one.
    pub(super) version: Option<String>,
    /// The source a fully qualified spec names (`registry+https://…`).
    pub(super) source: Option<String>,
}

/// Cargo's options before the subcommand that take a value in the next word
/// (`cargo --color always update`).
const GLOBAL_VALUE_OPTIONS: [&str; 4] = ["--color", "--config", "-C", "-Z"];

/// Shell reserved words that can stand before a command (`{ cargo test; }`,
/// `if cargo test; then`).
const RESERVED: [&str; 9] = [
    "{", "!", "if", "then", "else", "elif", "while", "until", "do",
];

/// `sudo`'s options that take a value in the next word.
const SUDO_VALUE_OPTIONS: [&str; 20] = [
    "-u",
    "-g",
    "-C",
    "-D",
    "-h",
    "-p",
    "-r",
    "-t",
    "-U",
    "-T",
    "--user",
    "--group",
    "--close-from",
    "--chdir",
    "--host",
    "--prompt",
    "--role",
    "--type",
    "--other-user",
    "--command-timeout",
];

/// GNU `time`'s options that take a value in the next word.
const TIME_VALUE_OPTIONS: [&str; 4] = ["-f", "--format", "-o", "--output"];

/// `env`'s options that take a value in the next word.
const ENV_VALUE_OPTIONS: [&str; 4] = ["-u", "--unset", "-C", "--chdir"];

/// Cargo's short options that take a value, so a `p` after one in a cluster
/// (`-Zp…`) is that value, not `-p`.
const SHORT_VALUE_OPTIONS: [char; 4] = ['C', 'F', 'Z', 'j'];

/// [`packages_in`] of POSIX shell code.
#[cfg(test)]
pub(super) fn packages(code: &str) -> Vec<Selected> {
    packages_in(code, shell::Dialect::Posix)
}

/// The packages the cargo commands in `code`, written for `dialect`, select,
/// in every spelling cargo takes: `-p x`, `--package x`, `-p=x`,
/// `--package=x`, `-px` and a cluster (`-qpx`, `-qp x`). A command is cargo's
/// when its first word, after any `NAME=value` assignments and a wrapper
/// (`env`, `time`, `sudo`, `exec`, `command`), is `cargo` (or a path ending in
/// `/cargo`); its arguments stop at `--`, after which they are the program's.
pub(super) fn packages_in(code: &str, dialect: shell::Dialect) -> Vec<Selected> {
    let mut found = Vec::new();
    for command in shell::commands_in(code, dialect) {
        let mut words: VecDeque<(usize, String)> = command.into();
        if !cargo_command(&mut words) {
            continue;
        }
        let mut subcommand: Option<String> = None;
        while let Some((line, word)) = words.pop_front() {
            if word == "--" {
                break;
            }
            let (line, name) = match package_flag(&word) {
                PackageFlag::Joined(value) => (line, value.to_owned()),
                PackageFlag::NextWord => match words.pop_front() {
                    Some(value) => value,
                    None => break,
                },
                PackageFlag::None => {
                    if subcommand.is_none() {
                        // script mode (`cargo -Zscript app.rs ARGS`, `cargo app.rs`):
                        // what follows the manifest is the script's
                        let script = word == "-Zscript"
                            || (word == "-Z"
                                && words.front().is_some_and(|(_, next)| next == "script"))
                            || has_extension(&word, "rs");
                        if script {
                            break;
                        }
                        if GLOBAL_VALUE_OPTIONS.contains(&word.as_str()) {
                            words.pop_front();
                        } else if !word.starts_with(['-', '+']) {
                            subcommand = Some(word);
                        }
                    }
                    continue;
                }
            };
            found.extend(package(&name).map(|(name, version, source)| Selected {
                line,
                subcommand: subcommand.clone(),
                name,
                version,
                source,
            }));
        }
    }
    found
}

/// Takes the words before cargo's arguments off the front of `words`: the
/// `NAME=value` assignments, an `env` wrapper with its options (`-S` splits
/// its value into the command it runs), and `cargo`. `false` when the
/// command is not cargo's.
fn cargo_command(words: &mut VecDeque<(usize, String)>) -> bool {
    loop {
        while words
            .front()
            .is_some_and(|(_, word)| assignment(word) || RESERVED.contains(&word.as_str()))
        {
            words.pop_front();
        }
        let Some((_, program)) = words.pop_front() else {
            return false;
        };
        if is_cargo(&program) {
            return true;
        }
        // `sudo [OPTION]... COMMAND` runs it as another user
        if program == "sudo" || program.ends_with("/sudo") {
            while let Some((line, word)) = words.pop_front() {
                if word == "--" {
                    break;
                } else if SUDO_VALUE_OPTIONS.contains(&word.as_str()) {
                    words.pop_front();
                } else if !word.starts_with('-') && !assignment(&word) {
                    words.push_front((line, word));
                    break;
                }
            }
            continue;
        }
        // `exec [-cl] [-a NAME] COMMAND` runs it in the shell's place
        if program == "exec" {
            while let Some((line, word)) = words.pop_front() {
                if word == "-a" {
                    words.pop_front();
                } else if !word.starts_with('-') {
                    words.push_front((line, word));
                    break;
                }
            }
            continue;
        }
        // `command [-p] COMMAND` runs it; `command -v`/`-V` only looks it up
        if program == "command" {
            while let Some((line, word)) = words.pop_front() {
                if matches!(word.as_str(), "-v" | "-V") {
                    return false;
                }
                if !word.starts_with('-') {
                    words.push_front((line, word));
                    break;
                }
            }
            continue;
        }
        // `time [OPTION]... COMMAND`: the shell keyword and GNU time alike
        if program == "time" || program.ends_with("/time") {
            while let Some((line, word)) = words.pop_front() {
                if TIME_VALUE_OPTIONS.contains(&word.as_str()) {
                    words.pop_front();
                } else if !word.starts_with('-') {
                    words.push_front((line, word));
                    break;
                }
            }
            continue;
        }
        if program != "env" && !program.ends_with("/env") {
            return false;
        }
        // `env [OPTION]... [NAME=VALUE]... COMMAND [ARG]...` runs COMMAND
        while let Some((line, word)) = words.pop_front() {
            let split = if matches!(word.as_str(), "-S" | "--split-string") {
                words.pop_front().map(|(_, value)| value)
            } else {
                word.strip_prefix("--split-string=")
                    .or_else(|| {
                        word.strip_prefix("-S")
                            .filter(|value| !value.is_empty() && !word.starts_with("--"))
                    })
                    .map(str::to_owned)
            };
            if let Some(split) = split {
                for (_, part) in shell::commands_in(&split, shell::Dialect::Posix)
                    .into_iter()
                    .flatten()
                    .rev()
                {
                    words.push_front((line, part));
                }
                break;
            } else if ENV_VALUE_OPTIONS.contains(&word.as_str()) {
                words.pop_front();
            } else if !word.starts_with('-') {
                // an assignment or the command: the outer loop reads it
                words.push_front((line, word));
                break;
            }
        }
    }
}

/// What one of cargo's words says about the package it selects.
enum PackageFlag<'a> {
    /// No package option.
    None,
    /// A package option whose value is the next word (`-p`, `-qp`).
    NextWord,
    /// A package option with its value in the word (`-px`, `--package=x`).
    Joined(&'a str),
}

/// What `word` says about the package it selects. A short cluster (`-qpx`)
/// is read as clap reads it: flags up to `p`, then the rest is the value,
/// unless a flag before `p` takes one.
fn package_flag(word: &str) -> PackageFlag<'_> {
    if matches!(word, "-p" | "--package") {
        return PackageFlag::NextWord;
    }
    if let Some(value) = word.strip_prefix("--package=") {
        return PackageFlag::Joined(value);
    }
    let Some(cluster) = word.strip_prefix('-').filter(|rest| !rest.starts_with('-')) else {
        return PackageFlag::None;
    };
    for (at, flag) in cluster.char_indices() {
        if flag == 'p' {
            let value = &cluster[at + 1..];
            let value = value.strip_prefix('=').unwrap_or(value);
            return if value.is_empty() {
                PackageFlag::NextWord
            } else {
                PackageFlag::Joined(value)
            };
        }
        if SHORT_VALUE_OPTIONS.contains(&flag) || !flag.is_ascii_alphabetic() {
            return PackageFlag::None;
        }
    }
    PackageFlag::None
}

/// Whether `word` is a `NAME=value` or `NAME+=value` assignment before a command (PowerShell's
/// `$env:NAME=value` too).
fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        let name = name.strip_prefix("$env:").unwrap_or(name);
        // `NAME+=value` appends
        let name = name.strip_suffix('+').unwrap_or(name);
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// Whether `program` is cargo: `cargo`, or `cargo.exe` on Windows, bare or at
/// the end of a path.
fn is_cargo(program: &str) -> bool {
    let file = program.rsplit(['/', '\\']).next().unwrap_or(program);
    file == "cargo" || file.eq_ignore_ascii_case("cargo.exe")
}

/// `word` as a package spec: its name (a glob too, [`is_glob`]), the
/// version after `@`, if any, and the source a fully qualified spec names;
/// `None` for a placeholder (`<crate>`, `$CRATE`,
/// `{name}`, `…`), which names no one package. Any other word is taken as written, so a malformed name
/// (`definitely.missing`) is a finding, as cargo rejects it.
fn package(word: &str) -> Option<(String, Option<String>, Option<String>)> {
    let placeholder = word.is_empty()
        || word.starts_with('@')
        || word.contains(['<', '>', '$', '{', '}', '…'])
        || word.contains("...");
    if placeholder {
        return None;
    }
    // a fully qualified spec, `[kind+]url[#name][@|:version]`: the name is the
    // fragment's, or else the URL's last path segment; the URL is its source
    let (spec, source) = match word.rsplit_once('#') {
        Some((url, fragment)) if url.contains("://") => {
            let spec = if fragment.starts_with(|c: char| c.is_ascii_alphabetic()) {
                fragment.to_owned()
            } else {
                format!("{}@{fragment}", last_segment(url))
            };
            (spec, Some(url.to_owned()))
        }
        _ if word.contains("://") => (last_segment(word).to_owned(), Some(word.to_owned())),
        _ => (word.to_owned(), None),
    };
    // `name@version`, or the legacy `name:version`
    Some(
        match spec.split_once('@').or_else(|| spec.split_once(':')) {
            Some((name, version)) => (name.to_owned(), Some(version.to_owned()), source),
            None => (spec, None, source),
        },
    )
}

/// The last path segment of `url` (`…/crates/flui-view` gives `flui-view`).
fn last_segment(url: &str) -> &str {
    url.trim_end_matches('/').rsplit('/').next().unwrap_or(url)
}

/// Whether a `-p` value is a glob cargo matches against package names
/// (`flui-*`, `flui-?iew`).
pub(super) fn is_glob(name: &str) -> bool {
    name.contains(['*', '?', '['])
}

/// Whether the glob `pattern` matches all of `name`: `*` any run, `?` any one
/// character, `[abc]`/`[a-z]` one of a class (`[!…]`/`[^…]` one not in it).
/// A table over (pattern token, name prefix), so no pattern takes more than
/// their product in steps (`***…z` does not backtrack).
pub(super) fn glob_matches(pattern: &str, name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    // `reached[j]`: the tokens so far match the first `j` characters of `name`
    let mut reached = vec![false; name.len() + 1];
    reached[0] = true;
    for token in glob_tokens(pattern) {
        let mut next = vec![false; name.len() + 1];
        match token {
            GlobToken::Star => {
                let mut any = false;
                for (j, slot) in next.iter_mut().enumerate() {
                    any |= reached[j];
                    *slot = any;
                }
            }
            GlobToken::One(matches) => {
                for (j, &c) in name.iter().enumerate() {
                    next[j + 1] = reached[j] && matches(c);
                }
            }
        }
        reached = next;
    }
    reached[name.len()]
}

/// One token of a glob.
enum GlobToken {
    /// `*`: any run of characters.
    Star,
    /// One character the predicate accepts: `?`, a class, or a literal.
    One(Box<dyn Fn(char) -> bool>),
}

/// The tokens of `pattern`; an unclosed `[` is a literal.
fn glob_tokens(pattern: &str) -> Vec<GlobToken> {
    let pattern: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < pattern.len() {
        let c = pattern[at];
        at += 1;
        tokens.push(match c {
            '*' => GlobToken::Star,
            '?' => GlobToken::One(Box::new(|_| true)),
            '[' => match class(&pattern[at..]) {
                Some((set, len)) => {
                    at += len;
                    GlobToken::One(Box::new(set))
                }
                None => GlobToken::One(Box::new(|c| c == '[')),
            },
            literal => GlobToken::One(Box::new(move |c| c == literal)),
        });
    }
    tokens
}

/// The class after a `[`: whether a character is in it, and how many
/// characters of the pattern it takes, its `]` included; `None` when unclosed.
fn class(pattern: &[char]) -> Option<(impl Fn(char) -> bool + 'static, usize)> {
    let negated = matches!(pattern.first(), Some('!' | '^'));
    let body_start = usize::from(negated);
    // a `]` first in the class is a member, not its end
    let close = pattern
        .iter()
        .skip(body_start + 1)
        .position(|&c| c == ']')
        .map(|at| at + body_start + 1)?;
    let body: Vec<char> = pattern[body_start..close].to_vec();
    let contains = move |c: char| {
        let mut at = 0;
        while at < body.len() {
            if at + 2 < body.len() && body[at + 1] == '-' {
                if (body[at]..=body[at + 2]).contains(&c) {
                    return true;
                }
                at += 3;
            } else {
                if body[at] == c {
                    return true;
                }
                at += 1;
            }
        }
        false
    };
    Some((move |c| contains(c) != negated, close + 1))
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
