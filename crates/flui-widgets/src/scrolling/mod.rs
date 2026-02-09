//! Scrolling widgets
//!
//! This module provides sliver-based scrolling widgets following Flutter's architecture.
//!
//! # Architecture
//!
//! The scrolling system consists of several layers:
//!
//! ```text
//! CustomScrollView (convenience)
//!   ├── Scrollable (gesture/physics)
//!   └── Viewport (visual/layout)
//!       └── RenderViewport (render layer)
//!           ├── SliverList
//!           ├── SliverGrid
//!           └── SliverAppBar
//! ```
//!
//! # Components
//!
//! - **Scrollable**: Handles gestures, physics, and scroll position
//! - **Viewport**: Visual container for slivers with layout coordination
//! - **CustomScrollView**: High-level API combining Scrollable + Viewport
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_widgets::scrolling::CustomScrollView;
//!
//! CustomScrollView::new()
//!     .slivers(vec![
//!         Box::new(SliverAppBar::new()),
//!         Box::new(SliverList::new()),
//!     ])
//! ```

// TODO: Re-enable after sliver migration is complete
// pub mod scrollable;
// pub mod viewport;
// pub mod custom_scroll_view;

// pub use scrollable::Scrollable;
// pub use viewport::Viewport;
// pub use custom_scroll_view::CustomScrollView;
