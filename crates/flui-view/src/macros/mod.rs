//! Declarative macros for the C2 static-tuple authoring path.
//!
//! The [`column!`](crate::column) and [`row!`](crate::row) macros
//! expand to the tuple form
//! `($($e),+,)` that [`ViewSeq`](crate::seq::ViewSeq) is implemented
//! for at arities `0..=16`. They are the ergonomic surface that lets
//! widget authors write
//!
//! ```text
//! Column {
//!     children: column![Greeting { name: "a".into() }, Padding { ... }, Text::new("c")],
//! }
//! ```
//!
//! without a manual `(…,)` trailing-comma tuple shape and without
//! the `vec![…]` / `.boxed()` ritual the dynamic-fallback path
//! requires.
//!
//! ## Authoring cliff at 17 children
//!
//! `ViewSeq` ships tuple impls for arities `0..=16` (Rust stdlib
//! cap). Authors who hand `column!` more than 16 children hit a
//! macro arm with `compile_error!` per FR-034 — the diagnostic
//! names the cliff, points to the `vec![child.boxed(), …]`
//! fallback, AND names the monomorphism cost so the author can
//! weigh the tradeoff before reaching for the bigger path.
//!
//! ## `column!` vs `row!`
//!
//! Both macros expand to the same tuple form at this layer — the
//! distinction is authoring-readability symmetry with the widget
//! call site (`Column { children: column![…] }` vs `Row { children:
//! row![…] }`). The widget itself decides whether the children flow
//! vertically (`Column`) or horizontally (`Row`); the macro is just
//! the input shape.

/// Build a heterogeneous tuple of children for a `Column` widget.
///
/// See [module docs](self) for the authoring shape and the
/// >16-children cliff.
#[macro_export]
macro_rules! column {
    () => {
        ()
    };
    ($e0:expr $(,)?) => {
        ($e0,)
    };
    ($e0:expr, $e1:expr $(,)?) => {
        ($e0, $e1)
    };
    ($e0:expr, $e1:expr, $e2:expr $(,)?) => {
        ($e0, $e1, $e2)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr $(,)?) => {
        ($e0, $e1, $e2, $e3)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr, $e4:expr $(,)?) => {
        ($e0, $e1, $e2, $e3, $e4)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr, $e4:expr, $e5:expr $(,)?) => {
        ($e0, $e1, $e2, $e3, $e4, $e5)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr, $e4:expr, $e5:expr, $e6:expr $(,)?) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr, $e4:expr, $e5:expr, $e6:expr, $e7:expr $(,)?) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr $(,)?
    ) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr $(,)?
    ) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr $(,)?
    ) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr $(,)?
    ) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr,
        $e12:expr $(,)?
    ) => {
        (
            $e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11, $e12,
        )
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr,
        $e12:expr, $e13:expr $(,)?
    ) => {
        (
            $e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11, $e12, $e13,
        )
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr,
        $e12:expr, $e13:expr, $e14:expr $(,)?
    ) => {
        (
            $e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11, $e12, $e13, $e14,
        )
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr,
        $e12:expr, $e13:expr, $e14:expr, $e15:expr $(,)?
    ) => {
        (
            $e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11, $e12, $e13, $e14, $e15,
        )
    };
    // Catch-all (17+ args): friendly FR-034 diagnostic. The author
    // sees this instead of a bare `the trait bound (A,B,...,Q):
    // ViewSeq is not satisfied` rustc note.
    ($($e:expr),+ $(,)?) => {
        ::core::compile_error!(
            "column!: more than 16 children exceeds the tuple ViewSeq cap of 16 \
             — use vec![child.boxed(), ...] for >16 children. The Vec<BoxedView> \
             path is monomorphism-free (per-child dyn-dispatch); consider splitting \
             into nested sub-tuples (e.g., column![column![a..p], column![q..]]) \
             to retain static-tuple monomorphism for sub-segments."
        )
    };
}

/// Build a heterogeneous tuple of children for a `Row` widget.
///
/// Expands to the same tuple form as [`column!`] — see that macro
/// for the authoring shape and the >16-children cliff. The naming
/// distinction is widget-readability symmetry (`Row { children:
/// row![…] }`), not a different runtime shape.
#[macro_export]
macro_rules! row {
    () => {
        ()
    };
    ($e0:expr $(,)?) => {
        ($e0,)
    };
    ($e0:expr, $e1:expr $(,)?) => {
        ($e0, $e1)
    };
    ($e0:expr, $e1:expr, $e2:expr $(,)?) => {
        ($e0, $e1, $e2)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr $(,)?) => {
        ($e0, $e1, $e2, $e3)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr, $e4:expr $(,)?) => {
        ($e0, $e1, $e2, $e3, $e4)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr, $e4:expr, $e5:expr $(,)?) => {
        ($e0, $e1, $e2, $e3, $e4, $e5)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr, $e4:expr, $e5:expr, $e6:expr $(,)?) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6)
    };
    ($e0:expr, $e1:expr, $e2:expr, $e3:expr, $e4:expr, $e5:expr, $e6:expr, $e7:expr $(,)?) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr $(,)?
    ) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr $(,)?
    ) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr $(,)?
    ) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr $(,)?
    ) => {
        ($e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11)
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr,
        $e12:expr $(,)?
    ) => {
        (
            $e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11, $e12,
        )
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr,
        $e12:expr, $e13:expr $(,)?
    ) => {
        (
            $e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11, $e12, $e13,
        )
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr,
        $e12:expr, $e13:expr, $e14:expr $(,)?
    ) => {
        (
            $e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11, $e12, $e13, $e14,
        )
    };
    (
        $e0:expr, $e1:expr, $e2:expr, $e3:expr,
        $e4:expr, $e5:expr, $e6:expr, $e7:expr,
        $e8:expr, $e9:expr, $e10:expr, $e11:expr,
        $e12:expr, $e13:expr, $e14:expr, $e15:expr $(,)?
    ) => {
        (
            $e0, $e1, $e2, $e3, $e4, $e5, $e6, $e7, $e8, $e9, $e10, $e11, $e12, $e13, $e14, $e15,
        )
    };
    // Catch-all (17+ args): same friendly FR-034 diagnostic, named
    // `row!` so the rustc error points at the right macro.
    ($($e:expr),+ $(,)?) => {
        ::core::compile_error!(
            "row!: more than 16 children exceeds the tuple ViewSeq cap of 16 \
             — use vec![child.boxed(), ...] for >16 children. The Vec<BoxedView> \
             path is monomorphism-free (per-child dyn-dispatch); consider splitting \
             into nested sub-tuples (e.g., row![row![a..p], row![q..]]) \
             to retain static-tuple monomorphism for sub-segments."
        )
    };
}

#[cfg(test)]
mod tests {
    use crate::seq::ViewSeq;

    #[derive(Clone)]
    struct Leaf(u32);

    impl crate::view::View for Leaf {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    impl crate::view::StatelessView for Leaf {
        fn build(&self, _ctx: &dyn crate::context::BuildContext) -> impl crate::view::IntoView {
            Leaf(self.0)
        }
    }

    #[test]
    fn column_zero_is_unit() {
        let s: () = column![];
        assert_eq!(<() as ViewSeq>::len(&s), 0);
    }

    #[test]
    fn column_three_heterogeneous_kids_compile() {
        // The macro should accept three children of three different
        // concrete types (only Leaf used here for self-containment).
        let s = column![Leaf(1), Leaf(2), Leaf(3)];
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn row_three_heterogeneous_kids_compile() {
        let s = row![Leaf(1), Leaf(2), Leaf(3)];
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn column_trailing_comma_accepted() {
        let s = column![Leaf(1), Leaf(2),];
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn column_sixteen_at_cap() {
        let s = column![
            Leaf(0),
            Leaf(1),
            Leaf(2),
            Leaf(3),
            Leaf(4),
            Leaf(5),
            Leaf(6),
            Leaf(7),
            Leaf(8),
            Leaf(9),
            Leaf(10),
            Leaf(11),
            Leaf(12),
            Leaf(13),
            Leaf(14),
            Leaf(15),
        ];
        assert_eq!(s.len(), 16);
    }
}
