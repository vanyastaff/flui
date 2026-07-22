//! Custom painter delegate for custom painting on a canvas.
//!
//! [`CustomPainter`] allows users to implement custom painting behavior
//! without creating a new render object. It provides methods for painting,
//! hit testing, and accessibility.

use std::{any::Any, fmt::Debug, sync::Arc, sync::Once};

use flui_foundation::Listenable;
use flui_painting::Canvas;
use flui_types::{Offset, Size};

/// Builder for semantics information.
///
/// **INCOMPLETE**: This is a placeholder type. Custom-painter semantics
/// callbacks are not yet modeled; the builder accepts no operations and is
/// consumed by platform integrations as an empty shell.
///
/// `SemanticsBuilder::new` used to panic via `unimplemented!()` on
/// construction, which is unacceptable for any consumer of
/// `CustomPainter::semantics_builder` reachable from production code. The
/// constructor now returns an inert empty builder and emits a
/// **one-shot** `tracing::warn!` so the missing-impl notice surfaces in
/// logs without aborting the process and without spamming
/// per-construction. The one-shot gate uses [`std::sync::Once`]; the
/// `Default` impl delegates to [`Self::new`] explicitly so the warn
/// fires through either constructor path.
#[derive(Debug, Clone)]
pub struct SemanticsBuilder {
    _private: (),
}

/// One-shot guard so the "semantics not wired" warn fires at most once
/// per process. Repeated construction during paint passes would
/// otherwise produce log spam.
static WARN_ONCE: Once = Once::new();

impl SemanticsBuilder {
    /// Creates a new empty semantics builder.
    ///
    /// Currently a no-op shell — custom-painter semantics build operations
    /// land when `CustomPainter` exposes a real semantics-builder contract.
    /// See `docs/research/2026-05-22-flui-rendering-engine-audit.md` for
    /// the background on this gap.
    ///
    /// On the first call per process emits a `tracing::warn!`; the
    /// `Once` gate suppresses subsequent warns to avoid per-frame log
    /// spam during paint passes.
    #[must_use]
    pub fn new() -> Self {
        WARN_ONCE.call_once(|| {
            tracing::warn!(
                "SemanticsBuilder: custom-painter semantics build operations are a no-op \
                 until CustomPainter exposes a real semantics-builder contract; \
                 the returned builder accepts no operations (this warn fires \
                 once per process)"
            );
        });
        Self { _private: () }
    }
}

impl Default for SemanticsBuilder {
    fn default() -> Self {
        // Explicit delegation so the `Once`-gated warn fires regardless of
        // which constructor the caller picks. A prior `#[derive(Default)]`
        // bypassed `new()` entirely, which skipped the warn.
        Self::new()
    }
}

/// A delegate that provides custom painting behavior.
///
/// Implement this trait to define custom painting on a canvas. The delegate
/// is used by `RenderCustomPaint` to paint content before or after its child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::delegates::CustomPainter;
/// use flui_rendering::pipeline::Canvas;
/// use flui_types::Size;
///
/// #[derive(Debug)]
/// struct CheckerboardPainter {
///     cell_size: f32,
/// }
///
/// impl CustomPainter for CheckerboardPainter {
///     fn paint(&self, canvas: &mut Canvas, size: Size) {
///         let cols = (size.width / self.cell_size).ceil() as i32;
///         let rows = (size.height / self.cell_size).ceil() as i32;
///         // Draw checkerboard pattern...
///     }
///
///     fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
///         if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
///             self.cell_size != old.cell_size
///         } else {
///             true
///         }
///     }
/// }
/// ```
pub trait CustomPainter: Send + Sync + Debug {
    /// Paint custom content on the canvas.
    ///
    /// The canvas coordinate space is configured such that the origin is at
    /// the top left of the box. The area of the box is the size argument.
    ///
    /// Paint operations should remain inside the given area.
    fn paint(&self, canvas: &mut Canvas, size: Size);

    /// Whether this painter should repaint when replaced with a new delegate.
    ///
    /// Called when a new instance of the delegate is provided, to check if
    /// the new instance represents different information that requires
    /// repainting.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous painter delegate
    ///
    /// # Returns
    ///
    /// `true` if the painter should repaint, `false` otherwise.
    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool;

    /// An optional repaint [`Listenable`]: when it notifies, the hosting
    /// `RenderCustomPaint` marks itself needing paint — the FLUI equivalent of
    /// Flutter's `CustomPainter(repaint:)` / `addListener`/`removeListener`
    /// wiring, which lets a painter driven by an [`Animation`] (or any
    /// `ChangeNotifier`) repaint without a widget rebuild.
    ///
    /// Implementations that return `Some` MUST return the *same* instance
    /// across calls, so the host can unsubscribe on detach / painter swap.
    /// Defaults to `None` (a static painter that never self-invalidates).
    ///
    /// [`Animation`]: https://api.flutter.dev/flutter/animation/Animation-class.html
    fn repaint(&self) -> Option<Arc<dyn Listenable>> {
        None
    }

    /// Hit test at the given position.
    ///
    /// The given position is relative to the same coordinate space as the
    /// last [`Self::paint`] call. Return `true` if the position is a "hit",
    /// `false` if it is a miss, and `None` to use the caller's default
    /// behavior.
    ///
    /// The default implementation returns `None`. `RenderCustomPaint`
    /// resolves the tri-state per Flutter parity: a *background* painter
    /// defaults to hit (`None` → `true`) and a *foreground* painter defaults
    /// to miss (`None` → `false`) — the trait itself is direction-agnostic,
    /// so it cannot pick a single default for both roles.
    fn hit_test(&self, _position: Offset) -> Option<bool> {
        None
    }

    /// Build semantics information for accessibility.
    ///
    /// Returns `Some(SemanticsBuilder)` if the painter provides semantic
    /// information, or `None` if it doesn't contribute to the semantics tree.
    fn semantics_builder(&self) -> Option<SemanticsBuilder> {
        None
    }

    /// Whether to rebuild semantics when the delegate changes.
    ///
    /// Called when a new instance of the delegate is provided, to check if
    /// the semantics information needs to be rebuilt.
    fn should_rebuild_semantics(&self, _old_delegate: &dyn CustomPainter) -> bool {
        true
    }

    /// Returns self as `Any` for downcasting.
    ///
    /// This enables comparing delegates of the same concrete type in
    /// `should_repaint` and `should_rebuild_semantics`.
    fn as_any(&self) -> &dyn Any;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestPainter {
        color: u32,
    }

    impl CustomPainter for TestPainter {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {
            // Test painting
        }

        fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
            if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
                self.color != old.color
            } else {
                true
            }
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn test_should_repaint_same_type() {
        let painter1 = TestPainter { color: 0x00FF_0000 };
        let painter2 = TestPainter { color: 0x00FF_0000 };
        let painter3 = TestPainter { color: 0x0000_FF00 };

        assert!(!painter1.should_repaint(&painter2));
        assert!(painter1.should_repaint(&painter3));
    }

    #[test]
    fn test_default_hit_test() {
        let painter = TestPainter { color: 0x00FF_0000 };
        assert_eq!(painter.hit_test(Offset::ZERO), None);
    }

    #[test]
    fn test_default_semantics() {
        let painter = TestPainter { color: 0x00FF_0000 };
        assert!(painter.semantics_builder().is_none());
    }
}
