//! Panic-payload text extraction and the `BUG:` internal-invariant classifier.
//!
//! Every `catch_unwind` boundary that reads a caught payload needs the
//! same two decidable operations — pull whatever text the payload carries
//! (or honestly admit it has none), and decide whether that text asserts a
//! FLUI-owned invariant rather than reporting a caller- or application-code
//! failure. This is their one home: a boundary that reads payload text
//! calls these two functions and never downcasts the payload itself, so
//! what one boundary reports is what every boundary reports. Boundaries
//! that only `resume_unwind` or discard the payload have no reason to
//! call either.
//!
//! See `docs/PANIC-POLICY.md`'s "The `BUG:` message convention" for the
//! policy [`is_internal_invariant`] mechanizes.

use std::any::Any;

/// The text of a panic payload, when it has any.
///
/// `panic!("literal")` produces a `&'static str` payload; `panic!("{}", x)`
/// and every other formatted `panic!` produce a `String`; anything else
/// (constructed with [`std::panic::panic_any`]) is opaque and reported as
/// `None` — never guessed at.
#[must_use]
pub fn payload_text(payload: &(dyn Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
}

/// Whether `text` asserts a FLUI-owned internal invariant rather than a
/// caller- or application-code failure.
///
/// This classifies only — it never decides how a caught panic is routed
/// (contained, re-raised, logged at what level); that stays a per-site
/// decision. It says whether `text` was written for the `BUG:` convention
/// documented in `docs/PANIC-POLICY.md`: every production-path `expect()`
/// that asserts an invariant FLUI itself must maintain is prefixed
/// `"BUG: "`, so a user hitting it knows immediately the fault is FLUI's,
/// not theirs.
#[must_use]
pub fn is_internal_invariant(text: &str) -> bool {
    text.starts_with("BUG:")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `panic!("literal")` and every formatted `panic!` must survive
    /// `catch_unwind` as their respective payload shapes, and anything else
    /// must come back as an honest `None` rather than a guessed string.
    #[test]
    fn a_panic_payload_keeps_its_message() {
        let borrowed = std::panic::catch_unwind(|| panic!("unsaved work check exploded"))
            .expect_err("the closure panics");
        assert_eq!(
            payload_text(&*borrowed),
            Some("unsaved work check exploded"),
            "a `panic!(\"literal\")` payload is a &'static str"
        );

        let owned = std::panic::catch_unwind(|| panic!("document {} is dirty", 7))
            .expect_err("the closure panics");
        assert_eq!(
            payload_text(&*owned),
            Some("document 7 is dirty"),
            "a formatted `panic!` payload is a String"
        );

        let opaque = std::panic::catch_unwind(|| std::panic::panic_any(41_u8))
            .expect_err("the closure panics");
        assert_eq!(
            payload_text(&*opaque),
            None,
            "a non-string payload has no text to report, and must not be guessed at"
        );
    }

    #[test]
    fn is_internal_invariant_is_true_for_a_bug_prefixed_message() {
        assert!(is_internal_invariant(
            "BUG: RenderId minted by this tree must resolve"
        ));
    }

    #[test]
    fn is_internal_invariant_is_false_for_a_lowercase_prefix() {
        assert!(!is_internal_invariant(
            "bug: wrong case is not the convention"
        ));
    }

    #[test]
    fn is_internal_invariant_is_false_when_bug_is_not_at_the_start() {
        assert!(!is_internal_invariant("x BUG: not a prefix"));
    }
}
