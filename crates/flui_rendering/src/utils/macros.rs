//! Utility macros for RenderObjects

/// Delegate standard DynRenderObject methods to internal state
///
/// This macro generates implementations for common DynRenderObject methods
/// by accessing the internal RenderState through RenderBoxMixin.
///
/// # Methods Delegated
///
/// - `size()` - Returns size from state (or Size::ZERO if not laid out)
/// - `mark_needs_layout()` - Marks object as needing layout
/// - `mark_needs_paint()` - Marks object as needing paint
/// - `needs_layout()` - Checks if layout is needed
/// - `needs_paint()` - Checks if paint is needed
/// - `constraints()` - Returns constraints from last layout
///
/// # Usage
///
/// ```rust,ignore
/// impl DynRenderObject for RenderPadding {
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         // Custom layout logic
///     }
///
///     fn paint(&self, painter: &egui::Painter, offset: Offset) {
///         // Custom paint logic
///     }
///
///     delegate_to_mixin!();
/// }
/// ```
#[macro_export]
macro_rules! delegate_to_mixin {
    () => {
        fn size(&self) -> flui_types::Size {
            self.state().size.unwrap_or(flui_types::Size::ZERO)
        }

        fn mark_needs_layout(&mut self) {
            self.state_mut().mark_needs_layout();
        }

        fn mark_needs_paint(&mut self) {
            self.state_mut().mark_needs_paint();
        }

        fn needs_layout(&self) -> bool {
            self.state().needs_layout()
        }

        fn needs_paint(&self) -> bool {
            self.state().needs_paint()
        }

        fn constraints(&self) -> Option<flui_types::constraints::BoxConstraints> {
            self.state().constraints
        }
    };
}
