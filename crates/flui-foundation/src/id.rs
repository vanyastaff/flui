//! Type-safe IDs for all tree levels using marker trait pattern.
//!
//! This module provides a generic `Id<T>` type with marker traits for type-safe
//! identification across different subsystems. Inspired by wgpu's ID system.
//!
//! # Architecture
//!
//! ```text
//! RawId (NonZeroUsize) ─► Id<T: Marker> ─► ViewId, RenderId, etc.
//! ElementId (NonZeroU64, packs u32 index + NonZeroU32 generation) ─► generational arena key
//! ```
//!
//! # Design Notes
//!
//! - `Id<T>` uses `NonZeroUsize` for niche optimization (`Option<Id<T>>` = `Id<T>` size)
//! - `ElementId` is a DISTINCT generational type: packs `(generation << 32) | index` into a
//!   `NonZeroU64`. The all-zero pattern stays forbidden, so `Option<ElementId>` niche is preserved.
//! - `ElementId` does NOT implement `Identifier` (which exposes `get()->Index`, stripping the
//!   generation). Use `.index()` + `.generation()` for arena operations.
//! - Marker traits provide type safety between different ID domains
//! - Non-element IDs are indices into `Slab` collections (valid until item removed)
//!
//! # Examples
//!
//! ```rust
//! use std::num::NonZeroU32;
//! use flui_foundation::{ElementId, RenderId, ViewId};
//!
//! // ElementId niche optimization: Option<ElementId> == size of ElementId
//! assert_eq!(
//!     std::mem::size_of::<ElementId>(),
//!     std::mem::size_of::<Option<ElementId>>()
//! );
//!
//! // Create ElementId from 1-based index (preserves legacy call sites)
//! let element = ElementId::new(1);
//! assert_eq!(element.index(), 0); // 0-based slab index
//!
//! // Create from explicit slot + generation
//! let generation = NonZeroU32::new(1).unwrap();
//! let element = ElementId::new_gen(0, generation);
//! assert_eq!(element.index(), 0);
//! assert_eq!(element.generation(), generation);
//!
//! // Other IDs create from usize (panics if 0)
//! let render = RenderId::new(2);
//!
//! // Safe creation that returns Option
//! let maybe_id = ViewId::new_checked(0); // None
//! let valid_id = ViewId::new_checked(1); // Some(ViewId(1))
//! ```
// `#[expect]` over `#[allow]` (edition-2024 idiom): this module
// still contains genuine `unsafe` (the `*_unchecked` zip/new
// constructors wrap `NonZeroUsize::new_unchecked`, a documented
// caller-guarantees-non-zero contract). The `expect` will fire an
// "unfulfilled expectation" lint the day the last `unsafe` block is
// removed, prompting deletion of this attribute.
#![expect(
    unsafe_code,
    reason = "RawId/Id `*_unchecked` constructors use NonZeroUsize::new_unchecked"
)]

use core::{
    cmp::Ordering,
    fmt::{self, Debug, Display},
    hash::{Hash, Hasher},
    marker::PhantomData,
    num::NonZeroUsize,
};

use crate::WasmNotSendSync;

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
pub(crate) type Index = usize;

// =========================================================================
// RawId - The underlying representation
// =========================================================================

/// The raw underlying representation of an identifier.
///
/// Uses `NonZeroUsize` for niche optimization - `Option<RawId>` has the same
/// size as `RawId` because the compiler uses 0 as the `None` representation.
///
/// `RawId` stays part of the public surface (downgrading it to
/// `pub(crate)` was considered but rejected): `Id::from_raw(raw: RawId)` is a
/// safe public constructor with a public doc-test, and downstream tests round-
/// trip through `Id::into_raw() -> RawId` + `RawId::unzip`. The internal
/// `Index` alias has no downstream consumers and remains the only part
/// considered for narrowing.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawId(NonZeroUsize);

impl RawId {
    /// Zip an index into a `RawId`.
    ///
    /// # Panics
    ///
    /// Panics if `index` is 0 (reserved for sentinel/None).
    #[inline]
    #[track_caller]
    #[must_use]
    pub fn zip(index: Index) -> Self {
        Self(NonZeroUsize::new(index).expect("ID index must be non-zero"))
    }

    /// Unzip a `RawId` back to its index.
    #[inline]
    #[must_use]
    pub const fn unzip(self) -> Index {
        self.0.get()
    }

    /// Creates a `RawId` without checking for zero.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index` is not 0.
    ///
    /// Staging: zero workspace callers. This exists to unblock future
    /// slab/arena hot paths that genuinely benefit from skipping the
    /// zero-check. Until a call site lands, the unsafe surface surface
    /// is dead code.
    #[doc(hidden)]
    #[inline]
    #[must_use]
    pub const unsafe fn zip_unchecked(index: Index) -> Self {
        // SAFETY: Caller guarantees index is non-zero
        unsafe { Self(NonZeroUsize::new_unchecked(index)) }
    }

    /// Creates a `RawId`, returning `None` if index is 0.
    #[inline]
    #[must_use]
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
/// Uses `WasmNotSendSync` for WASM compatibility - on native requires `Send +
/// Sync`, on WASM (single-threaded) has no thread-safety requirements.
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
/// `Id<T>` wraps a `RawId` with a marker type `T` that ensures IDs for
/// different resource types cannot be mixed up at compile time.
///
/// # Type Safety
///
/// ```compile_fail
/// use flui_foundation::{ViewId, ElementId};
///
/// let view_id = ViewId::new(1);
/// let id: ElementId = view_id;
/// ```
///
/// Keeping the identifier in its own domain compiles:
///
/// ```
/// use flui_foundation::{ViewId, ElementId};
///
/// let view_id = ViewId::new(1);
/// let id: ViewId = view_id;
/// ```
///
/// # Examples
///
/// ```rust
/// use flui_foundation::{ElementId, ViewId};
///
/// let view = ViewId::zip(1);
/// // ElementId is a distinct generational type — no zip/unzip.
/// let element = ElementId::new(1); // 1-based: index() == 0
///
/// // Different types; the compiler prevents mixing them.
/// // assert_eq!(view, element); // Would not compile!
/// assert_eq!(element.index(), 0);
/// ```
// The marker `T` appears only as a compile-time domain tag, never
// behind a reference or owned by the id. `PhantomData<fn() -> T>` makes
// `Id<T>` *invariant*-free over `T` while still requiring `T`, and —
// crucially — keeps `Id<T>: Send + Sync` regardless of `T`'s own auto
// traits (a `fn() -> T` pointer is always `Send + Sync`). A bare
// `PhantomData<T>` would instead make `Id<T>` covariant in `T` and leak
// `T`'s thread-safety, which is wrong for a zero-sized phantom tag.
#[repr(transparent)]
pub struct Id<T: Marker>(RawId, PhantomData<fn() -> T>);

impl<T: Marker> Id<T> {
    /// Creates an ID from a raw ID.
    ///
    /// This is a safe operation: every `RawId` already encodes a valid,
    /// non-zero index, and the marker type `T` carries no runtime
    /// invariant beyond compile-time domain tagging. Re-tagging a
    /// `RawId` under a different marker is a logic concern, not a memory-
    /// safety one, so no `unsafe` contract is required.
    ///
    /// ```rust
    /// use flui_foundation::{ViewId, RawId};
    ///
    /// // This must compile without an `unsafe` block.
    /// let raw = RawId::zip(1);
    /// let _id: ViewId = ViewId::from_raw(raw);
    /// ```
    #[inline]
    #[must_use]
    pub const fn from_raw(raw: RawId) -> Self {
        Self(raw, PhantomData)
    }

    /// Coerce the identifier into its raw underlying representation.
    #[inline]
    #[must_use]
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
    #[must_use]
    pub fn zip(index: Index) -> Self {
        Self(RawId::zip(index), PhantomData)
    }

    /// Unzip an Id back to its index.
    #[inline]
    #[must_use]
    pub const fn unzip(self) -> Index {
        self.0.unzip()
    }

    /// Creates an ID without checking for zero.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index` is not 0.
    ///
    /// Staging: zero workspace callers. See [`RawId::zip_unchecked`].
    #[doc(hidden)]
    #[inline]
    #[must_use]
    pub const unsafe fn zip_unchecked(index: Index) -> Self {
        // SAFETY: Caller guarantees index is non-zero
        unsafe { Self(RawId::zip_unchecked(index), PhantomData) }
    }

    /// Creates an ID, returning `None` if index is 0.
    #[inline]
    #[must_use]
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
    #[must_use]
    pub fn new(index: Index) -> Self {
        Self::zip(index)
    }

    /// Alias for `unzip` - returns the index.
    #[inline]
    #[must_use]
    pub const fn get(self) -> Index {
        self.unzip()
    }

    /// Alias for `try_zip` - creates an ID if index is non-zero.
    #[inline]
    #[must_use]
    pub const fn new_checked(index: Index) -> Option<Self> {
        Self::try_zip(index)
    }

    /// Alias for `zip_unchecked`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index` is not 0.
    ///
    /// Staging: zero workspace callers. See [`RawId::zip_unchecked`].
    #[doc(hidden)]
    #[inline]
    #[must_use]
    pub const unsafe fn new_unchecked(index: Index) -> Self {
        // SAFETY: Caller guarantees index is non-zero
        unsafe { Self::zip_unchecked(index) }
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

// `From<Index> for Id<T>` is always available (it used to be gated
// behind `#[cfg(test)]`). The conversion is safe — `Id::zip` wraps a
// 1-based usize and the niche-optimised `NonZeroUsize` guarantees zero
// is rejected at the `From<RawId> for Index` step upstream. Production
// callers gain `Id::from(some_index)` for ergonomics; the
// explicit-conversion path via `Id::zip(idx)` remains for callers that
// prefer the named constructor.
impl<T: Marker> From<Index> for Id<T> {
    fn from(index: Index) -> Self {
        Self::zip(index)
    }
}

// =========================================================================
// TreeId trait — minimal bound for tree-structure generics
// =========================================================================

/// Minimal bound for ID types usable in tree structure generics
/// (such as [`IndexedSlot<I>`](crate::IndexedSlot)).
///
/// This trait bundles the properties tree generics actually need:
/// `Copy`, `Eq`, `Hash`, `Debug`, `Display`, and thread-safety. It does **not**
/// expose `get() -> Index` — exposing the raw slab index would strip the
/// generation from generational IDs such as `ElementId`, making staleness
/// detection impossible at call sites outside `ElementTree`.
///
/// `Identifier` is a supertrait of `TreeId` that additionally provides
/// `get/zip/try_zip` for the non-generational `Id<T: Marker>` family
/// (`ViewId`, `RenderId`, `LayerId`, `SemanticsId`, …).
///
/// `ElementId` implements `TreeId` but **not** `Identifier`, so it can be
/// used as the `I` type parameter in `IndexedSlot<I>` without accidentally exposing an index-only accessor.
///
/// # Example
///
/// ```rust
/// use flui_foundation::{ElementId, TreeId, ViewId};
///
/// fn accepts_any_tree_id<I: TreeId>(id: I) {
///     // Can use Debug/Display, Copy, Eq — but not .get()
///     let _ = format!("{id}");
///     let id2 = id;
///     assert_eq!(id, id2);
/// }
///
/// accepts_any_tree_id(ElementId::new(1));
/// accepts_any_tree_id(ViewId::new(1));
/// ```
pub trait TreeId:
    Copy
    + Clone
    + Eq
    + PartialEq
    + Ord
    + PartialOrd
    + Hash
    + Debug
    + Display
    + WasmNotSendSync
    + 'static
{
    /// A diagnostic value identifying this id in error messages and logs.
    ///
    /// For plain ids this is the slab index; for generational ids it is the
    /// packed `(generation << 32) | index` value. **Diagnostic only** — it
    /// must never be used to index storage (that would strip the generation
    /// and reintroduce ABA).
    fn debug_value(self) -> u64;
}

// Blanket: every Identifier is automatically a TreeId.
impl<T: Identifier> TreeId for T {
    #[inline]
    fn debug_value(self) -> u64 {
        self.get() as u64
    }
}

// =========================================================================
// Identifier trait — index-based ids (the non-generational family)
// =========================================================================

/// Trait alias for index-based ID types usable in tree structures.
///
/// This provides a convenient bound for generic tree algorithms that need
/// to work with the non-generational `Id<T: Marker>` family
/// (`ViewId`, `RenderId`, `LayerId`, `SemanticsId`, …).
///
/// **Do not implement this for `ElementId`**: `Identifier::get()` returns the
/// raw slab index without the generation, making every call site ABA-unsafe.
/// Use `TreeId` as the generic bound where `ElementId` must be accepted.
///
/// # Example
///
/// ```rust
/// use flui_foundation::{Identifier, ViewId};
///
/// fn process_id<I: Identifier>(id: I) -> usize {
///     id.get()
/// }
///
/// assert_eq!(process_id(ViewId::zip(99)), 99);
/// ```
pub trait Identifier: TreeId {
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
///
/// Two sections:
/// - `plain:` → slab-index ids (`Id<markers::Marker>`, expose `get()`)
/// - `generational:` → ABA-safe arena keys (`GenId<markers::Marker>`,
///   no bare-index accessor)
macro_rules! ids {
    (
        plain: {
            $(
                $(#[$pmeta:meta])*
                pub type $pname:ident $pmarker:ident;
            )*
        }
        generational: {
            $(
                $(#[$gmeta:meta])*
                pub type $gname:ident $gmarker:ident;
            )*
        }
    ) => {
        /// Marker types for each resource.
        ///
        /// These are zero-sized enum types that exist only at compile time
        /// to provide type safety between different ID domains.
        pub mod markers {
            $(
                #[doc = concat!("Marker type for [`", stringify!($pname), "`](super::", stringify!($pname), ").")]
                #[derive(Debug)]
                pub enum $pmarker {}
                impl super::Marker for $pmarker {}
            )*
            $(
                #[doc = concat!("Marker type for [`", stringify!($gname), "`](super::", stringify!($gname), ").")]
                #[derive(Debug)]
                pub enum $gmarker {}
                impl super::Marker for $gmarker {}
            )*
        }

        $(
            $(#[$pmeta])*
            pub type $pname = Id<markers::$pmarker>;
        )*
        $(
            $(#[$gmeta])*
            pub type $gname = GenId<markers::$gmarker>;
        )*
    }
}

ids! {
    plain: {
        /// View ID - index into the View tree.
        ///
        /// Views are immutable configuration objects.
        /// They describe what the UI should look like but don't contain mutable state.
        pub type ViewId View;

        // NOTE: ElementId is NOT generated by this macro.
        // It is a distinct generational type defined below — see `ElementId`.

        /// Layer ID - index into the Layer tree.
        ///
        /// Layers handle compositing and GPU optimization. Created at repaint
        /// boundaries and cached for efficient rendering.
        pub type LayerId Layer;

        /// Semantics ID - index into the Semantics tree.
        ///
        /// `SemanticsNode`s provide accessibility information for screen readers
        /// and other assistive technologies.
        pub type SemanticsId Semantics;

        /// Listener ID - identifier for registered listeners.
        ///
        /// Used by `ChangeNotifier` and `Listenable` to track registered callbacks.
        pub type ListenerId Listener;

        /// Observer ID - identifier for registered observers.
        ///
        /// Used by `ObserverList` to track registered observers.
        pub type ObserverId Observer;

        /// Frame Callback ID - scheduler frame callback identifier.
        ///
        /// Identifies scheduled frame callbacks in the scheduler binding.
        pub type FrameCallbackId FrameCallback;

        /// Frame ID - scheduler frame identifier.
        ///
        /// Identifies individual frames in the scheduler frame lifecycle.
        pub type FrameId Frame;

        /// Task ID - scheduler task identifier.
        ///
        /// Identifies tasks in the priority-based task queue.
        pub type TaskId Task;

        /// Ticker ID - scheduler ticker identifier.
        ///
        /// Identifies tickers for animation timing callbacks.
        pub type TickerId Ticker;
    }
    generational: {
        /// Render ID - **generational** key into the `RenderObject` tree.
        ///
        /// `RenderObject`s handle layout and painting. They form a separate tree
        /// optimized for performance-critical operations.
        ///
        /// Generational ([`GenId`]): the render slab
        /// reuses slots, and any id-keyed retained cache or async repaint wake
        /// would otherwise alias a freed-and-reused slot (ABA — stale pixels
        /// under a different node). `RenderTree` accessors perform the
        /// generation check; there is no bare-index `get()`.
        pub type RenderId Render;

        /// Realm ID - **generational** key identifying one `UiRealm`
        /// incarnation.
        ///
        /// Generational so a recreated realm never compares equal to its
        /// predecessor: closing and reopening a realm's window mints a fresh
        /// generation at the same (or a reused) slot, and any stale id held
        /// by a worker or cache fails the generation check instead of
        /// silently addressing the new incarnation (ABA).
        ///
        /// Minted by the process composition root (`AppRuntime`); the
        /// platform layer maps native window handles to it. This is a
        /// distinct type from flui-platform's native `WindowId(pub u64)`
        /// (`flui-platform/src/window.rs`), which remains the
        /// platform-internal native handle key — the two are not
        /// interchangeable and neither converts implicitly to the other.
        pub type RealmId Realm;

        /// Data-transfer ID - **generational** key identifying one live
        /// transfer offer: a clipboard snapshot or an in-progress system
        /// drag session (ADR-0038).
        ///
        /// Generational ([`GenId`]): offers are short-lived and their slots
        /// are reused; a stale id held by a widget across a drag-cancel or a
        /// clipboard change must fail the generation check instead of
        /// silently addressing the next offer (ABA). Minted exclusively by
        /// the one offer table inside a platform's single
        /// `DataTransferSource` instance, so every observable id is
        /// redeemable at the place it was minted.
        pub type DataTransferId DataTransfer;
    }
}

// =========================================================================
// GenId<T> — generic generational arena key
// =========================================================================

/// A type-safe **generational** identifier for slab/arena-backed trees.
///
/// Packs a 32-bit slot index and a 32-bit non-zero generation into a
/// `NonZeroU64` with the layout `(generation << 32) | index` — the same
/// scheme as [`ElementId`], generalized over a [`Marker`] so other tree
/// domains ([`RenderId`] today; `LayerId`/`SemanticsId` candidates) get ABA
/// safety without bespoke types.
///
/// # Why generational
///
/// A bare slab index is reused after removal: a stale id minted against the
/// old occupant silently addresses the new one (ABA). Any id-keyed cache,
/// async wake, or cross-frame reference then targets the wrong node. The
/// generation makes staleness detectable: the owning tree bumps the slot's
/// generation on free, and its accessors compare before exposing the slot.
///
/// # No `Identifier` impl
///
/// Like [`ElementId`], `GenId<T>` deliberately does **not** implement
/// [`Identifier`]: `Identifier::get()` would strip the generation and make
/// every `id.get() - 1` call site ABA-unsafe again. Use [`Self::index`] /
/// [`Self::generation`] inside the owning tree's accessors only.
///
/// ```compile_fail
/// use flui_foundation::{Identifier, RenderId};
/// let id = RenderId::new(1);
/// let _ = id.get();
/// ```
///
/// The owner's slot accessor compiles:
///
/// ```
/// use flui_foundation::{Identifier, RenderId};
/// let id = RenderId::new(1);
/// let _ = id.index();
/// ```
///
/// # Niche optimisation
///
/// `generation >= 1` keeps the high 32 bits non-zero, so the all-zero
/// pattern is free for `Option<GenId<T>>` (same size as `GenId<T>`).
#[repr(transparent)]
pub struct GenId<T: Marker>(core::num::NonZeroU64, PhantomData<fn() -> T>);

impl<T: Marker> GenId<T> {
    /// Construct from explicit slab slot index and generation.
    ///
    /// # Panics
    ///
    /// Cannot panic in practice: `generation >= 1` guarantees a non-zero
    /// packed value; the `expect` is a proof guard.
    #[inline]
    #[must_use]
    pub fn new_gen(index: u32, generation: core::num::NonZeroU32) -> Self {
        let packed = (u64::from(generation.get()) << 32) | u64::from(index);
        Self(
            core::num::NonZeroU64::new(packed)
                .expect("BUG: generation is typed NonZeroU32, so generation >= 1 always keeps the packed value's high bits non-zero"),
            PhantomData,
        )
    }

    /// Convenience constructor preserving the 1-based `n` convention:
    /// `new(n)` ≡ `new_gen((n - 1) as u32, generation = 1)`, so
    /// `GenId::new(1).index() == 0`. Intended for tests/fixtures and the
    /// owning tree's first-generation mint; production ids should come from
    /// the owning tree so the generation matches the slot.
    ///
    /// # Panics
    ///
    /// Panics if `n == 0` (1-based invariant) or `n - 1` overflows `u32`.
    #[inline]
    #[track_caller]
    #[must_use]
    pub fn new(n: usize) -> Self {
        assert!(n >= 1, "GenId::new requires n >= 1 (1-based index); got 0");
        let index = u32::try_from(n - 1)
            .expect("GenId::new index overflows u32; tree exceeds u32::MAX slots");
        Self::new_gen(index, core::num::NonZeroU32::MIN)
    }

    /// The 0-based slab slot index packed into this id.
    ///
    /// For the owning tree's internal slot resolution only — every external
    /// access must go through the tree's generation-checked accessors.
    #[inline]
    #[must_use]
    pub fn index(self) -> u32 {
        (self.0.get() & 0xFFFF_FFFF) as u32
    }

    /// The generation packed into this id.
    ///
    /// # Panics
    ///
    /// Cannot panic for ids built via `new`/`new_gen` (proof guard only).
    #[inline]
    #[must_use]
    pub fn generation(self) -> core::num::NonZeroU32 {
        let generation = (self.0.get() >> 32) as u32;
        core::num::NonZeroU32::new(generation)
            .expect("BUG: every GenId is built via new/new_gen, which only accept a NonZeroU32 generation, so the packed high bits are always non-zero")
    }

    /// The raw packed `NonZeroU64` value (tracing/logging).
    #[inline]
    #[must_use]
    pub fn as_u64(self) -> u64 {
        self.0.get()
    }
}

// Manual impls so `T` needs no bounds beyond `Marker` (same rationale as `Id<T>`).
impl<T: Marker> Copy for GenId<T> {}
impl<T: Marker> Clone for GenId<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Marker> PartialEq for GenId<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T: Marker> Eq for GenId<T> {}
impl<T: Marker> PartialOrd for GenId<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T: Marker> Ord for GenId<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}
impl<T: Marker> Hash for GenId<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T: Marker> Debug for GenId<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = core::any::type_name::<T>();
        let marker_name = type_name.rsplit("::").next().unwrap_or(type_name);
        write!(
            f,
            "GenId<{}>(index={}, generation={})",
            marker_name,
            self.index(),
            self.generation()
        )
    }
}

impl<T: Marker> Display for GenId<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = core::any::type_name::<T>();
        let marker_name = type_name.rsplit("::").next().unwrap_or(type_name);
        write!(f, "{}({}:{})", marker_name, self.index(), self.generation())
    }
}

// TreeId directly (NOT via the Identifier blanket — GenId must not expose get()).
impl<T: Marker> TreeId for GenId<T> {
    #[inline]
    fn debug_value(self) -> u64 {
        self.as_u64()
    }
}

const _: () = assert!(
    core::mem::size_of::<GenId<markers::Render>>()
        == core::mem::size_of::<Option<GenId<markers::Render>>>(),
    "GenId niche invariant broken: generation must be NonZeroU32"
);

// =========================================================================
// PresentationId — one presentation-runtime incarnation
// =========================================================================

#[derive(Debug)]
enum PresentationMarker {}

impl Marker for PresentationMarker {}

/// Generational identity of one presentation-runtime incarnation.
///
/// A presentation owns the window/surface-facing state for one mounted UI:
/// input routing, frame scheduling, and the render pipeline. Its identity is
/// deliberately distinct from both [`RealmId`] and a platform-native window
/// handle. A realm may own multiple presentations, and replacing a native
/// window does not make those identity domains interchangeable.
///
/// The low 32 bits store a zero-based owner slot and the high 32 bits store a
/// non-zero generation. Reusing a released slot therefore mints a different
/// identity, so a stale frame, input event, or worker result cannot address a
/// later presentation incarnation by accident.
///
/// `PresentationId` is a nominal newtype, not a type alias or a conversion
/// layer around [`RealmId`]:
///
/// ```compile_fail
/// use flui_foundation::{PresentationId, RealmId};
///
/// let realm = RealmId::new(1);
/// let id: PresentationId = realm;
/// ```
///
/// Keeping the realm's own type compiles:
///
/// ```
/// use flui_foundation::{PresentationId, RealmId};
///
/// let realm = RealmId::new(1);
/// let id: RealmId = realm;
/// ```
///
/// Like [`GenId`] and [`ElementId`], this type intentionally does not
/// implement [`Identifier`]. Owner storage must validate both the slot and
/// generation instead of indexing through a generation-erasing `get()`.
///
/// ```compile_fail
/// use flui_foundation::{Identifier, PresentationId};
///
/// let presentation = PresentationId::new(1);
/// let _ = presentation.get();
/// ```
///
/// The owner's slot accessor compiles:
///
/// ```
/// use flui_foundation::{Identifier, PresentationId};
///
/// let presentation = PresentationId::new(1);
/// let _ = presentation.index();
/// ```
///
/// The non-zero generation preserves the null niche, so
/// `Option<PresentationId>` occupies the same eight bytes as
/// `PresentationId`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresentationId(GenId<PresentationMarker>);

const _: () = assert!(
    core::mem::size_of::<PresentationId>() == core::mem::size_of::<Option<PresentationId>>(),
    "PresentationId niche invariant broken: Option<PresentationId> must remain eight bytes"
);

impl PresentationId {
    /// Constructs an identity from a zero-based owner slot and non-zero generation.
    #[inline]
    #[must_use]
    pub fn new_gen(index: u32, generation: core::num::NonZeroU32) -> Self {
        Self(GenId::new_gen(index, generation))
    }

    /// Constructs a first-generation identity from a one-based slot number.
    ///
    /// `new(1)` addresses owner slot `0` with generation `1`. Production
    /// identities should be minted by the presentation owner so slot reuse
    /// advances the generation.
    ///
    /// # Panics
    ///
    /// Panics if `n == 0` or if `n - 1` exceeds `u32::MAX`.
    #[inline]
    #[track_caller]
    #[must_use]
    pub fn new(n: usize) -> Self {
        Self(GenId::new(n))
    }

    /// Returns the zero-based owner slot encoded in this identity.
    ///
    /// Only the owning presentation registry should use this value to resolve
    /// storage, and it must validate [`Self::generation`] before exposing a
    /// slot occupant.
    #[inline]
    #[must_use]
    pub fn index(self) -> u32 {
        self.0.index()
    }

    /// Returns the non-zero incarnation generation encoded in this identity.
    #[inline]
    #[must_use]
    pub fn generation(self) -> core::num::NonZeroU32 {
        self.0.generation()
    }

    /// Returns the packed identity value for diagnostics and tracing.
    ///
    /// This value must not be used as a storage index.
    #[inline]
    #[must_use]
    pub fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

impl Debug for PresentationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PresentationId(index={}, generation={})",
            self.index(),
            self.generation()
        )
    }
}

impl Display for PresentationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Presentation({}:{})", self.index(), self.generation())
    }
}

impl TreeId for PresentationId {
    #[inline]
    fn debug_value(self) -> u64 {
        self.as_u64()
    }
}

// =========================================================================
// PresentationAddress — the full routable identity of one presentation
// =========================================================================

/// The full routable identity of one presentation: which realm incarnation
/// owns it, and which presentation incarnation within that realm.
///
/// `RealmId` and `PresentationId` are each independently generational, but
/// neither alone is a safe cross-thread address: two different realm
/// incarnations can mint an identical `PresentationId` (same slot, same
/// generation) if their presentation counters happen to align, so a
/// protocol that carries only `PresentationId` cannot distinguish "the
/// right presentation in realm A" from "a same-numbered presentation in
/// unrelated realm B." Every protocol that crosses a thread or owner
/// boundary and needs to address one exact presentation — the raster
/// mailbox, the platform-to-realm dispatch table, the single native-window
/// map — carries this full pair, never `PresentationId` alone.
///
/// Promoted here (rule of three): `flui-app`, `flui-layer`, and
/// `flui-engine` each need this exact pair, so it lives in `flui-foundation`
/// alongside the ids it pairs rather than as a private duplicate in any one
/// of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresentationAddress {
    /// Which `UiRealm` incarnation owns this presentation.
    pub realm_id: RealmId,
    /// Which presentation incarnation, within that realm, this address
    /// names.
    pub presentation_id: PresentationId,
}

const _: () = {
    const fn assert_send_sync_copy<T: Send + Sync + Copy>() {}
    assert_send_sync_copy::<PresentationAddress>();
};

// =========================================================================
// ElementId — generational arena key for the element tree
// =========================================================================

/// Generational arena key for the element tree.
///
/// Packs a 32-bit slab slot index and a 32-bit non-zero generation into a
/// single `NonZeroU64` using the layout `(generation << 32) | index`.
///
/// # Niche optimisation
///
/// The all-zero bit pattern is unreachable: `generation` is `NonZeroU32` (≥ 1),
/// so the high 32 bits are never zero. `Option<ElementId>` therefore has the
/// same size as `ElementId` — the compiler uses 0 as the `None` sentinel.
///
/// ```rust
/// assert_eq!(
///     std::mem::size_of::<flui_foundation::ElementId>(),
///     std::mem::size_of::<Option<flui_foundation::ElementId>>(),
/// );
/// ```
///
/// # Staleness detection
///
/// Each slab slot carries a parallel generation counter. When a slot is freed
/// (eager remove or finalize), its generation is bumped. Any id that was minted
/// against the old generation now carries a stale generation value and will not
/// match the slot's current counter, causing all `ElementTree` accessors to
/// return `None`.
///
/// The staleness compare lives **only** inside `ElementTree`'s accessors —
/// `get` / `get_mut` / `contains` / `remove` / `remove_finalized` all route
/// through one private `resolve_index`. Call sites outside `ElementTree` must
/// go through those accessors; they must not call `id.index()` and index the
/// slab directly.
///
/// # No `Identifier` impl
///
/// `ElementId` does **not** implement `Identifier`. `Identifier::get()` returns
/// the raw slab index, stripping the generation and making every `id.get()-1`
/// call ABA-unsafe. Use `.index()` (0-based slab index) and `.generation()` for
/// all internal operations; external consumers use the `ElementTree` accessors.
///
/// The absence of a bare-index accessor is enforced at compile time: the
/// generational id has no `get()` (nor `Identifier::zip`/`unzip`):
///
/// ```compile_fail
/// use flui_foundation::{ElementId, Identifier};
/// let id = ElementId::new(1);
/// let _ = id.get();
/// ```
///
/// The owner's slot accessor compiles:
///
/// ```
/// use flui_foundation::{ElementId, Identifier};
/// let id = ElementId::new(1);
/// let _ = id.index();
/// ```
///
/// # Wire format
///
/// `ElementId` serialises as a `u64` (the packed `NonZeroU64` value). This is a
/// **wire-format break** from the old `Id<Element>` which serialised as a
/// `usize`. Element IDs are not persisted in any protocol, so this is acceptable.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementId(core::num::NonZeroU64);

// Compile-time niche assertion: must hold before any production code runs.
const _: () = assert!(
    core::mem::size_of::<ElementId>() == core::mem::size_of::<Option<ElementId>>(),
    "ElementId niche invariant broken: Option<ElementId> is larger than ElementId. \
     Generation field must be NonZeroU32 so the high 32 bits are never all-zero."
);

impl ElementId {
    /// Construct from explicit slab slot index and generation.
    ///
    /// The packed value is `(generation.get() as u64) << 32 | index as u64`.
    /// Because `generation >= 1`, the packed value is always non-zero, so the
    /// `NonZeroU64::new` cannot fail; the `expect` guards against a future
    /// refactor that accidentally makes the value zero.
    ///
    /// # Index cap
    ///
    /// `index` occupies the low 32 bits, so any value `0..=u32::MAX` packs
    /// losslessly (`u32::MAX + 1` addressable slots). `ElementTree` enforces
    /// the bound earlier — `alloc_id` narrows the `usize` slab index via
    /// `u32::try_from` and panics if a tree ever exceeds `u32::MAX` slots.
    ///
    /// # Panics
    ///
    /// In practice this function cannot panic: a `NonZeroU32` generation with
    /// any `u32` index always produces a non-zero packed value. The
    /// `.expect` is a compile-time proof guard; it fires only if the
    /// `(gen << 32) | idx` arithmetic somehow produced zero, which is
    /// impossible given `generation >= 1`.
    #[inline]
    #[must_use]
    pub fn new_gen(index: u32, generation: core::num::NonZeroU32) -> Self {
        let packed = (u64::from(generation.get()) << 32) | u64::from(index);
        // `generation >= 1` guarantees the high 32 bits are never zero, so the
        // packed value is always non-zero.
        Self(
            core::num::NonZeroU64::new(packed)
                .expect("BUG: generation is typed NonZeroU32, so generation >= 1 always keeps the packed value's high bits non-zero"),
        )
    }

    /// Convenience constructor preserving the 1-based `n` convention used by
    /// existing call sites: `new(n)` is equivalent to
    /// `new_gen((n - 1) as u32, NonZeroU32::new(1).unwrap())`.
    ///
    /// This maps 1-based indices to 0-based slab slots with generation = 1
    /// so that `ElementId::new(1).index() == 0`.
    ///
    /// # Panics
    ///
    /// Panics if `n` is 0 — the 1-based invariant requires `n >= 1`.
    /// Panics if `n - 1` overflows `u32` (more than `u32::MAX` elements).
    #[inline]
    #[track_caller]
    #[must_use]
    pub fn new(n: usize) -> Self {
        assert!(
            n >= 1,
            "ElementId::new requires n >= 1 (1-based index); got 0"
        );
        let index = u32::try_from(n - 1)
            .expect("ElementId::new index overflows u32; tree exceeds u32::MAX elements");
        Self::new_gen(index, core::num::NonZeroU32::MIN)
    }

    /// The 0-based slab slot index packed into this id.
    ///
    /// Use this (not a missing `.get()`) when you need the raw slab index for
    /// `ElementTree`-internal operations. All external consumers must go through
    /// `ElementTree`'s accessors (`get` / `get_mut` / `contains` / `remove` /
    /// `remove_finalized`), which perform the generation check before exposing
    /// the slot.
    #[inline]
    #[must_use]
    pub fn index(self) -> u32 {
        // The low 32 bits hold the index.
        (self.0.get() & 0xFFFF_FFFF) as u32
    }

    /// The generation packed into this id.
    ///
    /// A stale id (pointing at a freed-and-reused slot) will have a generation
    /// that no longer matches the slot's current generation in `ElementTree`.
    ///
    /// # Panics
    ///
    /// In practice this function cannot panic: every `ElementId` construction
    /// path keeps the high 32 bits non-zero — `new`/`new_gen` require a
    /// `NonZeroU32` generation by their own typing, and `Deserialize` (the
    /// only other path) explicitly rejects a zero generation field before
    /// building the value. The `.expect` is a proof guard that fires only if
    /// a zero-generation id was somehow constructed outside all of the above,
    /// which the `NonZeroU64` newtype wrapper prevents.
    #[inline]
    #[must_use]
    pub fn generation(self) -> core::num::NonZeroU32 {
        // The high 32 bits hold the generation. It is always non-zero because
        // `new_gen` requires a `NonZeroU32` generation, and the minimum packed
        // value with generation=1 is `1 << 32` which has non-zero high bits.
        let generation = (self.0.get() >> 32) as u32;
        // Invariant: every construction path (new/new_gen's NonZeroU32 typing,
        // Deserialize's explicit validation) keeps generation >= 1.
        core::num::NonZeroU32::new(generation)
            .expect("BUG: the high 32 bits are a NonZeroU32 generation by construction (new/new_gen typing; Deserialize validation)")
    }

    /// The raw packed `NonZeroU64` value. Useful for tracing / logging.
    #[inline]
    #[must_use]
    pub fn as_u64(self) -> u64 {
        self.0.get()
    }
}

impl core::fmt::Debug for ElementId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "ElementId(index={}, generation={})",
            self.index(),
            self.generation()
        )
    }
}

impl core::fmt::Display for ElementId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Element({}:{})", self.index(), self.generation())
    }
}

// `ElementId` is `Copy` + `Send` + `Sync` (all fields are plain integers).
// `TreeId` is auto-satisfied via the blanket `impl<T: Identifier> TreeId for T`
// (but ElementId does NOT implement Identifier — it implements TreeId directly).
impl TreeId for ElementId {
    #[inline]
    fn debug_value(self) -> u64 {
        self.as_u64()
    }
}

// =========================================================================
// Serde support
// =========================================================================

#[cfg(feature = "serde")]
mod serde_impl {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{ElementId, Id, Index, Marker, RawId};

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
            // `from_raw` is infallible and safe: the deserialized
            // `RawId` already upholds the non-zero niche invariant.
            Ok(Self::from_raw(raw))
        }
    }

    /// `ElementId` serialises as a `u64` (the packed `NonZeroU64` value).
    ///
    /// Wire-format note: this differs from the old `Id<Element>` which
    /// serialised as a `usize`. Element IDs are not persisted in any external
    /// protocol, so this break is acceptable.
    impl Serialize for ElementId {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.as_u64().serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for ElementId {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let packed = u64::deserialize(deserializer)?;
            let nz = core::num::NonZeroU64::new(packed).ok_or_else(|| {
                serde::de::Error::custom("ElementId packed value must be non-zero")
            })?;
            // Verify the generation (high 32 bits) is non-zero.
            let generation = (packed >> 32) as u32;
            if generation == 0 {
                return Err(serde::de::Error::custom(
                    "ElementId generation field (high 32 bits) must be non-zero",
                ));
            }
            Ok(ElementId(nz))
        }
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;

    use super::*;

    // -----------------------------------------------------------------------
    // RawId tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Id<T> tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // GenId (generational, RenderId) tests
    // -----------------------------------------------------------------------

    /// ABA safety: same slot, different generation → distinct ids.
    fn gen_id_stale_generation_is_distinct() {
        let gen1 = NonZeroU32::new(1).unwrap();
        let gen2 = NonZeroU32::new(2).unwrap();
        let live = RenderId::new_gen(5, gen2);
        let stale = RenderId::new_gen(5, gen1);
        assert_ne!(
            stale, live,
            "a stale RenderId must never compare equal to the slot's live id"
        );
    }

    // -----------------------------------------------------------------------
    // PresentationId tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // ElementId (generational) tests
    // -----------------------------------------------------------------------

    /// Niche/size: `Option<ElementId>` must have the same size as `ElementId`.
    /// The all-zero bit pattern is unreachable because generation >= 1.
    fn element_id_niche_size() {
        assert_eq!(
            size_of::<ElementId>(),
            size_of::<Option<ElementId>>(),
            "Option<ElementId> niche invariant: must equal size_of::<ElementId>()"
        );
        assert_eq!(
            size_of::<ElementId>(),
            size_of::<u64>(),
            "ElementId must be 8 bytes (NonZeroU64)"
        );
    }

    /// Zero input panics with a helpful message.
    fn element_id_new_zero_panics() {
        let payload = std::panic::catch_unwind(|| ElementId::new(0))
            .expect_err("ElementId::new(0) must panic");
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or_default();
        assert!(
            message.contains("ElementId::new requires n >= 1"),
            "unexpected panic message: {message}"
        );
    }

    /// The deserialiser rejects a packed `u64` whose high 32 bits
    /// (generation) are zero — a tampered/foreign value that could otherwise
    /// fabricate a generation-0 id that no live slot ever mints.
    #[cfg(feature = "serde")]
    fn element_id_serde_rejects_zero_generation() {
        // index=1, generation=0 → high 32 bits clear.
        let tampered = 0x0000_0000_0000_0001u64;
        let err = serde_json::from_str::<ElementId>(&tampered.to_string())
            .expect_err("generation==0 must be rejected");
        assert!(
            err.to_string().contains("generation"),
            "rejection must cite the generation invariant, got: {err}"
        );

        // A fully-zero packed value (the `Option` niche / None sentinel) is
        // likewise not a valid `ElementId`.
        assert!(serde_json::from_str::<ElementId>("0").is_err());
    }

    #[test]
    fn id_contract() {
        crate::test_cases::run_cases(&[
            (
                "a stale generation is distinct",
                gen_id_stale_generation_is_distinct,
            ),
            ("Option<ElementId> keeps the niche", element_id_niche_size),
            ("ElementId::new(0) panics", element_id_new_zero_panics),
            #[cfg(feature = "serde")]
            (
                "serde rejects a zero generation",
                element_id_serde_rejects_zero_generation,
            ),
        ]);
    }
}
