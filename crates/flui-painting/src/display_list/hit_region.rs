//! Hit-testable regions + pointer event types for display lists.
//!
//! Mythos chain U5 extracted these from the 2,434-LOC
//! `display_list.rs` god module. `HitRegion` carries an
//! `Arc<dyn Fn(&PointerEvent) + Send + Sync>` handler that
//! `flui-interaction`'s hit-test pump dispatches to on pointer events.
//! The handler heap allocation happens once at registration time, not
//! per command.

use std::{sync::Arc, time::Duration};

use flui_types::geometry::{Offset, Pixels, Point, Rect};

/// A pointer event for hit testing in display lists.
///
/// This is a minimal event type used for hit region handlers.
/// The full event system is in `flui_interaction`.
#[derive(Debug, Clone)]
pub struct PointerEvent {
    /// The type of pointer event.
    pub kind: PointerEventKind,
    /// The position of the event in local coordinates.
    pub position: Offset<Pixels>,
    /// The pointer ID.
    pub pointer: i32,
    /// The button state (for mouse events).
    pub buttons: i32,
    /// Time of the event.
    pub time_stamp: Duration,
}

/// The kind of pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerEventKind {
    /// Pointer entered a region.
    Enter,
    /// Pointer exited a region.
    Exit,
    /// Pointer button pressed.
    Down,
    /// Pointer moved.
    Move,
    /// Pointer button released.
    Up,
    /// Pointer interaction cancelled.
    Cancel,
}

impl PointerEvent {
    /// Create a new pointer event.
    pub fn new(kind: PointerEventKind, position: Offset<Pixels>, pointer: i32) -> Self {
        Self {
            kind,
            position,
            pointer,
            buttons: 0,
            time_stamp: Duration::ZERO,
        }
    }
}

/// Handler for pointer events in a hit region.
///
/// Unlike `flui_interaction`'s handler which returns
/// `EventPropagation`, this is a simpler callback that just receives
/// the event.
pub type HitRegionHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>;

/// A hit-testable region with an event handler.
///
/// HitRegions are added to DisplayList to enable event handling for
/// specific areas. When hit testing occurs, regions are checked in
/// reverse order (last added = topmost).
#[derive(Clone)]
pub struct HitRegion {
    /// Bounds of the hit-testable area.
    pub bounds: Rect<Pixels>,
    /// Handler to call when pointer events occur in this region.
    pub handler: HitRegionHandler,
}

impl HitRegion {
    /// Create a new hit region.
    pub fn new(bounds: Rect<Pixels>, handler: HitRegionHandler) -> Self {
        Self { bounds, handler }
    }

    /// Check if a point is inside this region.
    pub fn contains(&self, point: Point<Pixels>) -> bool {
        self.bounds.contains(point)
    }
}

impl std::fmt::Debug for HitRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitRegion")
            .field("bounds", &self.bounds)
            .field("handler", &"<handler>")
            .finish()
    }
}
