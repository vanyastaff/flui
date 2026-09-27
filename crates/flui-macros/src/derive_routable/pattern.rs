//! Route patterns (`#[route("/note/:id")]`): parsing, shape and specificity.
//!
//! Plain functions over strings, so the rules are unit-tested without a rustc
//! run per case.

use std::cmp::Ordering;

use syn::ext::IdentExt;
use syn::parse::Parser;

/// One segment of a route pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Segment {
    /// Text the location's segment must equal, decoded.
    Literal(String),
    /// `:name`: a segment that fills the field `name`.
    Param(String),
}

/// Parse a route pattern: `/` alone is the root; otherwise `/`-separated
/// segments, each a literal or a `:name` parameter.
///
/// # Errors
///
/// Why the pattern is not one, phrased for a compile error.
pub(crate) fn parse(pattern: &str) -> Result<Vec<Segment>, String> {
    let Some(rest) = pattern.strip_prefix('/') else {
        return Err("a route pattern starts with `/`".to_owned());
    };
    if pattern.contains(['?', '#', '%']) {
        return Err(
            "a route pattern has no `?` query, `#` fragment or `%` escape; write the literal \
             text, and `RoutePath` encodes it"
                .to_owned(),
        );
    }
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    if rest.ends_with('/') {
        return Err("a route pattern has no trailing `/`".to_owned());
    }
    rest.split('/').map(segment).collect()
}

fn segment(text: &str) -> Result<Segment, String> {
    if text.is_empty() {
        return Err("a route pattern has no empty segment".to_owned());
    }
    let Some(name) = text.strip_prefix(':') else {
        return Ok(Segment::Literal(text.to_owned()));
    };
    // `parse_any` admits keywords, so `:type` can fill a field `r#type`.
    match syn::Ident::parse_any.parse_str(name) {
        Ok(ident) if ident == name => Ok(Segment::Param(name.to_owned())),
        _ => Err(format!(
            "`:{name}` is not a parameter: write `:` and the name of a field"
        )),
    }
}

/// Whether two patterns match exactly the same locations: the same length,
/// the same literals in the same places, and parameters in the same places.
pub(crate) fn same_shape(a: &[Segment], b: &[Segment]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (Segment::Literal(x), Segment::Literal(y)) => x == y,
            (Segment::Param(_), Segment::Param(_)) => true,
            _ => false,
        })
}

/// The order patterns are tried in: shorter first, then segment by segment,
/// a literal before a parameter. Only patterns of one length can match one
/// location, so within a length this puts `/s/new` before `/s/:slug`.
pub(crate) fn specificity(a: &[Segment], b: &[Segment]) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| {
        a.iter()
            .zip(b)
            .map(|pair| match pair {
                (Segment::Literal(x), Segment::Literal(y)) => x.cmp(y),
                (Segment::Literal(_), Segment::Param(_)) => Ordering::Less,
                (Segment::Param(_), Segment::Literal(_)) => Ordering::Greater,
                (Segment::Param(_), Segment::Param(_)) => Ordering::Equal,
            })
            .find(|ordering| ordering.is_ne())
            .unwrap_or(Ordering::Equal)
    })
}

/// The first pair `(earlier, later)` of patterns with the same shape.
pub(crate) fn first_conflict(patterns: &[Vec<Segment>]) -> Option<(usize, usize)> {
    (0..patterns.len()).find_map(|later| {
        (0..later)
            .find(|&earlier| same_shape(&patterns[earlier], &patterns[later]))
            .map(|earlier| (earlier, later))
    })
}

/// Indices of `patterns` in the order they are tried.
pub(crate) fn match_order(patterns: &[Vec<Segment>]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..patterns.len()).collect();
    order.sort_by(|&a, &b| specificity(&patterns[a], &patterns[b]));
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(text: &str) -> Segment {
        Segment::Literal(text.to_owned())
    }

    fn param(name: &str) -> Segment {
        Segment::Param(name.to_owned())
    }

    fn parsed(pattern: &str) -> Vec<Segment> {
        parse(pattern).unwrap_or_else(|error| panic!("{pattern:?} parses: {error}"))
    }

    #[test]
    fn parse_reads_literals_and_parameters() {
        assert_eq!(parsed("/"), []);
        assert_eq!(parsed("/note/:id"), [lit("note"), param("id")]);
        assert_eq!(
            parsed("/user/:uid/post/:pid"),
            [lit("user"), param("uid"), lit("post"), param("pid")]
        );
        assert_eq!(parsed("/café/:s"), [lit("café"), param("s")]);
        assert_eq!(parsed("/t/:type"), [lit("t"), param("type")]);
        assert_eq!(parsed("/a:b"), [lit("a:b")]);
    }

    #[test]
    fn parse_rejects_what_is_not_a_pattern() {
        for (pattern, reason) in [
            ("", "starts with `/`"),
            ("note", "starts with `/`"),
            ("//", "trailing `/`"),
            ("/note/", "trailing `/`"),
            ("/a//b", "empty segment"),
            ("/a?b", "`?` query"),
            ("/a#b", "`#` fragment"),
            ("/a%20b", "`%` escape"),
            ("/note/:", "not a parameter"),
            ("/note/:1d", "not a parameter"),
            ("/note/:a-b", "not a parameter"),
        ] {
            let error = parse(pattern).expect_err(pattern);
            assert!(error.contains(reason), "{pattern:?}: {error}");
        }
    }

    #[test]
    fn same_shape_ignores_parameter_names_only() {
        assert!(same_shape(&parsed("/n/:a"), &parsed("/n/:b")));
        assert!(same_shape(&parsed("/"), &parsed("/")));
        assert!(!same_shape(&parsed("/n/:a"), &parsed("/n/new")));
        assert!(!same_shape(&parsed("/n/:a"), &parsed("/m/:a")));
        assert!(!same_shape(&parsed("/n"), &parsed("/n/:a")));
    }

    #[test]
    fn match_order_puts_literals_before_parameters() {
        let patterns = vec![
            parsed("/s/:slug"),
            parsed("/:any/new"),
            parsed("/s/new"),
            parsed("/"),
            parsed("/tag/:name"),
        ];
        let order: Vec<usize> = match_order(&patterns);
        // Root first; `/s/new` before `/s/:slug` although declared after it;
        // a leading literal before a leading parameter.
        assert_eq!(order[0], 3);
        let rank = |index: usize| order.iter().position(|&i| i == index).expect("ranked");
        assert!(rank(2) < rank(0), "{order:?}");
        assert!(rank(0) < rank(1), "{order:?}");
        assert!(rank(4) < rank(1), "{order:?}");
    }

    #[test]
    fn first_conflict_names_the_later_duplicate() {
        let patterns = vec![
            parsed("/"),
            parsed("/n/:a"),
            parsed("/n/new"),
            parsed("/n/:b"),
        ];
        assert_eq!(first_conflict(&patterns), Some((1, 3)));
        assert_eq!(first_conflict(&patterns[..3]), None);
    }
}
