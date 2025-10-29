//! # FLUI Derive Macros
//!
//! Derive macros for FLUI widgets that automatically implement required traits.
//!
//! ## Available Derives
//!
//! - `#[derive(StatelessWidget)]` - For widgets without state
//! - `#[derive(StatefulWidget)]` - For widgets with mutable state
//! - `#[derive(InheritedWidget)]` - For widgets that provide inherited data
//! - `#[derive(RenderObjectWidget)]` - For widgets that create RenderObjects
//!
//! ## Example: StatelessWidget
//!
//! ```rust,ignore
//! use flui_derive::StatelessWidget;
//!
//! #[derive(StatelessWidget, Clone)]
//! struct Text {
//!     text: String,
//! }
//!
//! impl StatelessWidget for Text {
//!     fn build(&self, cx: &BuildContext) -> BoxedWidget {
//!         // ...
//!     }
//! }
//! // ✅ Widget and DynWidget auto-implemented!
//! ```

use proc_macro::TokenStream;

mod inherited;
mod render_object;
mod stateful;
mod stateless;
mod utils;
mod widget_attr;

/// Derive StatelessWidget
///
/// Auto-implements: `Widget`, `DynWidget`
#[proc_macro_derive(StatelessWidget)]
pub fn derive_stateless_widget(input: TokenStream) -> TokenStream {
    stateless::derive(input)
}

/// Derive StatefulWidget
///
/// Auto-implements: `Widget`, `DynWidget`
#[proc_macro_derive(StatefulWidget)]
pub fn derive_stateful_widget(input: TokenStream) -> TokenStream {
    stateful::derive(input)
}

/// Derive InheritedWidget
///
/// Auto-implements: `Widget`, `DynWidget`
#[proc_macro_derive(InheritedWidget)]
pub fn derive_inherited_widget(input: TokenStream) -> TokenStream {
    inherited::derive(input)
}

/// Derive RenderObjectWidget
///
/// Auto-implements: `Widget`, `DynWidget`
#[proc_macro_derive(RenderObjectWidget, attributes(render_object))]
pub fn derive_render_object_widget(input: TokenStream) -> TokenStream {
    render_object::derive(input)
}

/// Derive State
#[proc_macro_derive(State)]
pub fn derive_state(input: TokenStream) -> TokenStream {
    stateful::derive_state(input)
}

/// Widget attribute macro
///
/// Automatically adds Debug, Clone derives and generates Widget/DynWidget implementations.
///
/// # Usage
///
/// ```rust,ignore
/// use flui_core::{StatelessWidget, BoxedWidget};
///
/// #[widget]  // or #[widget(stateless)]
/// struct MyWidget {
///     value: i32,
/// }
///
/// impl StatelessWidget for MyWidget {
///     fn build(&self) -> BoxedWidget {
///         // ...
///     }
/// }
/// ```
///
/// Available types:
/// - `#[widget]` or `#[widget(stateless)]` - StatelessWidget
/// - `#[widget(stateful)]` - StatefulWidget
/// - `#[widget(inherited)]` - InheritedWidget
/// - `#[widget(render_object)]` - RenderObjectWidget
#[proc_macro_attribute]
pub fn widget(args: TokenStream, input: TokenStream) -> TokenStream {
    widget_attr::widget_attribute(args, input)
}
