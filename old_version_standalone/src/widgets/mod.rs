//! Widget system for nebula-ui.
//!
//! This module provides a complete set of building blocks for constructing UIs,
//! inspired by Flutter's widget system but adapted for egui's immediate mode paradigm.
//!
//! # Widget Categories
//!
//! - **Primitives**: Basic building blocks (Container, Text, Image, Spacer)
//! - **Layout**: Layout widgets (Row, Column, Stack, Padding, Align)
//! - **Input**: User interaction (GestureDetector, MouseRegion, Draggable)
//! - **Forms**: Form inputs (TextField, Checkbox, Slider, Dropdown)
//! - **Scrolling**: Scrollable containers (ScrollView, ListView, GridView)
//! - **Animation**: Animated widgets (AnimatedContainer, Transitions)
//! - **Platform**: Platform-specific (SafeArea, MediaQuery, Builders)
//!
//! Note: Painting modules (DecorationPainter, TransformPainter, etc.) have been moved to `crate::painters`.
//!
//! # Example
//!
//! ```rust,no_run
//! use nebula_ui::widgets::primitives::Container;
//! use nebula_ui::types::styling::BoxDecoration;
//! use nebula_ui::types::core::Color;
//!
//! // Create a styled container
//! let container = Container::new()
//!     .with_decoration(BoxDecoration::new()
//!         .with_color(Color::WHITE)
//!         .with_border_radius(12.0.into()))
//!     .with_padding(16.0);
//! ```

pub mod base;
pub mod framework;
pub mod input;
pub mod layout;
pub mod primitives;
pub mod widget;
pub mod widget_trait;




// Re-export base widget types
pub use base::{
    NebulaWidget, StatelessWidget, StatefulWidget, RenderObjectWidget,
    SingleChildWidget, MultiChildWidget, WidgetBuilder,
    WidgetKey, WidgetDiagnostics, DiagnosticProperty, RenderConstraints,
};

// Re-export core widget trait
pub use widget::{
    Widget,
    IntoWidget,
    RenderObjectWidget as RenderObjectWidgetTrait,
    LeafRenderObjectWidget,
    SingleChildRenderObjectWidget,
    MultiChildRenderObjectWidget,
};

// Re-export framework types
pub use framework::{
    Element, ElementId, ElementTree, BuildContext,
    can_update_widget,
    // Widget traits
    StatelessWidget as StatelessWidgetTrait,
    StatefulWidget as StatefulWidgetTrait,
    State,
    // Element implementations
    ComponentElement,
    StatefulElement,
    SingleChildElement,
    MultiChildElement,
};

// Re-export WidgetExt trait and helpers
pub use widget_trait::WidgetExt;
#[cfg(debug_assertions)]
pub use widget_trait::WithDebug;


// pub mod forms;
// pub mod scrolling;
// pub mod animation;
// pub mod platform;






