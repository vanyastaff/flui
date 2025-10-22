//! Utility macros for RenderObjects

/// Delegate all standard methods from DynRenderObject to RenderBoxMixin
///
/// This macro generates implementations for all the boilerplate methods
/// that simply delegate to the RenderBoxMixin trait.
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
            $crate::core::RenderBoxMixin::size(self).unwrap_or(flui_types::Size::ZERO)
        }

        fn mark_needs_layout(&mut self) {
            $crate::core::RenderBoxMixin::mark_needs_layout(self);
        }

        fn mark_needs_paint(&mut self) {
            $crate::core::RenderBoxMixin::mark_needs_paint(self);
        }

        fn needs_layout(&self) -> bool {
            $crate::core::RenderBoxMixin::needs_layout(self)
        }

        fn needs_paint(&self) -> bool {
            $crate::core::RenderBoxMixin::needs_paint(self)
        }

        fn constraints(&self) -> Option<flui_types::constraints::BoxConstraints> {
            $crate::core::RenderBoxMixin::constraints(self)
        }
    };
}
