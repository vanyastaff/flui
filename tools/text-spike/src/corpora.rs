//! Loads the fixed corpus files and expands a paragraph into a large
//! (~10k line) document for the "large" size measurement.

use std::path::PathBuf;

pub const CORPUS_NAMES: &[&str] = &[
    "latin",
    "arabic",
    "arabic_mixed",
    "cjk",
    "emoji_zwj",
    "devanagari",
];

pub fn load_corpus(name: &str) -> std::io::Result<String> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "corpora",
        &format!("{name}.txt"),
    ]
    .iter()
    .collect();
    std::fs::read_to_string(path)
}

/// Repeats a trimmed paragraph one-per-line until `target_lines` lines
/// exist. Deliberately simple (no reflow, no shuffling) -- the point is a
/// document large enough to expose per-call overhead and cache effects,
/// not a realistic document.
pub fn expand_to_lines(paragraph: &str, target_lines: usize) -> String {
    let paragraph = paragraph.trim();
    if target_lines == 0 || paragraph.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(paragraph.len() * target_lines + target_lines);
    for i in 0..target_lines {
        out.push_str(paragraph);
        if i + 1 < target_lines {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_to_lines_produces_exactly_the_requested_line_count() {
        let expanded = expand_to_lines("hello", 5);
        assert_eq!(expanded.lines().count(), 5);
        assert!(expanded.lines().all(|line| line == "hello"));
    }

    #[test]
    fn expand_to_lines_of_zero_is_empty() {
        assert_eq!(expand_to_lines("hello", 0), "");
    }

    #[test]
    fn every_named_corpus_file_loads_and_is_non_empty() {
        for name in CORPUS_NAMES {
            let text = load_corpus(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!text.trim().is_empty(), "{name} is empty");
        }
    }
}
