//! One list per vocabulary.
//!
//! The `vocabulary!` macro expands a single list of `Variant => "name"` entries into
//! the enum, its `ALL` slice and its `name()` match. A variant cannot be
//! declared without also being listed and named, which is what ADR-0089 §3
//! asks of an enumerable vocabulary: every consumer that walks `ALL` sees every
//! variant, with no hand-maintained second list to fall behind.

/// Declares a `#[non_exhaustive]` vocabulary enum with `ALL` and `name()`.
///
/// Each entry is `Variant => "name"`, or `Variant = discriminant => "name"`
/// for an enum with an explicit `repr`. Attributes on an entry (docs,
/// `#[default]`) are kept on the variant.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident $(= $value:expr)? => $wire:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[non_exhaustive]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant $(= $value)?,
            )+
        }

        impl $name {
            /// Every variant, once, in declaration order.
            ///
            /// Generated from the same list as the enum, so it cannot miss a
            /// variant or list one twice.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The variant's name in this vocabulary.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)+
                }
            }
        }
    };
}
