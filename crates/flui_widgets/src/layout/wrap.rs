//! Wrap widget - arranges children with wrapping
//!
//! A widget that displays its children in multiple horizontal or vertical runs.
//! Similar to Flutter's Wrap widget.

use bon::Builder;
use flui_core::BuildContext;

use flui_core::view::{AnyView, IntoElement, RenderBuilder, View};
use flui_rendering::{RenderWrap, WrapAlignment, WrapCrossAlignment};
use flui_types::Axis;

pub use flui_rendering::{
    WrapAlignment as WrapAlignmentExport, WrapCrossAlignment as WrapCrossAlignmentExport,
};

/// A widget that displays its children in multiple horizontal or vertical runs.
///
/// Wrap arranges children like Row or Column, but wraps to the next line when
/// reaching the edge of the container. This is useful for tags, chips, or any
/// content that should flow naturally.
///
/// ## Key Properties
///
/// - **direction**: Main axis direction (Horizontal or Vertical)
/// - **alignment**: How to align runs along main axis
/// - **spacing**: Space between children in a run
/// - **run_spacing**: Space between runs
/// - **cross_alignment**: How to align children within a run
///
/// ## Layout Behavior
///
/// 1. Lays out children in order along main axis
/// 2. When reaching edge, wraps to next run
/// 3. Aligns runs according to alignment
/// 4. Aligns children within run according to cross_alignment
///
/// ## Common Use Cases
///
/// ### Tag cloud
/// ```rust,ignore
/// Wrap::builder()
///     .direction(Axis::Horizontal)
///     .spacing(8.0)
///     .run_spacing(8.0)
///     .children(vec![
///         Chip::new("Rust"),
///         Chip::new("Flutter"),
///         Chip::new("Widgets"),
///     ])
///     .build()
/// ```
///
/// ### Button group with wrapping
/// ```rust,ignore
/// Wrap::builder()
///     .spacing(12.0)
///     .alignment(WrapAlignment::Center)
///     .children(buttons)
///     .build()
/// ```
///
/// ### Vertical tag list
/// ```rust,ignore
/// Wrap::builder()
///     .direction(Axis::Vertical)
///     .run_spacing(16.0)
///     .children(items)
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Basic horizontal wrap
/// Wrap::new(vec![widget1, widget2, widget3])
///
/// // With spacing
/// Wrap::builder()
///     .spacing(10.0)
///     .run_spacing(10.0)
///     .children(widgets)
///     .build()
///
/// // Centered with spacing
/// Wrap::builder()
///     .alignment(WrapAlignment::Center)
///     .spacing(8.0)
///     .children(widgets)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct Wrap {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Main axis direction
    /// Default: Axis::Horizontal
    #[builder(default = Axis::Horizontal)]
    pub direction: Axis,

    /// Alignment of runs along main axis
    /// Default: WrapAlignment::Start
    #[builder(default = WrapAlignment::Start)]
    pub alignment: WrapAlignment,

    /// Spacing between children in a run
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub spacing: f32,

    /// Spacing between runs
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub run_spacing: f32,

    /// Cross-axis alignment within a run
    /// Default: WrapCrossAlignment::Start
    #[builder(default = WrapCrossAlignment::Start)]
    pub cross_alignment: WrapCrossAlignment,

    /// The children widgets
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Vec<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Wrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wrap")
            .field("key", &self.key)
            .field("direction", &self.direction)
            .field("alignment", &self.alignment)
            .field("spacing", &self.spacing)
            .field("run_spacing", &self.run_spacing)
            .field("cross_alignment", &self.cross_alignment)
            .field(
                "children",
                &if !self.children.is_empty() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for Wrap {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            direction: self.direction,
            alignment: self.alignment,
            spacing: self.spacing,
            run_spacing: self.run_spacing,
            cross_alignment: self.cross_alignment,
            children: self.children.clone(),
        }
    }
}

impl Wrap {
    /// Creates a new Wrap with default settings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let wrap = Wrap::new(vec![
    ///     Box::new(widget1) as Box<dyn AnyView>,
    ///     Box::new(widget2) as Box<dyn AnyView>,
    /// ]);
    /// ```
    pub fn new(children: Vec<Box<dyn AnyView>>) -> Self {
        Self {
            key: None,
            direction: Axis::Horizontal,
            alignment: WrapAlignment::Start,
            spacing: 0.0,
            run_spacing: 0.0,
            cross_alignment: WrapCrossAlignment::Start,
            children,
        }
    }

    /// Creates a horizontal Wrap.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let wrap = Wrap::horizontal(children);
    /// ```
    pub fn horizontal(children: Vec<Box<dyn AnyView>>) -> Self {
        Self {
            direction: Axis::Horizontal,
            ..Self::new(children)
        }
    }

    /// Creates a vertical Wrap.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let wrap = Wrap::vertical(children);
    /// ```
    pub fn vertical(children: Vec<Box<dyn AnyView>>) -> Self {
        Self {
            direction: Axis::Vertical,
            ..Self::new(children)
        }
    }

    /// Adds a child widget.
    pub fn add_child(&mut self, child: impl View + 'static) {
        self.children.push(Box::new(child));
    }

    /// Sets all children at once.
    pub fn set_children(&mut self, children: Vec<Box<dyn AnyView>>) {
        self.children = children;
    }
}

impl Default for Wrap {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

// Implement View for Wrap - New architecture
impl View for Wrap {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut render_wrap = RenderWrap::new(self.direction);
        render_wrap.alignment = self.alignment;
        render_wrap.spacing = self.spacing;
        render_wrap.run_spacing = self.run_spacing;
        render_wrap.cross_alignment = self.cross_alignment;

        RenderBuilder::multi(render_wrap).with_children(self.children)
    }
}

// bon Builder Extensions
use wrap_builder::{IsUnset, SetChildren, State};

// Custom setter for children
impl<S: State> WrapBuilder<S>
where
    S::Children: IsUnset,
{
    /// Sets the children widgets (works in builder chain).
    pub fn children(self, children: Vec<Box<dyn AnyView>>) -> WrapBuilder<SetChildren<S>> {
        self.children_internal(children)
    }
}

// Public build() wrapper
impl<S: State> WrapBuilder<S> {
    /// Builds the Wrap widget.
    pub fn build(self) -> Wrap {
        self.build_internal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::view::RenderBuilder;

    // Mock view for testing
    #[derive()]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            RenderBuilder::leaf(RenderPadding::new(EdgeInsets::ZERO))
        }
    }

    #[test]
    fn test_wrap_new() {
        let children = vec![Box::new(MockView) as Box<dyn AnyView>, Box::new(MockView)];
        let wrap = Wrap::new(children);
        assert_eq!(wrap.direction, Axis::Horizontal);
        assert_eq!(wrap.alignment, WrapAlignment::Start);
        assert_eq!(wrap.spacing, 0.0);
        assert_eq!(wrap.run_spacing, 0.0);
        assert_eq!(wrap.children.len(), 2);
    }

    #[test]
    fn test_wrap_horizontal() {
        let children = vec![Box::new(MockView) as Box<dyn AnyView>];
        let wrap = Wrap::horizontal(children);
        assert_eq!(wrap.direction, Axis::Horizontal);
    }

    #[test]
    fn test_wrap_vertical() {
        let children = vec![Box::new(MockView) as Box<dyn AnyView>];
        let wrap = Wrap::vertical(children);
        assert_eq!(wrap.direction, Axis::Vertical);
    }

    #[test]
    fn test_wrap_builder() {
        let wrap = Wrap::builder()
            .direction(Axis::Vertical)
            .spacing(10.0)
            .run_spacing(5.0)
            .alignment(WrapAlignment::Center)
            .cross_alignment(WrapCrossAlignment::End)
            .children(vec![Box::new(MockView), Box::new(MockView)])
            .build();

        assert_eq!(wrap.direction, Axis::Vertical);
        assert_eq!(wrap.spacing, 10.0);
        assert_eq!(wrap.run_spacing, 5.0);
        assert_eq!(wrap.alignment, WrapAlignment::Center);
        assert_eq!(wrap.cross_alignment, WrapCrossAlignment::End);
        assert_eq!(wrap.children.len(), 2);
    }

    #[test]
    fn test_wrap_default() {
        let wrap = Wrap::default();
        assert_eq!(wrap.children.len(), 0);
        assert_eq!(wrap.direction, Axis::Horizontal);
    }

    #[test]
    fn test_wrap_add_child() {
        let mut wrap = Wrap::default();
        wrap.add_child(MockView);
        wrap.add_child(MockView);
        assert_eq!(wrap.children.len(), 2);
    }

    #[test]
    fn test_wrap_set_children() {
        let mut wrap = Wrap::default();
        wrap.set_children(vec![
            Box::new(MockView),
            Box::new(MockView),
            Box::new(MockView),
        ]);
        assert_eq!(wrap.children.len(), 3);
    }
}

// Wrap now implements View trait directly
