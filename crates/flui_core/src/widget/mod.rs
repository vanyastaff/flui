//! Widget system - immutable configuration for UI elements
//!
//! This module provides the widget layer of FLUI's three-tree architecture:
//! - **Widget** → Immutable configuration (enum-based)
//! - **Element** → Mutable state holder (persists across rebuilds)
//! - **Render** → Layout and painting (optional, for render widgets)
//!
//! # Widget Types
//!
//! 1. **StatelessWidget** - Pure function from config to UI
//! 2. **StatefulWidget** - Creates a State object that persists
//! 3. **InheritedWidget** - Propagates data down the tree
//! 4. **RenderWidget** - Direct control over layout/paint
//! 5. **ParentDataWidget** - Attaches metadata to descendants
//!
//! # Architecture (Enum-Based)
//!
//! ```text
//! Widget (enum)
//!   ├─ Stateless(Box<dyn StatelessWidget>)
//!   ├─ Stateful(Box<dyn StatefulWidget>)
//!   ├─ Inherited(Box<dyn InheritedWidget>)
//!   ├─ Render(Box<dyn RenderWidget>)
//!   └─ ParentData(Box<dyn ParentDataWidget>)
//!
//! Benefits:
//! ✅ No trait coherence conflicts
//! ✅ Exhaustive pattern matching
//! ✅ Clear semantic variants
//! ✅ Consistent with Element enum
//! ```
//!
//! # Examples
//!
//! ## StatelessWidget
//!
//! ```rust,ignore
//! use flui_core::{Widget, StatelessWidget, BuildContext};
//!
//! #[derive(Debug, Clone)]
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessWidget for Greeting {
//!     fn build(&self, ctx: &BuildContext) -> Widget {
//!         Widget::render_object(Text::new(format!("Hello, {}!", self.name)))
//!     }
//!
//!     fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
//!         Box::new(self.clone())
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//!
//! // Create widget
//! let widget = Widget::stateless(Greeting {
//!     name: "World".into()
//! });
//! ```
//!
//! ## Pattern Matching
//!
//! ```rust,ignore
//! match widget {
//!     Widget::Stateless(w) => w.build(ctx),
//!     Widget::Stateful(w) => {
//!         let state = w.create_state();
//!         state.build(ctx)
//!     }
//!     Widget::Render(w) => {
//!         w.create_render_object(ctx)
//!     }
//!     _ => {}
//! }
//! ```

// Submodules
pub mod notification_listener;
pub mod traits;
pub mod widget;

// Re-exports - Enum-based system
pub use notification_listener::NotificationListener;
pub use traits::{
    InheritedWidget, ParentDataWidget, RenderWidget, State, StatefulWidget, StatelessWidget,
};
pub use widget::{IntoWidget, Widget};

// Widget is an enum - no DynWidget trait needed!
