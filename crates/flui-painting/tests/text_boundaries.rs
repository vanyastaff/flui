//! The grapheme and word boundaries an editor steps through
//! (`flui_painting::text_boundaries`, ADR-0092 §6).

use flui_painting::text_boundaries::{
    graphemes, is_grapheme_boundary, next_grapheme_boundary, previous_grapheme_boundary,
    word_segments, word_segments_from,
};

/// Starting a query at its line, rather than at the text's start,
/// answers what a walk over the whole text answers: every grapheme query
/// at every byte offset, and the word segments from every line, of texts
/// whose lines hold clusters that depend on what precedes them (a CR LF
/// split across the start of a line, regional-indicator pairs, a ZWJ
/// sequence, a combining mark). Fails if a line start is taken somewhere
/// a break is not mandatory, such as between CR and LF.
#[test]
fn a_query_from_its_line_answers_as_the_whole_text_does() {
    let corpus = [
        "ab\r\ncd\n\n\u{1f1fa}\u{1f1f8}\u{1f1eb}\u{1f1f7}\r\n\u{1f468}\u{200d}\u{1f469}",
        "\n\re\u{301}\n x  y\r\n\r\n",
        "\u{915}\u{94d}\u{937}\u{93f}\nfoo bar\u{2019}s 3.14\n",
    ];
    for text in corpus {
        let boundaries: Vec<usize> = graphemes(text)
            .map(|cluster| cluster.start)
            .chain(std::iter::once(text.len()))
            .collect();
        for offset in 0..=text.len() + 1 {
            let clamped = offset.min(text.len());
            let previous = boundaries
                .iter()
                .copied()
                .take_while(|&b| b < clamped)
                .last()
                .unwrap_or(0);
            let next = boundaries
                .iter()
                .copied()
                .find(|&b| b > clamped)
                .unwrap_or(text.len());
            assert_eq!(
                previous_grapheme_boundary(text, offset),
                previous,
                "{text:?} {offset}"
            );
            assert_eq!(
                next_grapheme_boundary(text, offset),
                next,
                "{text:?} {offset}"
            );
            assert_eq!(
                is_grapheme_boundary(text, offset),
                boundaries.contains(&offset),
                "{text:?} {offset}"
            );
        }
        let whole: Vec<_> = word_segments(text).collect();
        for at in 0..=text.len() {
            let from_line: Vec<_> = word_segments_from(text, at).collect();
            assert!(
                whole.ends_with(&from_line),
                "{text:?} from {at}: {from_line:?} is not a tail of {whole:?}"
            );
        }
    }
}
