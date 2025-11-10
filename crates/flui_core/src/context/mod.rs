//! Context system
//!
//! The context system provides a way to pass data through the element tree
//! without having to pass props down manually at every level.
//!
//! # Overview
//!
//! Context is useful for sharing data that can be considered "global" for
//! a tree of components, such as theme, locale, or user preferences.
//!
//! # Key Components
//!
//! - `Context<T>`: Typed context value
//! - `Provider`: Provides context to descendants
//! - Consumer API: Read context from BuildContext
//!
//! # Example
//!
//! ```rust,ignore
//! // Provide context
//! let theme_provider = Provider::new(
//!     Context::new(Theme::dark()),
//!     child_widget,
//! );
//!
//! // Consume context
//! fn build(&self, ctx: &mut BuildContext) -> View {
//!     let theme = ctx.read_context::<Theme>()?;
//!     // Use theme...
//! }
//! ```
//!
//! # Implementation vs Inheritance
//!
//! Context is orthogonal to Flutter's InheritedWidget. In Flui:
//! - Context: For application data (theme, locale, etc.)
//! - ProviderElement: For framework data (MediaQuery, Directionality)
//!
//! Both use [`ProviderElement`](crate::element::ProviderElement) but serve
//! different purposes.
//!
//! # Implementation Status
//!
//! This module is reserved for future context system implementation.
//! Context functionality is currently provided through ProviderElement
//! and dependency tracking in BuildContext.
