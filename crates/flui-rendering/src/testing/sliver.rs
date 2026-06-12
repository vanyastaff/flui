//! Ergonomic [`SliverConstraints`] construction for tests.
//!
//! [`SliverConstraints`] is a wide plain-data struct; building one by hand
//! is noisy and was duplicated as `vertical_constraints` /
//! `horizontal_constraints` helpers across the sliver tests. This module
//! offers a small builder that starts from a sensible viewport and lets a
//! test override only the fields it cares about.
//!
//! The axis pair is derived for you: [`vertical`] scrolls top-to-bottom
//! with a left-to-right cross axis, [`horizontal`] scrolls left-to-right
//! with a top-to-bottom cross axis.

use flui_types::layout::AxisDirection;

use crate::{constraints::SliverConstraints, view::ScrollDirection};

/// A chainable builder for [`SliverConstraints`].
///
/// Construct one with [`vertical`] or [`horizontal`], override extents with
/// the setters, then call [`build`](SliverConstraintsBuilder::build).
#[derive(Debug, Clone, Copy)]
pub struct SliverConstraintsBuilder {
    inner: SliverConstraints,
}

impl SliverConstraintsBuilder {
    /// Sets the scroll offset.
    #[must_use]
    pub fn scroll_offset(mut self, value: f32) -> Self {
        self.inner.scroll_offset = value;
        self
    }

    /// Sets the remaining paint extent (viewport space available to paint).
    #[must_use]
    pub fn remaining_paint_extent(mut self, value: f32) -> Self {
        self.inner.remaining_paint_extent = value;
        self
    }

    /// Sets the cross-axis extent.
    #[must_use]
    pub fn cross_axis_extent(mut self, value: f32) -> Self {
        self.inner.cross_axis_extent = value;
        self
    }

    /// Sets the total viewport main-axis extent.
    #[must_use]
    pub fn viewport_main_axis_extent(mut self, value: f32) -> Self {
        self.inner.viewport_main_axis_extent = value;
        self
    }

    /// Sets the remaining cache extent.
    #[must_use]
    pub fn remaining_cache_extent(mut self, value: f32) -> Self {
        self.inner.remaining_cache_extent = value;
        self
    }

    /// Sets the cache origin (typically negative).
    #[must_use]
    pub fn cache_origin(mut self, value: f32) -> Self {
        self.inner.cache_origin = value;
        self
    }

    /// Sets the overlap with the preceding sliver.
    #[must_use]
    pub fn overlap(mut self, value: f32) -> Self {
        self.inner.overlap = value;
        self
    }

    /// Sets the scroll extent already consumed by preceding slivers.
    #[must_use]
    pub fn preceding_scroll_extent(mut self, value: f32) -> Self {
        self.inner.preceding_scroll_extent = value;
        self
    }

    /// Sets the user scroll direction.
    #[must_use]
    pub fn user_scroll_direction(mut self, value: ScrollDirection) -> Self {
        self.inner.user_scroll_direction = value;
        self
    }

    /// Finalizes the builder into [`SliverConstraints`].
    #[must_use]
    pub fn build(self) -> SliverConstraints {
        self.inner
    }
}

impl From<SliverConstraintsBuilder> for SliverConstraints {
    fn from(builder: SliverConstraintsBuilder) -> Self {
        builder.build()
    }
}

/// A vertically-scrolling sliver constraint builder (main axis
/// top-to-bottom, cross axis left-to-right), starting from
/// [`SliverConstraints::default`].
#[must_use]
pub fn vertical() -> SliverConstraintsBuilder {
    SliverConstraintsBuilder {
        inner: SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            cross_axis_direction: AxisDirection::LeftToRight,
            ..SliverConstraints::default()
        },
    }
}

/// A horizontally-scrolling sliver constraint builder (main axis
/// left-to-right, cross axis top-to-bottom), starting from
/// [`SliverConstraints::default`].
#[must_use]
pub fn horizontal() -> SliverConstraintsBuilder {
    SliverConstraintsBuilder {
        inner: SliverConstraints {
            axis_direction: AxisDirection::LeftToRight,
            cross_axis_direction: AxisDirection::TopToBottom,
            ..SliverConstraints::default()
        },
    }
}
