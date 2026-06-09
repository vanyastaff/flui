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
// F9 — `#[expect]` over `#[allow]` (edition-2024 idiom): this module
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
/// `RawId` stays part of the public surface (F23 considered downgrading it to
/// `pub(crate)` but kept it `pub`): U1/F1 made `Id::from_raw(raw: RawId)` a
/// safe public constructor with a public doc-test, and downstream tests round-
/// trip through `Id::into_raw() -> RawId` + `RawId::unzip`. The internal
/// `Index` alias, which had no downstream consumers, is the part F23 narrows.
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
/// let element_id: ElementId = view_id; // Compile error!
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
// F7 — the marker `T` appears only as a compile-time domain tag, never
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

// Cycle 3 T-14: `From<Index> for Id<T>` is always available (was
// `#[cfg(test)]` pre-cycle). The conversion is safe — `Id::zip`
// wraps a 1-based usize and the niche-optimised `NonZeroUsize`
// guarantees zero is rejected at the `From<RawId> for Index` step
// upstream. Production callers gain `Id::from(some_index)` for
// ergonomics; the explicit-conversion path via `Id::zip(idx)`
// remains for callers that prefer the named constructor.
impl<T: Marker> From<Index> for Id<T> {
    fn from(index: Index) -> Self {
        Self::zip(index)
    }
}

// =========================================================================
// TreeId trait — minimal bound for flui-tree generics
// =========================================================================

/// Minimal bound for ID types usable in tree structure generics
/// (`Slot<I>`, `IndexedSlot<I>`, `TreeRead<I>`, etc.).
///
/// This trait bundles the properties the tree generic machinery actually needs:
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
/// used as the `I` type parameter in `IndexedSlot<I>`, `TreeRead<I>`, etc.
/// without accidentally exposing an index-only accessor.
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
}

// Blanket: every Identifier is automatically a TreeId.
impl<T: Identifier> TreeId for T {}

// =========================================================================
// Identifier trait alias (for backwards compatibility with flui-tree)
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

    // NOTE: ElementId is NOT generated by this macro.
    // It is a distinct generational type defined below — see `ElementId`.

    /// Render ID - index into the `RenderObject` tree.
    ///
    /// `RenderObject`s handle layout and painting. They form a separate tree
    /// optimized for performance-critical operations.
    pub type RenderId Render;

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
    // Scheduler IDs (consumed by flui-scheduler)
    // =====================================================================

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
/// use flui_foundation::ElementId;
/// let id = ElementId::new(1);
/// // ERROR: no method named `get` — ElementId is not an `Identifier`.
/// let _ = id.get();
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
                .expect("generation >= 1 ensures packed value is non-zero"),
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
    /// In practice this function cannot panic: any `ElementId` constructed via
    /// `new` or `new_gen` has a non-zero generation in the high 32 bits. The
    /// `.expect` is a proof guard that fires only if a zero-generation id was
    /// somehow constructed outside the public API, which is prevented by the
    /// `NonZeroU64` newtype wrapper.
    #[inline]
    #[must_use]
    pub fn generation(self) -> core::num::NonZeroU32 {
        // The high 32 bits hold the generation. It is always non-zero because
        // `new_gen` requires a `NonZeroU32` generation, and the minimum packed
        // value with generation=1 is `1 << 32` which has non-zero high bits.
        let generation = (self.0.get() >> 32) as u32;
        // Invariant: `new_gen` only accepts `NonZeroU32`, so generation is always >= 1.
        core::num::NonZeroU32::new(generation)
            .expect("ElementId generation is always >= 1; packed value invariant violated")
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
impl TreeId for ElementId {}

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
        assert_eq!(RawId::try_zip(42).map(super::RawId::unzip), Some(42));
    }

    // -----------------------------------------------------------------------
    // Id<T> tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_id_basics() {
        let id = ViewId::zip(42);
        assert_eq!(id.unzip(), 42);
    }

    #[test]
    #[should_panic(expected = "ID index must be non-zero")]
    fn test_id_zero_panics() {
        let _ = ViewId::zip(0);
    }

    #[test]
    fn test_id_try_zip() {
        assert!(RenderId::try_zip(0).is_none());
        assert_eq!(LayerId::try_zip(42).map(super::Id::unzip), Some(42));
    }

    #[test]
    fn test_non_element_niche_optimization() {
        // Option<Id<T>> must have same size as Id<T>
        assert_eq!(size_of::<ViewId>(), size_of::<Option<ViewId>>());
        assert_eq!(size_of::<RawId>(), size_of::<Option<RawId>>());
        assert_eq!(size_of::<RenderId>(), size_of::<Option<RenderId>>());
    }

    #[test]
    fn test_debug_format() {
        let id = ViewId::zip(42);
        let debug = format!("{id:?}");
        assert!(debug.contains("View"));
        assert!(debug.contains("42"));
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

        let recovered = ViewId::from_raw(raw);
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
        assert_eq!(ViewId::new_checked(42).map(super::Id::get), Some(42));
    }

    #[test]
    fn test_scheduler_id_types() {
        // Sanity-check the scheduler-consumer IDs survive the audit.
        let frame = FrameId::zip(1);
        let callback = FrameCallbackId::zip(2);
        let task = TaskId::zip(3);
        let ticker = TickerId::zip(4);

        assert_eq!(frame.unzip(), 1);
        assert_eq!(callback.unzip(), 2);
        assert_eq!(task.unzip(), 3);
        assert_eq!(ticker.unzip(), 4);
    }

    // -----------------------------------------------------------------------
    // ElementId (generational) tests
    // -----------------------------------------------------------------------

    /// E1 — niche/size: `Option<ElementId>` must have the same size as `ElementId`.
    /// The all-zero bit pattern is unreachable because generation >= 1.
    #[test]
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

    /// E1 — pack/unpack round-trip for `new_gen`.
    #[test]
    fn element_id_new_gen_round_trip() {
        let generation = NonZeroU32::new(7).unwrap();
        let id = ElementId::new_gen(42, generation);
        assert_eq!(id.index(), 42, "index must round-trip");
        assert_eq!(id.generation(), generation, "generation must round-trip");
    }

    /// E1 — `new(n)` preserves 1-based convention: `new(1).index() == 0`.
    #[test]
    fn element_id_new_one_based() {
        let id = ElementId::new(1);
        assert_eq!(id.index(), 0, "new(1) must map to slab index 0");
        assert_eq!(
            id.generation(),
            NonZeroU32::new(1).unwrap(),
            "new(n) must use generation=1"
        );

        let id2 = ElementId::new(10);
        assert_eq!(id2.index(), 9);
        assert_eq!(id2.generation(), NonZeroU32::new(1).unwrap());
    }

    /// E1 — zero input panics with a helpful message.
    #[test]
    #[should_panic(expected = "ElementId::new requires n >= 1")]
    fn element_id_new_zero_panics() {
        let _ = ElementId::new(0);
    }

    /// E1 — index cap: `new(n)` panics when the 0-based slab index `n - 1`
    /// exceeds `u32::MAX` (the packed index field is 32 bits). 64-bit hosts
    /// only — on a 32-bit `usize` the literal itself would overflow.
    #[test]
    #[cfg(target_pointer_width = "64")]
    #[should_panic(expected = "index overflows u32")]
    fn element_id_new_index_overflow_panics() {
        let _ = ElementId::new(u32::MAX as usize + 2);
    }

    /// E1 — `new_gen` round-trips at the maximum generation. The
    /// overflow *policy* (retire-by-panic when a slot is recycled past
    /// `u32::MAX`) lives in `ElementTree::bump_generation`; this only
    /// checks the id packs/unpacks the boundary value losslessly.
    #[test]
    fn element_id_max_generation_round_trip() {
        let max_gen = NonZeroU32::new(u32::MAX).unwrap();
        let id = ElementId::new_gen(0, max_gen);
        assert_eq!(id.index(), 0);
        assert_eq!(id.generation(), max_gen);
    }

    /// E1 — Debug / Display output contains discriminating fields.
    #[test]
    fn element_id_display_debug() {
        let generation = NonZeroU32::new(3).unwrap();
        let id = ElementId::new_gen(5, generation);
        let debug = format!("{id:?}");
        assert!(debug.contains('5'), "debug must contain index");
        assert!(debug.contains('3'), "debug must contain generation");
        let display = format!("{id}");
        assert!(
            display.contains("Element"),
            "display must contain 'Element'"
        );
    }

    /// E1 — distinct (index, generation) pairs produce distinct ids.
    #[test]
    fn element_id_eq_uses_full_packed_value() {
        let gen1 = NonZeroU32::new(1).unwrap();
        let gen2 = NonZeroU32::new(2).unwrap();
        let id_a = ElementId::new_gen(0, gen1);
        let id_b = ElementId::new_gen(0, gen2); // same index, different generation
        let id_c = ElementId::new_gen(1, gen1); // different index, same generation
        assert_ne!(id_a, id_b, "stale id must not equal live id (ABA safety)");
        assert_ne!(id_a, id_c);
        assert_eq!(id_a, ElementId::new_gen(0, gen1));
    }

    /// E1 — `TreeId` bound: `ElementId` must satisfy `TreeId` without `Identifier`.
    #[test]
    fn element_id_implements_tree_id() {
        fn needs_tree_id<I: TreeId>(id: I) -> I {
            id
        }
        let id = ElementId::new(1);
        let returned = needs_tree_id(id);
        assert_eq!(id, returned);
    }

    /// E1 — `ElementId` does NOT accidentally implement `Identifier`.
    /// This is a compile-time check enforced by the absence of the impl.
    /// We verify at runtime that the two traits are distinct by confirming
    /// `ViewId` (which does impl `Identifier`) can call `.get()` but `ElementId`
    /// has no such method (the `get()` method simply does not exist on `ElementId`).
    #[test]
    fn view_id_implements_identifier_element_id_does_not() {
        // ViewId: Identifier — .get() works
        let view = ViewId::new(5);
        assert_eq!(view.get(), 5);
        // ElementId: no .get() method. The absence of an `Identifier` impl is
        // enforced at compile time by the `compile_fail` doc-test on the
        // `ElementId` type (which shows `id.get()` does not compile).
        let elem = ElementId::new(5);
        assert_eq!(elem.index(), 4); // 0-based
    }

    /// E1 — `as_u64` returns the raw packed value; useful for tracing.
    #[test]
    fn element_id_as_u64_nonzero() {
        let id = ElementId::new(1);
        assert!(id.as_u64() > 0, "packed value must always be non-zero");
        // Verify it is indeed the packed representation.
        let gen_bits = u64::from(id.generation().get()) << 32;
        let idx_bits = u64::from(id.index());
        assert_eq!(id.as_u64(), gen_bits | idx_bits);
    }

    /// E1 — serde round-trip through a real format (`serde_json`): an
    /// `ElementId` serialises to its packed `u64` and deserialises back to an
    /// equal id, preserving both index and generation.
    #[test]
    #[cfg(feature = "serde")]
    fn element_id_serde_round_trip() {
        let id = ElementId::new_gen(7, NonZeroU32::new(3).unwrap());
        let json = serde_json::to_string(&id).expect("serialize");
        // Wire format is the bare packed u64, not a struct.
        assert_eq!(json, id.as_u64().to_string());
        let back: ElementId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, id);
        assert_eq!(back.index(), 7);
        assert_eq!(back.generation().get(), 3);
    }

    /// E1 — the deserialiser rejects a packed `u64` whose high 32 bits
    /// (generation) are zero — a tampered/foreign value that could otherwise
    /// fabricate a generation-0 id that no live slot ever mints.
    #[test]
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
}
