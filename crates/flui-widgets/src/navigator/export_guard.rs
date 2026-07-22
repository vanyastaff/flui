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

/// Every Rust identifier appearing on a `pub use` / `pub mod` line of `source`.
fn exported_identifiers(source: &str) -> Vec<&str> {
    source
        .lines()
        .map(str::trim_start)
        .filter(|code| code.starts_with("pub use") || code.starts_with("pub mod"))
        .flat_map(|code| {
            code.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|token| !token.is_empty())
        })
        .collect()
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
}
