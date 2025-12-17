//! Type-safe IDs for all tree levels using marker trait pattern.
//!
//! This module provides a generic `Id<T>` type with marker traits for type-safe
//! identification across different subsystems. Inspired by wgpu's ID system.
//!
//! # Architecture
//!
//! ```text
//! RawId (NonZeroUsize) ─► Id<T: Marker> ─► ViewId, ElementId, etc.
//! ```
//!
//! # Design Notes
//!
//! - All IDs use `NonZeroUsize` for niche optimization (`Option<Id>` = `Id` size)
//! - Marker traits provide type safety between different ID domains
//! - IDs are indices into `Slab` collections (valid until item removed)
//!
//! # Examples
//!
//! ```rust
//! use flui_foundation::{ViewId, ElementId, RenderId};
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
#![allow(unsafe_code)]

use crate::WasmNotSendSync;
use core::{
    cmp::Ordering,
    fmt::{self, Debug, Display},
    hash::{Hash, Hasher},
    marker::PhantomData,
    num::NonZeroUsize,
};

// =========================================================================
// Compile-time size assertions
// =========================================================================

const _: () = {
    // RawId must be pointer-sized for efficient passing
    assert!(size_of::<RawId>() == size_of::<usize>());
};

const _: () = {
    // Option<RawId> must have same size (niche optimization)
    assert!(size_of::<RawId>() == size_of::<Option<RawId>>());
};

// =========================================================================
// Index type alias (for slab indices)
// =========================================================================

/// Index type for slab-based storage.
///
/// This is the raw index value before being wrapped in `RawId`.
pub type Index = usize;

// =========================================================================
// RawId - The underlying representation
// =========================================================================

/// The raw underlying representation of an identifier.
///
/// Uses `NonZeroUsize` for niche optimization - `Option<RawId>` has the same
/// size as `RawId` because the compiler uses 0 as the `None` representation.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawId(NonZeroUsize);

impl RawId {
    /// Zip an index into a RawId.
    ///
    /// # Panics
    ///
    /// Panics if `index` is 0 (reserved for sentinel/None).
    #[inline]
    #[track_caller]
    pub fn zip(index: Index) -> Self {
        Self(NonZeroUsize::new(index).expect("ID index must be non-zero"))
    }

    /// Unzip a RawId back to its index.
    #[inline]
    pub const fn unzip(self) -> Index {
        self.0.get()
    }

    /// Creates a RawId without checking for zero.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index` is not 0.
    #[inline]
    pub const unsafe fn zip_unchecked(index: Index) -> Self {
        // SAFETY: Caller guarantees index is non-zero
        Self(NonZeroUsize::new_unchecked(index))
    }

    /// Creates a RawId, returning `None` if index is 0.
    #[inline]
    pub const fn try_zip(index: Index) -> Option<Self> {
        match NonZeroUsize::new(index) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }
}

impl Debug for RawId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RawId({})", self.unzip())
    }
}

impl Display for RawId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.unzip())
    }
}

impl From<NonZeroUsize> for RawId {
    #[inline]
    fn from(value: NonZeroUsize) -> Self {
        Self(value)
    }
}

impl From<RawId> for Index {
    #[inline]
    fn from(id: RawId) -> Self {
        id.unzip()
    }
}

// =========================================================================
// Marker trait
// =========================================================================

/// Marker trait for ID type discrimination.
///
/// Each resource type defines its own marker, ensuring that IDs for different
/// resources cannot be confused. The marker is a zero-sized type that exists
/// only at compile time.
///
/// Uses `WasmNotSendSync` for WASM compatibility - on native requires `Send + Sync`,
/// on WASM (single-threaded) has no thread-safety requirements.
///
/// # Example
///
/// ```rust
/// use flui_foundation::Marker;
///
/// // Define a custom marker for a new resource type
/// #[derive(Debug)]
/// pub enum CustomMarker {}
/// impl Marker for CustomMarker {}
/// ```
pub trait Marker: 'static + WasmNotSendSync + Debug {}

// =========================================================================
// Id<T> - The generic typed identifier
// =========================================================================

/// A type-safe identifier for a specific resource type.
///
/// `Id<T>` wraps a `RawId` with a marker type `T` that ensures IDs for different
/// resource types cannot be mixed up at compile time.
///
/// # Type Safety
///
/// ```compile_fail
/// use flui_foundation::{ViewId, ElementId};
///
/// let view_id = ViewId::new(1);
/// let element_id: ElementId = view_id; // Compile error!
/// ```
///
/// # Examples
///
/// ```rust
/// use flui_foundation::{ViewId, ElementId};
///
/// let view = ViewId::zip(1);
/// let element = ElementId::zip(1);
///
/// // Same underlying value, but different types
/// assert_eq!(view.unzip(), element.unzip());
/// // assert_eq!(view, element); // Would not compile!
/// ```
#[repr(transparent)]
pub struct Id<T: Marker>(RawId, PhantomData<T>);

impl<T: Marker> Id<T> {
    /// Creates an ID from a raw ID.
    ///
    /// # Safety
    ///
    /// The caller must ensure the raw ID is valid for this marker type.
    #[inline]
    pub const unsafe fn from_raw(raw: RawId) -> Self {
        Self(raw, PhantomData)
    }

    /// Coerce the identifier into its raw underlying representation.
    #[inline]
    pub const fn into_raw(self) -> RawId {
        self.0
    }

    /// Zip an index into an Id.
    ///
    /// # Panics
    ///
    /// Panics if `index` is 0.
    #[inline]
    #[track_caller]
    pub fn zip(index: Index) -> Self {
        Self(RawId::zip(index), PhantomData)
    }

    /// Unzip an Id back to its index.
    #[inline]
    pub const fn unzip(self) -> Index {
        self.0.unzip()
    }

    /// Creates an ID without checking for zero.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index` is not 0.
    #[inline]
    pub const unsafe fn zip_unchecked(index: Index) -> Self {
        // SAFETY: Caller guarantees index is non-zero
        Self(RawId::zip_unchecked(index), PhantomData)
    }

    /// Creates an ID, returning `None` if index is 0.
    #[inline]
    pub const fn try_zip(index: Index) -> Option<Self> {
        match RawId::try_zip(index) {
            Some(raw) => Some(Self(raw, PhantomData)),
            None => None,
        }
    }

    // =========================================================================
    // Convenience aliases (for easier migration from old API)
    // =========================================================================

    /// Alias for `zip` - creates an ID from an index.
    #[inline]
    #[track_caller]
    pub fn new(index: Index) -> Self {
        Self::zip(index)
    }

    /// Alias for `unzip` - returns the index.
    #[inline]
    pub const fn get(self) -> Index {
        self.unzip()
    }

    /// Alias for `try_zip` - creates an ID if index is non-zero.
    #[inline]
    pub const fn new_checked(index: Index) -> Option<Self> {
        Self::try_zip(index)
    }

    /// Alias for `zip_unchecked`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index` is not 0.
    #[inline]
    pub const unsafe fn new_unchecked(index: Index) -> Self {
        // SAFETY: Caller guarantees index is non-zero
        Self::zip_unchecked(index)
    }
}

// Manual trait implementations to avoid requiring T: Trait bounds

impl<T: Marker> Copy for Id<T> {}

impl<T: Marker> Clone for Id<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Marker> Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = core::any::type_name::<T>();
        let marker_name = type_name.rsplit("::").next().unwrap_or(type_name);
        write!(f, "Id<{}>({})", marker_name, self.unzip())
    }
}

impl<T: Marker> Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = core::any::type_name::<T>();
        let marker_name = type_name.rsplit("::").next().unwrap_or(type_name);
        write!(f, "{}({})", marker_name, self.unzip())
    }
}

impl<T: Marker> Hash for Id<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T: Marker> PartialEq for Id<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Marker> Eq for Id<T> {}

impl<T: Marker> PartialOrd for Id<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Marker> Ord for Id<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

// Conversions

impl<T: Marker> From<NonZeroUsize> for Id<T> {
    #[inline]
    fn from(value: NonZeroUsize) -> Self {
        Self(RawId::from(value), PhantomData)
    }
}

impl<T: Marker> From<Id<T>> for Index {
    #[inline]
    fn from(id: Id<T>) -> Self {
        id.unzip()
    }
}

impl<T: Marker> From<Id<T>> for RawId {
    #[inline]
    fn from(id: Id<T>) -> Self {
        id.0
    }
}

// Arithmetic operations (for bitmap indexing in dirty tracking)

impl<T: Marker> core::ops::Sub<Index> for Id<T> {
    type Output = Index;

    #[inline]
    fn sub(self, rhs: Index) -> Index {
        self.unzip() - rhs
    }
}

impl<T: Marker> core::ops::Add<Index> for Id<T> {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Index) -> Self {
        Self::zip(self.unzip() + rhs)
    }
}

// Test-only: Allow creating from usize for convenience
#[cfg(test)]
impl<T: Marker> From<Index> for Id<T> {
    fn from(index: Index) -> Self {
        Self::zip(index)
    }
}

// =========================================================================
// Identifier trait alias (for backwards compatibility with flui-tree)
// =========================================================================

/// Trait alias for ID types usable in tree structures.
///
/// This provides a convenient bound for generic tree algorithms that need
/// to work with any ID type (`ViewId`, `ElementId`, `RenderId`, etc.).
///
/// All `Id<T: Marker>` types automatically implement this trait.
///
/// # Example
///
/// ```rust
/// use flui_foundation::{Identifier, ElementId, ViewId};
///
/// fn process_id<I: Identifier>(id: I) -> usize {
///     id.into()
/// }
///
/// assert_eq!(process_id(ElementId::zip(42)), 42);
/// assert_eq!(process_id(ViewId::zip(99)), 99);
/// ```
pub trait Identifier:
    Copy
    + Clone
    + Eq
    + PartialEq
    + Ord
    + PartialOrd
    + Hash
    + Debug
    + Display
    + Into<Index>
    + Send
    + Sync
    + 'static
{
    /// Returns the underlying index value.
    fn get(self) -> Index;

    /// Creates an ID from an index, panics if zero.
    fn zip(index: Index) -> Self;

    /// Creates an ID from an index, returns None if zero.
    fn try_zip(index: Index) -> Option<Self>;
}

// Blanket implementation for all Id<T> types
impl<T: Marker> Identifier for Id<T> {
    #[inline]
    fn get(self) -> Index {
        self.unzip()
    }

    #[inline]
    fn zip(index: Index) -> Self {
        Id::zip(index)
    }

    #[inline]
    fn try_zip(index: Index) -> Option<Self> {
        Id::try_zip(index)
    }
}

// =========================================================================
// Marker types and type aliases
// =========================================================================

/// Define marker types and ID type aliases.
macro_rules! ids {
    ($(
        $(#[$meta:meta])*
        pub type $name:ident $marker:ident;
    )*) => {
        /// Marker types for each resource.
        ///
        /// These are zero-sized enum types that exist only at compile time
        /// to provide type safety between different ID domains.
        pub mod markers {
            $(
                #[doc = concat!("Marker type for [`", stringify!($name), "`](super::", stringify!($name), ").")]
                #[derive(Debug)]
                pub enum $marker {}
                impl super::Marker for $marker {}
            )*
        }

        $(
            $(#[$meta])*
            pub type $name = Id<markers::$marker>;
        )*
    }
}

ids! {
    // =====================================================================
    // Core Tree IDs (5-tree architecture)
    // =====================================================================

    /// View ID - index into the View tree.
    ///
    /// Views are immutable configuration objects (like Flutter's Widgets).
    /// They describe what the UI should look like but don't contain mutable state.
    pub type ViewId View;

    /// Element ID - index into the Element tree.
    ///
    /// Elements are the mutable counterparts to Views. They manage lifecycle,
    /// hold state between rebuilds, and coordinate updates.
    pub type ElementId Element;

    /// Render ID - index into the RenderObject tree.
    ///
    /// RenderObjects handle layout and painting. They form a separate tree
    /// optimized for performance-critical operations.
    pub type RenderId Render;

    /// Layer ID - index into the Layer tree.
    ///
    /// Layers handle compositing and GPU optimization. Created at repaint
    /// boundaries and cached for efficient rendering.
    pub type LayerId Layer;

    /// Semantics ID - index into the Semantics tree.
    ///
    /// SemanticsNodes provide accessibility information for screen readers
    /// and other assistive technologies.
    pub type SemanticsId Semantics;

    // =====================================================================
    // Listener/Observer IDs
    // =====================================================================

    /// Listener ID - identifier for registered listeners.
    ///
    /// Used by `ChangeNotifier` and `Listenable` to track registered callbacks.
    pub type ListenerId Listener;

    /// Observer ID - identifier for registered observers.
    ///
    /// Used by `ObserverList` to track registered observers.
    pub type ObserverId Observer;

    // =====================================================================
    // Platform/System IDs (from Flutter)
    // =====================================================================

    /// Texture ID - GPU texture identifier.
    ///
    /// References a texture registered with the engine for external content
    /// like video frames or platform views.
    pub type TextureId Texture;

    /// Platform View ID - native view identifier.
    ///
    /// References a native platform view embedded in the Flutter view hierarchy.
    pub type PlatformViewId PlatformView;

    /// Device ID - input device identifier.
    ///
    /// Identifies different input devices (keyboards, mice, touch screens).
    pub type DeviceId Device;

    /// Pointer ID - pointer/touch identifier.
    ///
    /// Tracks individual touch points or pointer devices in multi-touch scenarios.
    pub type PointerId Pointer;

    /// Embedder ID - embedder identifier.
    ///
    /// Identifies the embedder that generated an event (for multi-view scenarios).
    pub type EmbedderId Embedder;

    // =====================================================================
    // Keyboard IDs
    // =====================================================================

    /// Key ID - logical keyboard key identifier.
    ///
    /// Represents a logical key independent of physical keyboard layout.
    pub type KeyId Key;

    /// Vendor ID - hardware vendor identifier.
    ///
    /// Identifies the manufacturer of an input device.
    pub type VendorId Vendor;

    /// Product ID - hardware product identifier.
    ///
    /// Identifies a specific product model from a vendor.
    pub type ProductId Product;

    // =====================================================================
    // Navigation/Restoration IDs
    // =====================================================================

    /// Restoration Scope ID - state restoration scope identifier.
    ///
    /// Used for state restoration to track restoration scopes across app restarts.
    pub type RestorationScopeId RestorationScope;

    /// Route ID - navigation route identifier.
    ///
    /// Identifies a route in the navigation stack.
    pub type RouteId Route;

    // =====================================================================
    // Animation/Scheduler IDs
    // =====================================================================

    /// Animation ID - animation frame callback identifier.
    ///
    /// Returned when scheduling frame callbacks, used to cancel them.
    pub type AnimationId Animation;

    /// Frame Callback ID - scheduler frame callback identifier.
    ///
    /// Identifies scheduled frame callbacks in the scheduler binding.
    pub type FrameCallbackId FrameCallback;

    // =====================================================================
    // Gesture IDs
    // =====================================================================

    /// Motion Event ID - Android motion event identifier.
    ///
    /// Identifies motion events on Android platform for gesture handling.
    pub type MotionEventId MotionEvent;

    /// Gesture ID - gesture recognizer identifier.
    ///
    /// Identifies active gesture recognizers during gesture disambiguation.
    pub type GestureId Gesture;

    // =====================================================================
    // Group/Region IDs
    // =====================================================================

    /// Group ID - tap region group identifier.
    ///
    /// Groups tap regions together for coordinated tap handling.
    pub type GroupId Group;

    /// Focus ID - focus node identifier.
    ///
    /// Identifies nodes in the focus tree for keyboard focus management.
    pub type FocusId Focus;

    // =====================================================================
    // Debug/Inspector IDs
    // =====================================================================

    /// Diagnostics ID - widget inspector diagnostics identifier.
    ///
    /// Used by the widget inspector for object identification and tracking.
    pub type DiagnosticsId Diagnostics;

    /// Location ID - source location identifier.
    ///
    /// Identifies source code locations for debugging and error reporting.
    pub type LocationId Location;
}

// =========================================================================
// Serde support
// =========================================================================

#[cfg(feature = "serde")]
mod serde_impl {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for RawId {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.unzip().serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for RawId {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let index = Index::deserialize(deserializer)?;
            RawId::try_zip(index)
                .ok_or_else(|| serde::de::Error::custom("ID index must be non-zero"))
        }
    }

    impl<T: Marker> Serialize for Id<T> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.0.serialize(serializer)
        }
    }

    impl<'de, T: Marker> Deserialize<'de> for Id<T> {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let raw = RawId::deserialize(deserializer)?;
            // SAFETY: We just validated the RawId is non-zero
            Ok(unsafe { Self::from_raw(raw) })
        }
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_id_basics() {
        let id = RawId::zip(42);
        assert_eq!(id.unzip(), 42);
    }

    #[test]
    #[should_panic(expected = "non-zero")]
    fn test_raw_id_zero_panics() {
        let _ = RawId::zip(0);
    }

    #[test]
    fn test_raw_id_try_zip() {
        assert!(RawId::try_zip(0).is_none());
        assert_eq!(RawId::try_zip(42).map(|id| id.unzip()), Some(42));
    }

    #[test]
    fn test_id_basics() {
        let id = ViewId::zip(42);
        assert_eq!(id.unzip(), 42);
    }

    #[test]
    #[should_panic]
    fn test_id_zero_panics() {
        let _ = ElementId::zip(0);
    }

    #[test]
    fn test_id_try_zip() {
        assert!(RenderId::try_zip(0).is_none());
        assert_eq!(LayerId::try_zip(42).map(|id| id.unzip()), Some(42));
    }

    #[test]
    fn test_niche_optimization() {
        // Option<Id> should be same size as Id
        assert_eq!(size_of::<ViewId>(), size_of::<Option<ViewId>>());
        assert_eq!(size_of::<ElementId>(), size_of::<Option<ElementId>>());
        assert_eq!(size_of::<RawId>(), size_of::<Option<RawId>>());
    }

    #[test]
    fn test_all_ids_same_size() {
        let size = size_of::<ViewId>();
        assert_eq!(size_of::<ElementId>(), size);
        assert_eq!(size_of::<RenderId>(), size);
        assert_eq!(size_of::<LayerId>(), size);
        assert_eq!(size_of::<SemanticsId>(), size);
        assert_eq!(size_of::<TextureId>(), size);
        assert_eq!(size_of::<PlatformViewId>(), size);
    }

    #[test]
    fn test_type_safety() {
        let view = ViewId::zip(1);
        let element = ElementId::zip(1);

        // Same underlying value
        assert_eq!(view.unzip(), element.unzip());

        // But different types (this would not compile):
        // assert_eq!(view, element);
    }

    #[test]
    fn test_debug_format() {
        let id = ViewId::zip(42);
        let debug = format!("{:?}", id);
        assert!(debug.contains("View"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn test_display_format() {
        let id = ElementId::zip(42);
        let display = format!("{}", id);
        assert!(display.contains("Element"));
        assert!(display.contains("42"));
    }

    #[test]
    fn test_arithmetic() {
        let id = ViewId::zip(10);
        assert_eq!(id - 5, 5);
        assert_eq!((id + 5).unzip(), 15);
    }

    #[test]
    fn test_ordering() {
        let id1 = RenderId::zip(1);
        let id2 = RenderId::zip(2);
        let id3 = RenderId::zip(3);

        assert!(id1 < id2);
        assert!(id2 < id3);
        assert!(id1 < id3);
    }

    #[test]
    fn test_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(ViewId::zip(1));
        set.insert(ViewId::zip(2));
        set.insert(ViewId::zip(1)); // Duplicate

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_raw_conversion() {
        let id = ViewId::zip(42);
        let raw = id.into_raw();
        assert_eq!(raw.unzip(), 42);

        let recovered = unsafe { ViewId::from_raw(raw) };
        assert_eq!(recovered, id);
    }

    #[test]
    fn test_convenience_aliases() {
        // new/get are aliases for zip/unzip
        let id = ViewId::new(42);
        assert_eq!(id.get(), 42);
        assert_eq!(id.get(), id.unzip());

        // new_checked is alias for try_zip
        assert!(ViewId::new_checked(0).is_none());
        assert_eq!(ViewId::new_checked(42).map(|id| id.get()), Some(42));
    }

    #[test]
    fn test_new_id_types() {
        // Test some of the new Flutter-inspired ID types
        let texture = TextureId::zip(1);
        let platform_view = PlatformViewId::zip(2);
        let device = DeviceId::zip(3);
        let pointer = PointerId::zip(4);

        assert_eq!(texture.unzip(), 1);
        assert_eq!(platform_view.unzip(), 2);
        assert_eq!(device.unzip(), 3);
        assert_eq!(pointer.unzip(), 4);
    }
}
