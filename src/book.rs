//! The book's chapters as doctests.
//!
//! Each chapter of `book/src` is the documentation of one item below, so
//! `cargo test --doc` compiles every `rust` fence in the book against this
//! facade and runs the ones that are not `no_run`. A fence marked `ignore`
//! would escape that check, and a chapter left out of this list would escape
//! it whole: the unit test at the end refuses both.

macro_rules! chapters {
    ($($(#[$cfg:meta])* $name:ident => $path:literal,)*) => {
        $(
            #[cfg(doctest)]
            $(#[$cfg])*
            #[doc = include_str!(concat!("../book/src/", $path))]
            mod $name {}
        )*

        /// Every chapter above, by its path under `book/src`.
        #[cfg(test)]
        const CHAPTERS: &[&str] = &[$($path),*];
    };
}

chapters! {
    introduction => "README.md",
    installation => "getting-started/installation.md",
    contributing => "getting-started/contributing.md",
    overview => "concepts/overview.md",
    view_element_render => "concepts/view-element-render.md",
    keys => "concepts/keys.md",
    lifecycle => "concepts/lifecycle.md",
    layout => "concepts/layout.md",
    state => "concepts/state.md",
    mapping => "mapping.md",
    catalog => "widgets/catalog.md",
    cookbook => "cookbook/overview.md",
    forms => "cookbook/forms.md",
    async_work => "cookbook/async.md",
    themes => "cookbook/themes.md",
    animation => "cookbook/animation.md",
    // `flui::testing` exists only with the `testing` feature; CI runs the
    // book's doctests with it (`ci.yml` `doc-test`, `docs.yml`).
    #[cfg(feature = "testing")]
    testing => "cookbook/testing.md",
    architecture => "architecture.md",
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::CHAPTERS;

    fn book_source() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/book/src"))
    }

    /// The chapters `SUMMARY.md` links, plus the introduction it names as
    /// `README.md`.
    fn summary_chapters() -> BTreeSet<String> {
        let summary = std::fs::read_to_string(book_source().join("SUMMARY.md"))
            .expect("BUG: book/src/SUMMARY.md is part of the repository");
        summary
            .split("](")
            .skip(1)
            .filter_map(|link| link.split_once(')').map(|(path, _)| path.to_owned()))
            .collect::<BTreeSet<String>>()
    }

    #[test]
    fn every_chapter_of_the_book_is_doctested() {
        let listed = CHAPTERS
            .iter()
            .map(|path| (*path).to_owned())
            .collect::<BTreeSet<String>>();
        assert_eq!(
            summary_chapters(),
            listed,
            "src/book.rs lists exactly the chapters book/src/SUMMARY.md links"
        );
    }

    #[test]
    fn no_rust_sample_in_the_book_is_ignored() {
        let mut ignored = Vec::<String>::new();
        for chapter in CHAPTERS {
            let text = std::fs::read_to_string(book_source().join(chapter)).expect(
                "BUG: every listed chapter exists, as mdBook's create-missing = false checks",
            );
            for (index, line) in text.lines().enumerate() {
                // a fence may sit in a block quote or a list item, and open
                // with backticks or tildes
                let fenced = line.trim_start_matches(|c: char| c.is_whitespace() || c == '>');
                let Some(info) = fenced
                    .strip_prefix("```")
                    .or_else(|| fenced.strip_prefix("~~~"))
                else {
                    continue;
                };
                // rustdoc splits a fence's attributes on commas, spaces, braces and
                // quotes, and `ignore-<target>` skips the sample on that target
                if info
                    .split(|c: char| matches!(c, ',' | '{' | '}' | '"') || c.is_whitespace())
                    .any(|attribute| attribute == "ignore" || attribute.starts_with("ignore-"))
                {
                    ignored.push(format!("{chapter}:{}", index + 1));
                }
            }
        }
        assert!(
            ignored.is_empty(),
            "a book sample compiles as a doctest; mark text that is not Rust `text`: {ignored:?}"
        );
    }
}
