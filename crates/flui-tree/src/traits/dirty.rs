//! Dirty tracking traits for layout and paint.
//!
//! This module provides traits for managing layout and paint dirty flags
//! on tree elements, with support for both individual and batch operations.
//!
//! # Architecture
//!
//! ```text
//! DirtyTracking (base trait)
//!     │
//!     └── DirtyTrackingExt (batch operations, enumeration)
//!
//! AtomicDirtyFlags (implementation helper)
//!     │
//!     └── Compact atomic storage for per-element flags
//! ```
//!
//! # Thread Safety
//!
//! All traits require `Send + Sync`. Marking operations take `&self`
//! (not `&mut self`) to allow concurrent marking from callbacks.
//! Implementations typically use atomic operations.

use flui_foundation::ElementId;
use std::sync::atomic::{AtomicU8, Ordering};

// ============================================================================
// DIRTY TRACKING (BASE TRAIT)
// ============================================================================

/// Dirty flag management for layout and paint phases.
///
/// This trait provides methods to mark elements as needing layout
/// or paint, and to query/clear those flags. It's designed for
/// efficient incremental updates where only dirty elements are
/// processed.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync`. The `mark_*` methods take
/// `&self` (not `&mut self`) to allow marking from multiple contexts.
/// This typically requires internal synchronization (atomic flags).
///
/// # Design Rationale
///
/// Dirty tracking is separated from other tree operations because:
/// 1. Flags are typically stored in `RenderState`, not the node itself
/// 2. Marking can happen from callbacks without mutable access
/// 3. Different implementations may use atomic vs. locked flags
///
/// # Example
///
/// ```rust
/// use flui_tree::DirtyTracking;
/// use flui_foundation::ElementId;
/// use std::sync::Mutex;
/// use std::collections::HashSet;
///
/// struct SimpleDirtyTracker {
///     layout_dirty: Mutex<HashSet<ElementId>>,
///     paint_dirty: Mutex<HashSet<ElementId>>,
/// }
///
/// impl SimpleDirtyTracker {
///     fn new() -> Self {
///         Self {
///             layout_dirty: Mutex::new(HashSet::new()),
///             paint_dirty: Mutex::new(HashSet::new()),
///         }
///     }
/// }
///
/// impl DirtyTracking for SimpleDirtyTracker {
///     fn mark_needs_layout(&self, id: ElementId) {
///         self.layout_dirty.lock().unwrap().insert(id);
///     }
///
///     fn mark_needs_paint(&self, id: ElementId) {
///         self.paint_dirty.lock().unwrap().insert(id);
///     }
///
///     fn clear_needs_layout(&self, id: ElementId) {
///         self.layout_dirty.lock().unwrap().remove(&id);
///     }
///
///     fn clear_needs_paint(&self, id: ElementId) {
///         self.paint_dirty.lock().unwrap().remove(&id);
///     }
///
///     fn needs_layout(&self, id: ElementId) -> bool {
///         self.layout_dirty.lock().unwrap().contains(&id)
///     }
///
///     fn needs_paint(&self, id: ElementId) -> bool {
///         self.paint_dirty.lock().unwrap().contains(&id)
///     }
/// }
///
/// // Usage
/// let tracker = SimpleDirtyTracker::new();
/// let id = ElementId::new(1);
///
/// tracker.mark_needs_layout(id);
/// assert!(tracker.needs_layout(id));
/// assert!(tracker.is_dirty(id));
///
/// tracker.clear_needs_layout(id);
/// assert!(!tracker.needs_layout(id));
/// ```
pub trait DirtyTracking: Send + Sync {
    /// Marks an element as needing layout.
    ///
    /// This should propagate up to ancestors if they need to be
    /// re-laid-out as well (relayout boundary handling).
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark
    ///
    /// # Thread Safety
    ///
    /// This method takes `&self` to allow concurrent marking.
    /// Implementations should use atomic operations or other
    /// synchronization.
    fn mark_needs_layout(&self, id: ElementId);

    /// Marks an element as needing paint.
    ///
    /// Unlike layout, paint dirty flags typically don't propagate
    /// to ancestors (paint is local to the element).
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark
    fn mark_needs_paint(&self, id: ElementId);

    /// Clears the needs-layout flag for an element.
    ///
    /// Called after layout is complete for the element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to clear
    fn clear_needs_layout(&self, id: ElementId);

    /// Clears the needs-paint flag for an element.
    ///
    /// Called after paint is complete for the element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to clear
    fn clear_needs_paint(&self, id: ElementId);

    /// Returns `true` if the element needs layout.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    fn needs_layout(&self, id: ElementId) -> bool;

    /// Returns `true` if the element needs paint.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    fn needs_paint(&self, id: ElementId) -> bool;

    /// Returns `true` if the element needs either layout or paint.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    #[inline]
    fn is_dirty(&self, id: ElementId) -> bool {
        self.needs_layout(id) || self.needs_paint(id)
    }

    /// Marks an element as needing both layout and paint.
    ///
    /// Convenience method for full invalidation.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark
    #[inline]
    fn mark_needs_rebuild(&self, id: ElementId) {
        self.mark_needs_layout(id);
        self.mark_needs_paint(id);
    }

    /// Clears both layout and paint flags.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to clear
    #[inline]
    fn clear_dirty(&self, id: ElementId) {
        self.clear_needs_layout(id);
        self.clear_needs_paint(id);
    }

    // ========================================================================
    // ENUMERATION (override for implementations that track dirty sets)
    // ========================================================================

    /// Returns all elements that need layout.
    ///
    /// Default returns empty vector. Override in implementations
    /// that maintain dirty sets.
    #[inline]
    fn elements_needing_layout(&self) -> Vec<ElementId> {
        Vec::new()
    }

    /// Returns all elements that need paint.
    ///
    /// Default returns empty vector. Override in implementations
    /// that maintain dirty sets.
    #[inline]
    fn elements_needing_paint(&self) -> Vec<ElementId> {
        Vec::new()
    }
}

// ============================================================================
// EXTENDED DIRTY TRACKING
// ============================================================================

/// Extended dirty tracking with batch operations and enumeration.
///
/// This trait adds batch operations and dirty element enumeration
/// for more efficient pipeline processing.
///
/// # Automatic Implementation
///
/// This trait is automatically implemented for all `DirtyTracking`
/// implementors via a blanket implementation.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::DirtyTrackingExt;
///
/// fn process_all_dirty<T: DirtyTrackingExt>(tree: &T) {
///     // Get all elements needing layout
///     let dirty = tree.elements_needing_layout();
///
///     // Process and clear in batch
///     for id in &dirty {
///         // ... process layout ...
///     }
///     tree.clear_many_layout(dirty);
/// }
/// ```
pub trait DirtyTrackingExt: DirtyTracking {
    // ========================================================================
    // ENUMERATION (delegates to DirtyTracking methods)
    // ========================================================================

    /// Returns all dirty elements (needing layout or paint).
    #[inline]
    fn elements_dirty(&self) -> Vec<ElementId> {
        let mut result = DirtyTracking::elements_needing_layout(self);
        for id in DirtyTracking::elements_needing_paint(self) {
            if !result.contains(&id) {
                result.push(id);
            }
        }
        result
    }

    // ========================================================================
    // COUNTING
    // ========================================================================

    /// Returns the count of elements needing layout.
    #[inline]
    fn layout_dirty_count(&self) -> usize {
        DirtyTracking::elements_needing_layout(self).len()
    }

    /// Returns the count of elements needing paint.
    #[inline]
    fn paint_dirty_count(&self) -> usize {
        DirtyTracking::elements_needing_paint(self).len()
    }

    /// Returns the total count of dirty elements.
    #[inline]
    fn dirty_count(&self) -> usize {
        self.elements_dirty().len()
    }

    /// Returns `true` if any elements need layout.
    #[inline]
    fn has_dirty_layout(&self) -> bool {
        self.layout_dirty_count() > 0
    }

    /// Returns `true` if any elements need paint.
    #[inline]
    fn has_dirty_paint(&self) -> bool {
        self.paint_dirty_count() > 0
    }

    /// Returns `true` if any elements are dirty.
    #[inline]
    fn has_any_dirty(&self) -> bool {
        self.has_dirty_layout() || self.has_dirty_paint()
    }

    // ========================================================================
    // BATCH MARKING
    // ========================================================================

    /// Marks multiple elements as needing layout.
    ///
    /// # Arguments
    ///
    /// * `ids` - The elements to mark
    #[inline]
    fn mark_many_needs_layout(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.mark_needs_layout(id);
        }
    }

    /// Marks multiple elements as needing paint.
    ///
    /// # Arguments
    ///
    /// * `ids` - The elements to mark
    #[inline]
    fn mark_many_needs_paint(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.mark_needs_paint(id);
        }
    }

    /// Marks multiple elements as needing rebuild (layout + paint).
    #[inline]
    fn mark_many_needs_rebuild(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.mark_needs_rebuild(id);
        }
    }

    // ========================================================================
    // BATCH CLEARING
    // ========================================================================

    /// Clears layout flags for multiple elements.
    ///
    /// # Arguments
    ///
    /// * `ids` - The elements to clear
    #[inline]
    fn clear_many_layout(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.clear_needs_layout(id);
        }
    }

    /// Clears paint flags for multiple elements.
    ///
    /// # Arguments
    ///
    /// * `ids` - The elements to clear
    #[inline]
    fn clear_many_paint(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.clear_needs_paint(id);
        }
    }

    /// Clears all flags for multiple elements.
    #[inline]
    fn clear_many_dirty(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.clear_dirty(id);
        }
    }

    /// Clears all layout dirty flags.
    #[inline]
    fn clear_all_layout(&self) {
        for id in DirtyTracking::elements_needing_layout(self) {
            self.clear_needs_layout(id);
        }
    }

    /// Clears all paint dirty flags.
    #[inline]
    fn clear_all_paint(&self) {
        for id in DirtyTracking::elements_needing_paint(self) {
            self.clear_needs_paint(id);
        }
    }

    /// Clears all dirty flags.
    #[inline]
    fn clear_all_dirty(&self) {
        for id in self.elements_dirty() {
            self.clear_dirty(id);
        }
    }

    // ========================================================================
    // DRAIN OPERATIONS
    // ========================================================================

    /// Returns and clears all elements needing layout.
    ///
    /// This is more efficient than calling `elements_needing_layout()`
    /// followed by `clear_all_layout()`.
    #[inline]
    fn drain_needing_layout(&self) -> Vec<ElementId> {
        let elements = DirtyTracking::elements_needing_layout(self);
        self.clear_many_layout(elements.iter().copied());
        elements
    }

    /// Returns and clears all elements needing paint.
    #[inline]
    fn drain_needing_paint(&self) -> Vec<ElementId> {
        let elements = DirtyTracking::elements_needing_paint(self);
        self.clear_many_paint(elements.iter().copied());
        elements
    }

    /// Returns and clears all dirty elements.
    #[inline]
    fn drain_dirty(&self) -> Vec<ElementId> {
        let elements = self.elements_dirty();
        self.clear_many_dirty(elements.iter().copied());
        elements
    }

    // ========================================================================
    // CONDITIONAL OPERATIONS
    // ========================================================================

    /// Marks an element as needing layout if a condition is met.
    #[inline]
    fn mark_needs_layout_if(&self, id: ElementId, condition: bool) {
        if condition {
            self.mark_needs_layout(id);
        }
    }

    /// Marks an element as needing paint if a condition is met.
    #[inline]
    fn mark_needs_paint_if(&self, id: ElementId, condition: bool) {
        if condition {
            self.mark_needs_paint(id);
        }
    }

    /// Marks an element as needing rebuild if a condition is met.
    #[inline]
    fn mark_needs_rebuild_if(&self, id: ElementId, condition: bool) {
        if condition {
            self.mark_needs_rebuild(id);
        }
    }

    // ========================================================================
    // FILTERING
    // ========================================================================

    /// Filters elements needing layout by a predicate.
    #[inline]
    fn filter_needing_layout<F>(&self, predicate: F) -> Vec<ElementId>
    where
        F: FnMut(&ElementId) -> bool,
    {
        DirtyTracking::elements_needing_layout(self)
            .into_iter()
            .filter(predicate)
            .collect()
    }

    /// Filters elements needing paint by a predicate.
    #[inline]
    fn filter_needing_paint<F>(&self, predicate: F) -> Vec<ElementId>
    where
        F: FnMut(&ElementId) -> bool,
    {
        DirtyTracking::elements_needing_paint(self)
            .into_iter()
            .filter(predicate)
            .collect()
    }
}

// Blanket implementation for all DirtyTracking implementors
impl<T: DirtyTracking + ?Sized> DirtyTrackingExt for T {}

// ============================================================================
// ATOMIC DIRTY FLAGS
// ============================================================================

/// Compact atomic dirty flags for render elements.
///
/// This type provides lock-free dirty flag management using a single
/// atomic byte. It's designed to be embedded in `RenderState`.
///
/// # Flags
///
/// - Bit 0: `NEEDS_LAYOUT`
/// - Bit 1: `NEEDS_PAINT`
/// - Bit 2: `NEEDS_COMPOSITING_BITS_UPDATE`
/// - Bit 3: `NEEDS_SEMANTICS_UPDATE`
/// - Bit 4: `NEEDS_ACCESSIBILITY_UPDATE`
/// - Bit 5: `IS_REPAINT_BOUNDARY`
/// - Bit 6: `IS_RELAYOUT_BOUNDARY`
/// - Bit 7: Reserved
///
/// # Thread Safety
///
/// All operations use atomic instructions. By default, `Ordering::SeqCst`
/// is used for correctness. For performance-critical code, the `_relaxed`
/// variants use `Ordering::Relaxed`.
///
/// # Example
///
/// ```rust
/// use flui_tree::AtomicDirtyFlags;
///
/// let flags = AtomicDirtyFlags::new();
///
/// // Mark as needing layout
/// flags.mark_needs_layout();
/// assert!(flags.needs_layout());
///
/// // Clear after layout
/// flags.clear_needs_layout();
/// assert!(!flags.needs_layout());
/// ```
#[derive(Debug)]
pub struct AtomicDirtyFlags {
    flags: AtomicU8,
}

impl AtomicDirtyFlags {
    // ========================================================================
    // FLAG CONSTANTS
    // ========================================================================

    /// Flag bit for needs-layout.
    pub const NEEDS_LAYOUT: u8 = 1 << 0;

    /// Flag bit for needs-paint.
    pub const NEEDS_PAINT: u8 = 1 << 1;

    /// Flag bit for needs-compositing-bits-update.
    pub const NEEDS_COMPOSITING_BITS_UPDATE: u8 = 1 << 2;

    /// Flag bit for needs-semantics-update.
    pub const NEEDS_SEMANTICS_UPDATE: u8 = 1 << 3;

    /// Flag bit for needs-accessibility-update.
    pub const NEEDS_ACCESSIBILITY_UPDATE: u8 = 1 << 4;

    /// Flag bit indicating this is a repaint boundary.
    pub const IS_REPAINT_BOUNDARY: u8 = 1 << 5;

    /// Flag bit indicating this is a relayout boundary.
    pub const IS_RELAYOUT_BOUNDARY: u8 = 1 << 6;

    /// All dirty flags combined (layout + paint).
    pub const ALL_DIRTY: u8 = Self::NEEDS_LAYOUT | Self::NEEDS_PAINT;

    /// All needs-* flags combined.
    pub const ALL_NEEDS: u8 = Self::NEEDS_LAYOUT
        | Self::NEEDS_PAINT
        | Self::NEEDS_COMPOSITING_BITS_UPDATE
        | Self::NEEDS_SEMANTICS_UPDATE
        | Self::NEEDS_ACCESSIBILITY_UPDATE;

    // ========================================================================
    // CONSTRUCTORS
    // ========================================================================

    /// Creates new dirty flags with no flags set.
    #[inline]
    pub const fn new() -> Self {
        Self {
            flags: AtomicU8::new(0),
        }
    }

    /// Creates new dirty flags with initial layout dirty.
    #[inline]
    pub const fn new_needs_layout() -> Self {
        Self {
            flags: AtomicU8::new(Self::NEEDS_LAYOUT),
        }
    }

    /// Creates new dirty flags with initial paint dirty.
    #[inline]
    pub const fn new_needs_paint() -> Self {
        Self {
            flags: AtomicU8::new(Self::NEEDS_PAINT),
        }
    }

    /// Creates new dirty flags with both layout and paint dirty.
    #[inline]
    pub const fn new_dirty() -> Self {
        Self {
            flags: AtomicU8::new(Self::ALL_DIRTY),
        }
    }

    /// Creates new dirty flags with specified initial flags.
    #[inline]
    pub const fn with_flags(flags: u8) -> Self {
        Self {
            flags: AtomicU8::new(flags),
        }
    }

    // ========================================================================
    // LOW-LEVEL OPERATIONS
    // ========================================================================

    /// Sets one or more flags atomically.
    #[inline]
    pub fn set(&self, flag: u8) {
        self.flags.fetch_or(flag, Ordering::SeqCst);
    }

    /// Clears one or more flags atomically.
    #[inline]
    pub fn clear(&self, flag: u8) {
        self.flags.fetch_and(!flag, Ordering::SeqCst);
    }

    /// Returns `true` if the specified flag(s) are set.
    #[inline]
    pub fn is_set(&self, flag: u8) -> bool {
        (self.flags.load(Ordering::SeqCst) & flag) != 0
    }

    /// Returns `true` if all specified flags are set.
    #[inline]
    pub fn all_set(&self, flags: u8) -> bool {
        (self.flags.load(Ordering::SeqCst) & flags) == flags
    }

    /// Returns the raw flags value.
    #[inline]
    pub fn get(&self) -> u8 {
        self.flags.load(Ordering::SeqCst)
    }

    /// Sets all flags to a specific value.
    #[inline]
    pub fn store(&self, value: u8) {
        self.flags.store(value, Ordering::SeqCst);
    }

    /// Clears all flags.
    #[inline]
    pub fn clear_all(&self) {
        self.flags.store(0, Ordering::SeqCst);
    }

    /// Toggles one or more flags atomically.
    #[inline]
    pub fn toggle(&self, flag: u8) {
        self.flags.fetch_xor(flag, Ordering::SeqCst);
    }

    /// Atomically sets flags and returns the previous value.
    #[inline]
    pub fn fetch_set(&self, flag: u8) -> u8 {
        self.flags.fetch_or(flag, Ordering::SeqCst)
    }

    /// Atomically clears flags and returns the previous value.
    #[inline]
    pub fn fetch_clear(&self, flag: u8) -> u8 {
        self.flags.fetch_and(!flag, Ordering::SeqCst)
    }

    // ========================================================================
    // RELAXED OPERATIONS (for performance-critical code)
    // ========================================================================

    /// Sets flags with relaxed ordering.
    #[inline]
    pub fn set_relaxed(&self, flag: u8) {
        self.flags.fetch_or(flag, Ordering::Relaxed);
    }

    /// Clears flags with relaxed ordering.
    #[inline]
    pub fn clear_relaxed(&self, flag: u8) {
        self.flags.fetch_and(!flag, Ordering::Relaxed);
    }

    /// Returns `true` if flags are set (relaxed ordering).
    #[inline]
    pub fn is_set_relaxed(&self, flag: u8) -> bool {
        (self.flags.load(Ordering::Relaxed) & flag) != 0
    }

    /// Returns the raw flags value (relaxed ordering).
    #[inline]
    pub fn get_relaxed(&self) -> u8 {
        self.flags.load(Ordering::Relaxed)
    }

    // ========================================================================
    // LAYOUT CONVENIENCE METHODS
    // ========================================================================

    /// Sets the needs-layout flag.
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.set(Self::NEEDS_LAYOUT);
    }

    /// Clears the needs-layout flag.
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.clear(Self::NEEDS_LAYOUT);
    }

    /// Returns `true` if needs-layout is set.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.is_set(Self::NEEDS_LAYOUT)
    }

    // ========================================================================
    // PAINT CONVENIENCE METHODS
    // ========================================================================

    /// Sets the needs-paint flag.
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.set(Self::NEEDS_PAINT);
    }

    /// Clears the needs-paint flag.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.clear(Self::NEEDS_PAINT);
    }

    /// Returns `true` if needs-paint is set.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.is_set(Self::NEEDS_PAINT)
    }

    // ========================================================================
    // COMPOSITING CONVENIENCE METHODS
    // ========================================================================

    /// Sets the needs-compositing-bits-update flag.
    #[inline]
    pub fn mark_needs_compositing_bits_update(&self) {
        self.set(Self::NEEDS_COMPOSITING_BITS_UPDATE);
    }

    /// Clears the needs-compositing-bits-update flag.
    #[inline]
    pub fn clear_needs_compositing_bits_update(&self) {
        self.clear(Self::NEEDS_COMPOSITING_BITS_UPDATE);
    }

    /// Returns `true` if needs-compositing-bits-update is set.
    #[inline]
    pub fn needs_compositing_bits_update(&self) -> bool {
        self.is_set(Self::NEEDS_COMPOSITING_BITS_UPDATE)
    }

    // ========================================================================
    // SEMANTICS CONVENIENCE METHODS
    // ========================================================================

    /// Sets the needs-semantics-update flag.
    #[inline]
    pub fn mark_needs_semantics_update(&self) {
        self.set(Self::NEEDS_SEMANTICS_UPDATE);
    }

    /// Clears the needs-semantics-update flag.
    #[inline]
    pub fn clear_needs_semantics_update(&self) {
        self.clear(Self::NEEDS_SEMANTICS_UPDATE);
    }

    /// Returns `true` if needs-semantics-update is set.
    #[inline]
    pub fn needs_semantics_update(&self) -> bool {
        self.is_set(Self::NEEDS_SEMANTICS_UPDATE)
    }

    // ========================================================================
    // ACCESSIBILITY CONVENIENCE METHODS
    // ========================================================================

    /// Sets the needs-accessibility-update flag.
    #[inline]
    pub fn mark_needs_accessibility_update(&self) {
        self.set(Self::NEEDS_ACCESSIBILITY_UPDATE);
    }

    /// Clears the needs-accessibility-update flag.
    #[inline]
    pub fn clear_needs_accessibility_update(&self) {
        self.clear(Self::NEEDS_ACCESSIBILITY_UPDATE);
    }

    /// Returns `true` if needs-accessibility-update is set.
    #[inline]
    pub fn needs_accessibility_update(&self) -> bool {
        self.is_set(Self::NEEDS_ACCESSIBILITY_UPDATE)
    }

    // ========================================================================
    // BOUNDARY METHODS
    // ========================================================================

    /// Sets the is-repaint-boundary flag.
    #[inline]
    pub fn set_repaint_boundary(&self, value: bool) {
        if value {
            self.set(Self::IS_REPAINT_BOUNDARY);
        } else {
            self.clear(Self::IS_REPAINT_BOUNDARY);
        }
    }

    /// Returns `true` if this is a repaint boundary.
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.is_set(Self::IS_REPAINT_BOUNDARY)
    }

    /// Sets the is-relayout-boundary flag.
    #[inline]
    pub fn set_relayout_boundary(&self, value: bool) {
        if value {
            self.set(Self::IS_RELAYOUT_BOUNDARY);
        } else {
            self.clear(Self::IS_RELAYOUT_BOUNDARY);
        }
    }

    /// Returns `true` if this is a relayout boundary.
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.is_set(Self::IS_RELAYOUT_BOUNDARY)
    }

    // ========================================================================
    // COMBINED OPERATIONS
    // ========================================================================

    /// Returns `true` if any dirty flag is set (layout or paint).
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.is_set(Self::ALL_DIRTY)
    }

    /// Returns `true` if any needs-* flag is set.
    #[inline]
    pub fn needs_any_update(&self) -> bool {
        self.is_set(Self::ALL_NEEDS)
    }

    /// Marks both layout and paint as needed.
    #[inline]
    pub fn mark_needs_rebuild(&self) {
        self.set(Self::ALL_DIRTY);
    }

    /// Clears both layout and paint flags.
    #[inline]
    pub fn clear_dirty(&self) {
        self.clear(Self::ALL_DIRTY);
    }

    /// Clears all needs-* flags.
    #[inline]
    pub fn clear_all_needs(&self) {
        self.clear(Self::ALL_NEEDS);
    }
}

impl Default for AtomicDirtyFlags {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AtomicDirtyFlags {
    fn clone(&self) -> Self {
        Self {
            flags: AtomicU8::new(self.get()),
        }
    }
}

impl PartialEq for AtomicDirtyFlags {
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}

impl Eq for AtomicDirtyFlags {}

impl std::hash::Hash for AtomicDirtyFlags {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.get().hash(state);
    }
}

impl std::fmt::Display for AtomicDirtyFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let flags = self.get();
        let mut parts = Vec::new();

        if flags & Self::NEEDS_LAYOUT != 0 {
            parts.push("NEEDS_LAYOUT");
        }
        if flags & Self::NEEDS_PAINT != 0 {
            parts.push("NEEDS_PAINT");
        }
        if flags & Self::NEEDS_COMPOSITING_BITS_UPDATE != 0 {
            parts.push("NEEDS_COMPOSITING");
        }
        if flags & Self::NEEDS_SEMANTICS_UPDATE != 0 {
            parts.push("NEEDS_SEMANTICS");
        }
        if flags & Self::NEEDS_ACCESSIBILITY_UPDATE != 0 {
            parts.push("NEEDS_ACCESSIBILITY");
        }
        if flags & Self::IS_REPAINT_BOUNDARY != 0 {
            parts.push("REPAINT_BOUNDARY");
        }
        if flags & Self::IS_RELAYOUT_BOUNDARY != 0 {
            parts.push("RELAYOUT_BOUNDARY");
        }

        if parts.is_empty() {
            write!(f, "AtomicDirtyFlags(none)")
        } else {
            write!(f, "AtomicDirtyFlags({})", parts.join(" | "))
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // ATOMIC DIRTY FLAGS TESTS
    // ========================================================================

    #[test]
    fn test_atomic_dirty_flags_new() {
        let flags = AtomicDirtyFlags::new();
        assert!(!flags.needs_layout());
        assert!(!flags.needs_paint());
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_constructors() {
        let layout = AtomicDirtyFlags::new_needs_layout();
        assert!(layout.needs_layout());
        assert!(!layout.needs_paint());

        let paint = AtomicDirtyFlags::new_needs_paint();
        assert!(!paint.needs_layout());
        assert!(paint.needs_paint());

        let dirty = AtomicDirtyFlags::new_dirty();
        assert!(dirty.needs_layout());
        assert!(dirty.needs_paint());
    }

    #[test]
    fn test_atomic_dirty_flags_layout() {
        let flags = AtomicDirtyFlags::new();

        flags.mark_needs_layout();
        assert!(flags.needs_layout());
        assert!(!flags.needs_paint());
        assert!(flags.is_dirty());

        flags.clear_needs_layout();
        assert!(!flags.needs_layout());
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_paint() {
        let flags = AtomicDirtyFlags::new();

        flags.mark_needs_paint();
        assert!(!flags.needs_layout());
        assert!(flags.needs_paint());
        assert!(flags.is_dirty());

        flags.clear_needs_paint();
        assert!(!flags.needs_paint());
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_multiple() {
        let flags = AtomicDirtyFlags::new();

        flags.mark_needs_layout();
        flags.mark_needs_paint();

        assert!(flags.needs_layout());
        assert!(flags.needs_paint());
        assert!(flags.all_set(AtomicDirtyFlags::ALL_DIRTY));

        flags.clear_needs_layout();
        assert!(!flags.needs_layout());
        assert!(flags.needs_paint());

        flags.clear_all();
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_boundaries() {
        let flags = AtomicDirtyFlags::new();

        flags.set_repaint_boundary(true);
        assert!(flags.is_repaint_boundary());

        flags.set_relayout_boundary(true);
        assert!(flags.is_relayout_boundary());

        flags.set_repaint_boundary(false);
        assert!(!flags.is_repaint_boundary());
        assert!(flags.is_relayout_boundary());
    }

    #[test]
    fn test_atomic_dirty_flags_toggle() {
        let flags = AtomicDirtyFlags::new();

        flags.toggle(AtomicDirtyFlags::NEEDS_LAYOUT);
        assert!(flags.needs_layout());

        flags.toggle(AtomicDirtyFlags::NEEDS_LAYOUT);
        assert!(!flags.needs_layout());
    }

    #[test]
    fn test_atomic_dirty_flags_fetch_operations() {
        let flags = AtomicDirtyFlags::new();

        let prev = flags.fetch_set(AtomicDirtyFlags::NEEDS_LAYOUT);
        assert_eq!(prev, 0);
        assert!(flags.needs_layout());

        let prev = flags.fetch_clear(AtomicDirtyFlags::NEEDS_LAYOUT);
        assert!(prev & AtomicDirtyFlags::NEEDS_LAYOUT != 0);
        assert!(!flags.needs_layout());
    }

    #[test]
    fn test_atomic_dirty_flags_clone() {
        let flags = AtomicDirtyFlags::new();
        flags.mark_needs_layout();

        let cloned = flags.clone();
        assert!(cloned.needs_layout());
    }

    #[test]
    fn test_atomic_dirty_flags_display() {
        let flags = AtomicDirtyFlags::new();
        assert_eq!(format!("{}", flags), "AtomicDirtyFlags(none)");

        flags.mark_needs_layout();
        assert!(format!("{}", flags).contains("NEEDS_LAYOUT"));

        flags.mark_needs_paint();
        assert!(format!("{}", flags).contains("NEEDS_PAINT"));
    }

    #[test]
    fn test_atomic_dirty_flags_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let flags = Arc::new(AtomicDirtyFlags::new());
        let mut handles = vec![];

        // Spawn multiple threads that mark flags
        for _ in 0..10 {
            let flags = Arc::clone(&flags);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    flags.mark_needs_layout();
                    flags.mark_needs_paint();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Flags should be set
        assert!(flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_relaxed_operations() {
        let flags = AtomicDirtyFlags::new();

        flags.set_relaxed(AtomicDirtyFlags::NEEDS_LAYOUT);
        assert!(flags.is_set_relaxed(AtomicDirtyFlags::NEEDS_LAYOUT));

        flags.clear_relaxed(AtomicDirtyFlags::NEEDS_LAYOUT);
        assert!(!flags.is_set_relaxed(AtomicDirtyFlags::NEEDS_LAYOUT));
    }

    // ========================================================================
    // DIRTY TRACKING EXT TESTS
    // ========================================================================

    // Simple test implementation for DirtyTracking
    struct TestDirtyTracker {
        layout_dirty: std::sync::Mutex<std::collections::HashSet<ElementId>>,
        paint_dirty: std::sync::Mutex<std::collections::HashSet<ElementId>>,
    }

    impl TestDirtyTracker {
        fn new() -> Self {
            Self {
                layout_dirty: std::sync::Mutex::new(std::collections::HashSet::new()),
                paint_dirty: std::sync::Mutex::new(std::collections::HashSet::new()),
            }
        }
    }

    impl DirtyTracking for TestDirtyTracker {
        fn mark_needs_layout(&self, id: ElementId) {
            self.layout_dirty.lock().unwrap().insert(id);
        }

        fn mark_needs_paint(&self, id: ElementId) {
            self.paint_dirty.lock().unwrap().insert(id);
        }

        fn clear_needs_layout(&self, id: ElementId) {
            self.layout_dirty.lock().unwrap().remove(&id);
        }

        fn clear_needs_paint(&self, id: ElementId) {
            self.paint_dirty.lock().unwrap().remove(&id);
        }

        fn needs_layout(&self, id: ElementId) -> bool {
            self.layout_dirty.lock().unwrap().contains(&id)
        }

        fn needs_paint(&self, id: ElementId) -> bool {
            self.paint_dirty.lock().unwrap().contains(&id)
        }

        fn elements_needing_layout(&self) -> Vec<ElementId> {
            self.layout_dirty.lock().unwrap().iter().copied().collect()
        }

        fn elements_needing_paint(&self) -> Vec<ElementId> {
            self.paint_dirty.lock().unwrap().iter().copied().collect()
        }
    }

    // Note: TestDirtyTracker automatically gets DirtyTrackingExt via blanket impl

    #[test]
    fn test_dirty_tracking_ext_batch_mark() {
        let tracker = TestDirtyTracker::new();
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];

        tracker.mark_many_needs_layout(ids.iter().copied());
        assert_eq!(tracker.layout_dirty_count(), 3);

        tracker.mark_many_needs_paint(ids.iter().copied());
        assert_eq!(tracker.paint_dirty_count(), 3);
    }

    #[test]
    fn test_dirty_tracking_ext_batch_clear() {
        let tracker = TestDirtyTracker::new();
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];

        tracker.mark_many_needs_layout(ids.iter().copied());
        tracker.clear_many_layout(ids[..2].iter().copied());
        assert_eq!(tracker.layout_dirty_count(), 1);
    }

    #[test]
    fn test_dirty_tracking_ext_drain() {
        let tracker = TestDirtyTracker::new();
        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);

        tracker.mark_needs_layout(id1);
        tracker.mark_needs_layout(id2);

        let drained = tracker.drain_needing_layout();
        assert_eq!(drained.len(), 2);
        assert_eq!(tracker.layout_dirty_count(), 0);
    }

    #[test]
    fn test_dirty_tracking_ext_conditional() {
        let tracker = TestDirtyTracker::new();
        let id = ElementId::new(1);

        tracker.mark_needs_layout_if(id, false);
        assert!(!tracker.needs_layout(id));

        tracker.mark_needs_layout_if(id, true);
        assert!(tracker.needs_layout(id));
    }
}
