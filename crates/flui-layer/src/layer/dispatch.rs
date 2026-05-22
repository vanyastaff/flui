//! Generated `is_*` / `as_*` / `as_*_mut` accessors for the [`Layer`] enum.
//!
//! Mythos Step 4 collapsed ~600 LOC of hand-written boilerplate (19 variants
//! × 3 methods = 57 nearly-identical methods) into a single
//! `gen_layer_accessors!` macro invocation defined here.
//!
//! Composite predicates (`Layer::is_clip`, `Layer::is_linking`,
//! `Layer::is_opaque`) and the semantic methods (`bounds`,
//! `needs_compositing`) stay hand-written on [`Layer`] in `layer/mod.rs`
//! because they pattern-match across multiple variants and would obscure
//! the macro form.
//!
//! [`Layer`]: crate::layer::Layer

/// Generates `is_<snake>`, `as_<snake>`, and `as_<snake>_mut` accessor
/// methods on the [`Layer`] enum for each `(Variant, Type, snake_name)`
/// triple.
///
/// The macro emits one `impl Layer` block containing the 3N methods so the
/// rustdoc renders alongside the enum definition.
///
/// # Example
///
/// ```ignore
/// gen_layer_accessors! {
///     Canvas => CanvasLayer, canvas;
///     Picture => PictureLayer, picture;
///     // ...
/// }
/// ```
///
/// expands to:
///
/// ```ignore
/// impl Layer {
///     #[inline]
///     pub fn is_canvas(&self) -> bool {
///         matches!(self, Layer::Canvas(_))
///     }
///     #[inline]
///     pub fn as_canvas(&self) -> Option<&CanvasLayer> {
///         match self { Layer::Canvas(l) => Some(l), _ => None }
///     }
///     #[inline]
///     pub fn as_canvas_mut(&mut self) -> Option<&mut CanvasLayer> {
///         match self { Layer::Canvas(l) => Some(l), _ => None }
///     }
///     // ...
/// }
/// ```
macro_rules! gen_layer_accessors {
    ( $( $variant:ident => $ty:ty, $is_fn:ident, $as_fn:ident, $as_mut_fn:ident );+ $(;)? ) => {
        impl crate::layer::Layer {
            $(
                #[doc = concat!("Returns `true` if this is a [`Layer::", stringify!($variant), "`].")]
                #[inline]
                pub fn $is_fn(&self) -> bool {
                    matches!(self, crate::layer::Layer::$variant(_))
                }

                #[doc = concat!("Returns a shared reference to the inner [`",
                    stringify!($ty), "`] if this is a [`Layer::", stringify!($variant), "`].")]
                ///
                /// The accessor returns `&T` for both inline `Variant(T)` and
                /// boxed `Variant(Box<T>)` storage forms via the
                /// [`std::borrow::Borrow`] trait — `Box<T>: Borrow<T>` and the
                /// blanket `T: Borrow<T>` impl let callers ignore the boxing
                /// decision.
                #[inline]
                pub fn $as_fn(&self) -> Option<&$ty> {
                    match self {
                        crate::layer::Layer::$variant(layer) => Some(
                            <_ as ::std::borrow::Borrow<$ty>>::borrow(layer),
                        ),
                        _ => None,
                    }
                }

                #[doc = concat!("Returns a mutable reference to the inner [`",
                    stringify!($ty), "`] if this is a [`Layer::", stringify!($variant), "`].")]
                ///
                /// Mirrors the [`std::borrow::BorrowMut`] indirection from the
                /// shared accessor so inline and boxed variants share one
                /// signature.
                #[inline]
                pub fn $as_mut_fn(&mut self) -> Option<&mut $ty> {
                    match self {
                        crate::layer::Layer::$variant(layer) => Some(
                            <_ as ::std::borrow::BorrowMut<$ty>>::borrow_mut(layer),
                        ),
                        _ => None,
                    }
                }
            )+
        }
    };
}

pub(crate) use gen_layer_accessors;

// The actual macro invocation lives in `layer/mod.rs` next to the enum
// definition so the generated methods render in the enum's rustdoc.
