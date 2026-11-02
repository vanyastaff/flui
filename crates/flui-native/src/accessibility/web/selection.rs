//! Positions in a paragraph's runs against offsets in its element's text.
//!
//! A paragraph that publishes runs (static text in a `SelectionArea`,
//! ADR-0214) is one text node in the semantics host, the runs' text joined.
//! The page's selection names UTF-16 offsets in that node; AccessKit names a
//! run and a character in it, a character being as many UTF-8 bytes as the
//! run's `character_lengths` says. Pure Rust, tested on the Linux host.

/// One run of a paragraph: its text and the UTF-8 length of each of its
/// characters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Run<'a> {
    pub(crate) text: &'a str,
    pub(crate) lengths: &'a [u8],
}

impl Run<'_> {
    /// The UTF-16 length of each character, or `None` when the lengths do
    /// not split the text on character boundaries.
    fn characters(&self) -> Option<Vec<u32>> {
        let mut start = 0;
        let mut characters = Vec::with_capacity(self.lengths.len());
        for &length in self.lengths {
            let end = start + usize::from(length);
            let character = self.text.get(start..end)?;
            let units = character.encode_utf16().count();
            characters.push(u32::try_from(units).ok()?);
            start = end;
        }
        (start == self.text.len()).then_some(characters)
    }

    /// The run's UTF-16 length.
    fn units(&self) -> Option<u32> {
        u32::try_from(self.text.encode_utf16().count()).ok()
    }
}

/// The run and the character UTF-16 `offset` into the runs' joined text
/// falls on. An offset inside a character, between the halves of a
/// surrogate pair or inside a cluster, moves back to the character's start.
/// An offset where one run ends is the next run's start; the end of the last
/// run is past its last character. `None` past the end, or for runs whose
/// lengths do not split their text.
pub(crate) fn position_at(runs: &[Run<'_>], offset: u32) -> Option<(usize, usize)> {
    let mut start = 0u32;
    for (index, run) in runs.iter().enumerate() {
        let units = run.units()?;
        let end = start.checked_add(units)?;
        let last = index + 1 == runs.len();
        if offset < end || (last && offset == end) {
            let characters = run.characters()?;
            let mut at = start;
            for (character, units) in characters.iter().enumerate() {
                if offset < at + units {
                    return Some((index, character));
                }
                at += units;
            }
            return Some((index, characters.len()));
        }
        start = end;
    }
    None
}

/// The UTF-16 offset into the runs' joined text of character `character` of
/// run `run`; `None` for a run or a character the runs do not have.
pub(crate) fn offset_of(runs: &[Run<'_>], run: usize, character: usize) -> Option<u32> {
    let mut start = 0u32;
    for before in runs.get(..run)? {
        start = start.checked_add(before.units()?)?;
    }
    let characters = runs.get(run)?.characters()?;
    let inside: u32 = characters.get(..character)?.iter().sum();
    start.checked_add(inside)
}

#[cfg(test)]
mod tests {
    use super::{Run, offset_of, position_at};

    /// The UTF-8 length of each of `characters`, as a run publishes them.
    fn lengths(characters: &[&str]) -> Vec<u8> {
        characters
            .iter()
            .map(|character| u8::try_from(character.len()).expect("a short character"))
            .collect()
    }

    /// Offsets in the joined text against run positions, both ways: a
    /// surrogate pair is one character of two units, a cluster of several
    /// code points one character, and an offset inside either moves back to
    /// its start.
    #[test]
    fn an_offset_in_the_text_is_a_run_and_a_character() {
        // "a😀" then "e\u{301}b": an emoji of two UTF-16 units, then a
        // cluster of two code points.
        let first = lengths(&["a", "😀"]);
        let second = lengths(&["e\u{301}", "b"]);
        let runs = [
            Run {
                text: "a😀",
                lengths: &first,
            },
            Run {
                text: "e\u{301}b",
                lengths: &second,
            },
        ];
        let rows = [
            (0, Some((0, 0)), "the start"),
            (1, Some((0, 1)), "before the emoji"),
            (2, Some((0, 1)), "between its halves, back to its start"),
            (3, Some((1, 0)), "where a run ends, the next one starts"),
            (4, Some((1, 0)), "inside the cluster, back to its start"),
            (5, Some((1, 1)), "after the cluster"),
            (6, Some((1, 2)), "the end of the last run"),
            (7, None, "past the end"),
        ];
        for (offset, position, why) in rows {
            assert_eq!(position_at(&runs, offset), position, "{why}");
        }
        let back = [
            ((0, 0), Some(0)),
            ((0, 1), Some(1)),
            ((0, 2), Some(3)),
            ((1, 0), Some(3)),
            ((1, 1), Some(5)),
            ((1, 2), Some(6)),
            ((1, 3), None),
            ((2, 0), None),
        ];
        for ((run, character), offset) in back {
            assert_eq!(
                offset_of(&runs, run, character),
                offset,
                "run {run}, character {character}"
            );
        }
    }

    /// Lengths that do not split a run's text on its characters name
    /// nothing.
    #[test]
    fn runs_whose_lengths_do_not_fit_their_text_name_no_position() {
        let split = [1, 3];
        let short = [1];
        for lengths in [&split[..], &short[..]] {
            let runs = [Run {
                text: "a😀",
                lengths,
            }];
            assert_eq!(position_at(&runs, 1), None, "{lengths:?}");
            assert_eq!(offset_of(&runs, 0, 1), None, "{lengths:?}");
        }
    }
}
