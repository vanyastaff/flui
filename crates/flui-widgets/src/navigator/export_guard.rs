//! The shared "this name is not exported" check, used by every layer's privacy
//! test.
//!
//! # Why not `source.contains(name)`
//!
//! Because it is wrong twice. The navigator seam exports `RouteBindingSlot`, which
//! *contains* the string `RouteBinding` — the private capability it deliberately
//! hides. A substring scan flags the safe export and would tempt someone to relax
//! the guard rather than fix it. Splitting each `pub use` line into identifiers
//! and comparing exactly says what the test means.

/// Every Rust identifier appearing in a `pub use` / `pub mod` **statement** of
/// `source` — including the ones rustfmt wrapped onto continuation lines.
///
/// # Why a statement and not a line
///
/// This filtered to lines *starting* `pub use`, which meant that for a wrapped
/// re-export only the **first physical line** was ever scanned. `lib.rs`'s
/// navigator block is `pub use navigator::{` followed by thirty-odd names on
/// continuation lines, so the guard saw none of them — provable rather than
/// inferred: `PageRoute` sat on the callers' `INTERNAL` lists *and* was exported
/// from that block, and the guard passed.
///
/// The failure had no signal and a cliff behind it. `navigator/mod.rs`'s
/// `pub use named_route::{…}` was 94 characters against `max_width = 100`; one
/// more export name would have wrapped it and silently taken five more names out
/// of scope, and two more exports are already parked as "purely additive later".
///
/// So the unit is the statement: accumulate from the line that opens it to the
/// one carrying its terminating `;`. Identifiers stay borrowed from `source`
/// because each line is tokenised where it sits rather than joined.
fn exported_identifiers(source: &str) -> Vec<&str> {
    let mut exported = Vec::new();
    let mut inside_statement = false;

    for line in source.lines() {
        let code = line.trim_start();
        // Comments are stripped before tokenising: a `// see RouteHistory` note
        // inside a re-export block would otherwise read as an exported name.
        let code = code.split("//").next().unwrap_or(code);

        if code.starts_with("pub use") || code.starts_with("pub mod") {
            inside_statement = true;
        }
        if !inside_statement {
            continue;
        }

        exported.extend(
            code.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|token| !token.is_empty()),
        );

        if code.contains(';') {
            inside_statement = false;
        }
    }

    exported
}

/// Fail if any of `forbidden` is exported from `source`, naming the file.
pub(crate) fn assert_not_exported(file: &str, source: &str, forbidden: &[&str]) {
    let exported = exported_identifiers(source);
    for name in forbidden {
        assert!(
            !exported.contains(name),
            "{file} exports the internal `{name}` — it has no sign-off gate yet"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_matched_whole_not_as_substrings() {
        let source = "pub use binding::RouteBindingSlot;\nlet RouteHistory = 1;\n";
        assert_not_exported("x.rs", source, &["RouteBinding", "RouteHistory"]);
    }

    #[test]
    #[should_panic(expected = "exports the internal `RouteHistory`")]
    fn a_real_export_is_caught() {
        assert_not_exported(
            "x.rs",
            "pub use history::RouteHistory;",
            &["RouteHistory", "RouteBinding"],
        );
    }

    #[test]
    fn only_pub_use_and_pub_mod_lines_are_scanned() {
        assert_not_exported("x.rs", "use history::RouteHistory;", &["RouteHistory"]);
    }

    /// The bug this scanner was rewritten for: a name on a **continuation** line.
    ///
    /// The old line-filtered scanner passed this, because only the line starting
    /// `pub use` was ever examined. Note the leaked name is deliberately *not*
    /// first in the block — a fixture that put it on the opening line would pass
    /// against the broken scanner too and prove nothing.
    #[test]
    #[should_panic(expected = "exports the internal `RouteHistory`")]
    fn a_name_wrapped_onto_a_continuation_line_is_caught() {
        assert_not_exported(
            "x.rs",
            "pub use navigator::{\n    Navigator, RouteHistory, RouteId,\n};\n",
            &["RouteHistory"],
        );
    }

    /// And the statement ends at its `;`, so a later plain `use` is still ignored.
    #[test]
    fn scanning_stops_at_the_terminating_semicolon() {
        assert_not_exported(
            "x.rs",
            "pub use navigator::{\n    Navigator,\n};\nuse history::RouteHistory;\n",
            &["RouteHistory"],
        );
    }

    /// A comment inside a re-export block is not an export.
    #[test]
    fn a_comment_inside_a_block_is_not_scanned() {
        assert_not_exported(
            "x.rs",
            "pub use navigator::{\n    // see RouteHistory for why\n    Navigator,\n};\n",
            &["RouteHistory"],
        );
    }
}
