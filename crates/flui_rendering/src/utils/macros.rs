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
///     fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
///         // Custom layout logic
///     }
///
///     fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
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
            self.state().size.lock().unwrap_or(flui_types::Size::ZERO)
        }

        fn mark_needs_layout(&self) {
            self.state().mark_needs_layout();
        }

        fn mark_needs_paint(&self) {
            self.state().mark_needs_paint();
        }

        fn needs_layout(&self) -> bool {
            self.state().needs_layout()
        }

        fn needs_paint(&self) -> bool {
            self.state().needs_paint()
        }

        fn constraints(&self) -> Option<flui_types::constraints::BoxConstraints> {
            *self.state().constraints.lock()
        }
    };
}

/// Implement adopt_child for single-child RenderObjects
///
/// This macro provides the standard adopt_child implementation for all
/// SingleRenderBox<T> types by delegating to the inherent method.
///
/// # Usage
///
/// ```rust,ignore
/// impl DynRenderObject for RenderPadding {
///     fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
///         // Custom layout logic
///     }
///
///     fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
///         // Custom paint logic
///     }
///
///     impl_adopt_child_single!();
///     delegate_to_mixin!();
/// }
/// ```
#[macro_export]
macro_rules! impl_adopt_child_single {
    () => {
        fn adopt_child(&mut self, child: Box<dyn flui_core::DynRenderObject>) {
            $crate::core::SingleRenderBox::adopt_child(self, child);
        }
    };
}

/// Implement adopt_child for multi-child RenderObjects
///
/// This macro provides the standard adopt_child implementation for all
/// ContainerRenderBox<T> types by delegating to the inherent method.
///
/// # Usage
///
/// ```rust,ignore
/// impl DynRenderObject for RenderFlex {
///     fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
///         // Custom layout logic
///     }
///
///     fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
///         // Custom paint logic
///     }
///
///     impl_adopt_child_multi!();
///     delegate_to_mixin!();
/// }
/// ```
#[macro_export]
macro_rules! impl_adopt_child_multi {
    () => {
        fn adopt_child(&mut self, child: Box<dyn flui_core::DynRenderObject>) {
            $crate::core::ContainerRenderBox::adopt_child(self, child);
        }
    };
}

/// Complete delegation for single-child RenderObjects (ONE macro instead of two!)
///
/// This combines `impl_adopt_child_single!()` + `delegate_to_mixin!()` into one macro.
/// Use this for all SingleRenderBox<T> types.
///
/// # Usage
///
/// ```rust,ignore
/// impl DynRenderObject for RenderPadding {
///     fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size { ... }
///     fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) { ... }
///     delegate_single!();  // That's it! Just one line!
/// }
/// ```
#[macro_export]
macro_rules! delegate_single {
    () => {
        fn size(&self) -> flui_types::Size {
            self.state().size.lock().unwrap_or(flui_types::Size::ZERO)
        }

        fn mark_needs_layout(&self) {
            self.state().mark_needs_layout();
        }

        fn mark_needs_paint(&self) {
            self.state().mark_needs_paint();
        }

        fn needs_layout(&self) -> bool {
            self.state().needs_layout()
        }

        fn needs_paint(&self) -> bool {
            self.state().needs_paint()
        }

        fn constraints(&self) -> Option<flui_types::constraints::BoxConstraints> {
            *self.state().constraints.lock()
        }

        fn adopt_child(&mut self, child: Box<dyn flui_core::DynRenderObject>) {
            $crate::core::SingleRenderBox::adopt_child(self, child);
        }
    };
}

/// Complete delegation for multi-child RenderObjects (ONE macro instead of two!)
///
/// This combines `impl_adopt_child_multi!()` + `delegate_to_mixin!()` into one macro.
/// Use this for all ContainerRenderBox<T> types.
///
/// # Usage
///
/// ```rust,ignore
/// impl DynRenderObject for RenderFlex {
///     fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size { ... }
///     fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) { ... }
///     delegate_multi!();  // That's it! Just one line!
/// }
/// ```
#[macro_export]
macro_rules! delegate_multi {
    () => {
        fn size(&self) -> flui_types::Size {
            self.state().size.lock().unwrap_or(flui_types::Size::ZERO)
        }

        fn mark_needs_layout(&self) {
            self.state().mark_needs_layout();
        }

        fn mark_needs_paint(&self) {
            self.state().mark_needs_paint();
        }

        fn needs_layout(&self) -> bool {
            self.state().needs_layout()
        }

        fn needs_paint(&self) -> bool {
            self.state().needs_paint()
        }

        fn constraints(&self) -> Option<flui_types::constraints::BoxConstraints> {
            *self.state().constraints.lock()
        }

        fn adopt_child(&mut self, child: Box<dyn flui_core::DynRenderObject>) {
            $crate::core::ContainerRenderBox::adopt_child(self, child);
        }
    };
}
