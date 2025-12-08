//! Type-safe IDs for all tree levels
//!
// Allow unsafe code in this module - it's required for NonZeroUsize::new_unchecked
// which provides a const unsafe constructor for performance-critical ID creation.
#![allow(unsafe_code)]
//!
//! Flui uses a 5-tree architecture similar to Flutter:
//! - **View**: Immutable configuration (like Flutter's Widget)
//! - **Element**: Mutable lifecycle management
//! - **Render**: Layout and painting
//! - **Layer**: Compositing and GPU optimization
//! - **Semantics**: Accessibility information for assistive technologies
//!
//! All IDs use `NonZeroUsize` for niche optimization:
//! - `Option<Id>` is same size as `Id` (no extra byte needed)
//! - Prevents 0 from being a valid ID (reserved for sentinel)
//! - IDs are reused after removal (slab behavior)
//!
//! # Design Notes
//!
//! These IDs are indices into `Slab` collections. They remain valid until
//! the corresponding item is removed, at which point the ID may be reused.
//! Always verify an ID is still valid before dereferencing.
//!
//! # Examples
//!
//! ```rust
//! use flui_foundation::{ViewId, ElementId, RenderId, LayerId, SemanticsId};
//!
//! // All IDs have same size as Option<Id> (niche optimization)
//! assert_eq!(
//!     std::mem::size_of::<ElementId>(),
//!     std::mem::size_of::<Option<ElementId>>()
//! );
//!
//! // Create from usize (panics if 0)
//! let element = ElementId::new(1);
//! let render = RenderId::new(2);
//!
//! // Safe creation that returns Option
//! let maybe_id = ViewId::new_checked(0); // None
//! let valid_id = ViewId::new_checked(1); // Some(ViewId(1))
//! ```

use std::num::NonZeroUsize;

// =========================================================================
// Macro for defining ID types
// =========================================================================

macro_rules! define_id {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(transparent)]
        #[must_use = "IDs should be used for tree node identification"]
        $vis struct $name(NonZeroUsize);

        impl $name {
            /// Create a new ID from a non-zero usize.
            ///
            /// # Panics
            ///
            /// Panics if `id` is 0. Zero is reserved for sentinel values
            /// and cannot be used as a valid ID.
            ///
            /// If you need to handle 0, use [`new_checked()`](Self::new_checked) instead.
            #[inline]
            #[track_caller]
            pub fn new(id: usize) -> Self {
                Self(NonZeroUsize::new(id).unwrap_or_else(|| {
                    panic!(
                        "{}::new() called with 0, which is not a valid ID.\n\
                        \n\
                        {} uses NonZeroUsize internally, so 0 is reserved for sentinel values.\n\
                        \n\
                        To handle potentially-zero values, use {}::new_checked() instead:\n\
                        ```\n\
                        match {}::new_checked(id) {{\n\
                            Some(id) => /* use id */,\n\
                            None => /* handle zero case */,\n\
                        }}\n\
                        ```",
                        stringify!($name),
                        stringify!($name),
                        stringify!($name),
                        stringify!($name)
                    )
                }))
            }

            /// Create a new ID from a usize, returning None if 0.
            #[inline]
            pub const fn new_checked(id: usize) -> Option<Self> {
                match NonZeroUsize::new(id) {
                    Some(nz) => Some(Self(nz)),
                    None => None,
                }
            }

            /// Get the inner usize value.
            #[inline]
            pub const fn get(self) -> usize {
                self.0.get()
            }

            /// Create an ID without checking if the value is non-zero.
            ///
            /// # Safety
            ///
            /// The caller must ensure that `id` is not 0.
            #[inline]
            pub const unsafe fn new_unchecked(id: usize) -> Self {
                // SAFETY: Caller must ensure id is non-zero
                unsafe { Self(NonZeroUsize::new_unchecked(id)) }
            }
        }

        // Conversions
        impl From<NonZeroUsize> for $name {
            #[inline]
            fn from(id: NonZeroUsize) -> Self {
                Self(id)
            }
        }

        impl From<$name> for usize {
            #[inline]
            fn from(id: $name) -> usize {
                id.get()
            }
        }

        // Display for debugging
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.get())
            }
        }

        // Arithmetic operations (for bitmap indexing in dirty tracking)
        impl std::ops::Sub<usize> for $name {
            type Output = usize;

            #[inline]
            fn sub(self, rhs: usize) -> usize {
                self.get() - rhs
            }
        }

        impl std::ops::Add<usize> for $name {
            type Output = $name;

            #[inline]
            fn add(self, rhs: usize) -> $name {
                $name::new(self.get() + rhs)
            }
        }

        // Serde support (feature gated)
        #[cfg(feature = "serde")]
        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_u64(self.get() as u64)
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let id = u64::deserialize(deserializer)?;
                if id == 0 {
                    return Err(serde::de::Error::custom(concat!(
                        stringify!($name),
                        " cannot be zero (uses NonZeroUsize internally)"
                    )));
                }

                // Convert to usize (may truncate on 32-bit systems)
                #[allow(clippy::cast_possible_truncation)]
                let id_usize = id as usize;
                if id_usize == 0 {
                    return Err(serde::de::Error::custom(concat!(
                        stringify!($name),
                        " overflowed when converting from u64 to usize"
                    )));
                }

                Ok(Self::new(id_usize))
            }
        }

        // Test-only: Allow creating from usize for convenience
        #[cfg(test)]
        impl From<usize> for $name {
            fn from(id: usize) -> Self {
                Self::new(id)
            }
        }
    };
}

// =========================================================================
// ID Type Definitions
// =========================================================================

define_id! {
    /// View ID - stable index into the View tree
    ///
    /// Views are immutable configuration objects (like Flutter's Widgets).
    /// They describe what the UI should look like but don't contain mutable state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::ViewId;
    ///
    /// let id = ViewId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    pub struct ViewId;
}

define_id! {
    /// Element ID - stable index into the Element tree
    ///
    /// Elements are the mutable counterparts to Views. They manage lifecycle,
    /// hold state between rebuilds, and coordinate updates between Views and `RenderObjects`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::ElementId;
    ///
    /// let id = ElementId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    pub struct ElementId;
}

define_id! {
    /// Render ID - stable index into the `RenderObject` tree
    ///
    /// `RenderObjects` handle layout and painting. They form a separate tree
    /// optimized for performance-critical operations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::RenderId;
    ///
    /// let id = RenderId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    pub struct RenderId;
}

define_id! {
    /// Layer ID - stable index into the Layer tree
    ///
    /// Layers handle compositing and GPU optimization. They're created at
    /// repaint boundaries and cached for efficient rendering.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::LayerId;
    ///
    /// let id = LayerId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    pub struct LayerId;
}

define_id! {
    /// Semantics ID - stable index into the Semantics tree
    ///
    /// `SemanticsNodes` provide accessibility information for screen readers
    /// and other assistive technologies. The semantics tree is built in
    /// parallel with the layer tree during the paint phase.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::SemanticsId;
    ///
    /// let id = SemanticsId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    pub struct SemanticsId;
}

define_id! {
    /// Listener ID - unique identifier for registered listeners
    ///
    /// Used by `ChangeNotifier` and `Listenable` to track registered callbacks.
    /// Each listener gets a unique ID that can be used to remove it later.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::ListenerId;
    ///
    /// let id = ListenerId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    pub struct ListenerId;
}

define_id! {
    /// Observer ID - unique identifier for registered observers
    ///
    /// Used by `ObserverList` to track registered observers.
    /// Similar to `ListenerId` but for the observer pattern.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::ObserverId;
    ///
    /// let id = ObserverId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    pub struct ObserverId;
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_id_type {
        ($mod_name:ident, $name:ident) => {
            mod $mod_name {
                use super::*;

                #[test]
                fn test_new() {
                    let id = $name::new(42);
                    assert_eq!(id.get(), 42);
                }

                #[test]
                #[should_panic]
                fn test_new_zero_panics() {
                    let _ = $name::new(0);
                }

                #[test]
                fn test_new_checked() {
                    assert_eq!($name::new_checked(0), None);
                    assert_eq!($name::new_checked(1).map(|id| id.get()), Some(1));
                    assert_eq!($name::new_checked(42).map(|id| id.get()), Some(42));
                }

                #[test]
                fn test_new_unchecked() {
                    let id = unsafe { $name::new_unchecked(1) };
                    assert_eq!(id.get(), 1);
                }

                #[test]
                fn test_niche_optimization() {
                    // Option<Id> should be same size as Id (niche optimization)
                    assert_eq!(
                        std::mem::size_of::<$name>(),
                        std::mem::size_of::<Option<$name>>()
                    );
                }

                #[test]
                fn test_from_non_zero() {
                    let nz = NonZeroUsize::new(42).unwrap();
                    let id = $name::from(nz);
                    assert_eq!(id.get(), 42);
                }

                #[test]
                fn test_into_usize() {
                    let id = $name::new(42);
                    let value: usize = id.into();
                    assert_eq!(value, 42);
                }

                #[test]
                fn test_display() {
                    let id = $name::new(42);
                    assert_eq!(format!("{}", id), concat!(stringify!($name), "(42)"));
                }

                #[test]
                fn test_arithmetic() {
                    let id = $name::new(10);
                    assert_eq!(id - 5, 5);
                    assert_eq!((id + 5).get(), 15);
                }

                #[test]
                fn test_equality() {
                    let id1 = $name::new(42);
                    let id2 = $name::new(42);
                    let id3 = $name::new(43);

                    assert_eq!(id1, id2);
                    assert_ne!(id1, id3);
                }

                #[test]
                fn test_ordering() {
                    let id1 = $name::new(1);
                    let id2 = $name::new(2);
                    let id3 = $name::new(3);

                    assert!(id1 < id2);
                    assert!(id2 < id3);
                    assert!(id1 < id3);
                }

                #[test]
                fn test_from_usize_in_tests() {
                    let id: $name = 42.into();
                    assert_eq!(id.get(), 42);
                }

                #[cfg(feature = "serde")]
                #[test]
                fn test_serde_roundtrip() {
                    let id = $name::new(42);
                    let json = serde_json::to_string(&id).unwrap();
                    assert_eq!(json, "42");

                    let deserialized: $name = serde_json::from_str(&json).unwrap();
                    assert_eq!(deserialized.get(), 42);
                }

                #[cfg(feature = "serde")]
                #[test]
                fn test_serde_zero_rejection() {
                    let json = "0";
                    let result: Result<$name, _> = serde_json::from_str(json);
                    assert!(result.is_err());
                }
            }
        };
    }

    // Run all tests for each ID type
    test_id_type!(view_id, ViewId);
    test_id_type!(element_id, ElementId);
    test_id_type!(render_id, RenderId);
    test_id_type!(layer_id, LayerId);
    test_id_type!(semantics_id, SemanticsId);

    #[test]
    fn test_all_ids_have_same_size() {
        assert_eq!(
            std::mem::size_of::<ViewId>(),
            std::mem::size_of::<ElementId>()
        );
        assert_eq!(
            std::mem::size_of::<ElementId>(),
            std::mem::size_of::<RenderId>()
        );
        assert_eq!(
            std::mem::size_of::<RenderId>(),
            std::mem::size_of::<LayerId>()
        );
        assert_eq!(
            std::mem::size_of::<LayerId>(),
            std::mem::size_of::<SemanticsId>()
        );
    }

    #[test]
    fn test_ids_are_distinct_types() {
        let view = ViewId::new(1);
        let element = ElementId::new(1);
        let render = RenderId::new(1);
        let layer = LayerId::new(1);
        let semantics = SemanticsId::new(1);

        // These should not compile if uncommented:
        // let _: ViewId = element;
        // let _: ElementId = render;
        // let _: RenderId = layer;
        // let _: LayerId = semantics;

        assert_eq!(view.get(), element.get());
        assert_eq!(element.get(), render.get());
        assert_eq!(render.get(), layer.get());
        assert_eq!(layer.get(), semantics.get());
    }
}
