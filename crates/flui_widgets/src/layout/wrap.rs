//! Wrap widget - arranges children with wrapping
//!
//! A widget that displays its children in multiple horizontal or vertical runs.
//! Similar to Flutter's Wrap widget.

use bon::Builder;
use flui_core::widget::{RenderWidget, Widget};
use flui_core::{BuildContext, render::RenderNode};
use flui_rendering::{RenderWrap, WrapAlignment, WrapCrossAlignment};
use flui_types::Axis;

pub use flui_rendering::{WrapAlignment as WrapAlignmentExport, WrapCrossAlignment as WrapCrossAlignmentExport};

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
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_wrap)]
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
    #[builder(default)]
    pub children: Vec<Widget>,
}

impl Wrap {
    /// Creates a new Wrap with default settings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let wrap = Wrap::new(vec![widget1, widget2, widget3]);
    /// ```
    pub fn new(children: Vec<Widget>) -> Self {
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
    pub fn horizontal(children: Vec<Widget>) -> Self {
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
    pub fn vertical(children: Vec<Widget>) -> Self {
        Self {
            direction: Axis::Vertical,
            ..Self::new(children)
        }
    }
}

impl Default for Wrap {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

// bon Builder Extensions
use wrap_builder::State;

impl<S: State> WrapBuilder<S> {
    /// Builds the Wrap widget.
    pub fn build(self) -> Widget {
        Widget::render_object(self.build_wrap())
    }
}

// Implement RenderWidget
impl RenderWidget for Wrap {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        let mut render = RenderWrap::new(self.direction);
        render.alignment = self.alignment;
        render.spacing = self.spacing;
        render.run_spacing = self.run_spacing;
        render.cross_alignment = self.cross_alignment;

        RenderNode::multi(Box::new(render))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Multi { render, .. } = render_object {
            if let Some(wrap) = render.downcast_mut::<RenderWrap>() {
                wrap.direction = self.direction;
                wrap.alignment = self.alignment;
                wrap.spacing = self.spacing;
                wrap.run_spacing = self.run_spacing;
                wrap.cross_alignment = self.cross_alignment;
            }
        }
    }

    fn children(&self) -> Option<&[Widget]> {
        Some(&self.children)
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(Wrap, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_new() {
        let children = vec![Widget::from(()), Widget::from(())];
        let wrap = Wrap::new(children.clone());
        assert_eq!(wrap.direction, Axis::Horizontal);
        assert_eq!(wrap.alignment, WrapAlignment::Start);
        assert_eq!(wrap.spacing, 0.0);
        assert_eq!(wrap.run_spacing, 0.0);
        assert_eq!(wrap.children.len(), 2);
    }

    #[test]
    fn test_wrap_horizontal() {
        let children = vec![Widget::from(())];
        let wrap = Wrap::horizontal(children);
        assert_eq!(wrap.direction, Axis::Horizontal);
    }

    #[test]
    fn test_wrap_vertical() {
        let children = vec![Widget::from(())];
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
            .children(vec![Widget::from(()), Widget::from(())])
            .build();

        // build() returns Widget, so we can't easily test the inner Wrap
        // Just verify it compiles and returns Widget
    }

    #[test]
    fn test_wrap_default() {
        let wrap = Wrap::default();
        assert_eq!(wrap.children.len(), 0);
        assert_eq!(wrap.direction, Axis::Horizontal);
    }
}
