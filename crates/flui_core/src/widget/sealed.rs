//! Sealed trait pattern for Widget hierarchy
//!
//! This module uses the "sealed trait" pattern to control which types can implement
//! the `Widget` trait. This prevents downstream crates from implementing `Widget`
//! directly, ensuring type safety and preventing trait coherence issues.
//!
//! # The Sealed Trait Pattern
//!
//! The sealed trait pattern is used throughout the Rust standard library
//! (e.g., `Iterator`, `std::io::Seek`) to control trait implementations.
//!
//! ## How it works:
//!
//! 1. Define a `Sealed` trait in a private module
//! 2. Make public traits extend `Sealed`
//! 3. Only types in this crate can impl `Sealed`
//! 4. Therefore, only types in this crate can impl the public trait
//!
//! ## Benefits:
//!
//! - **Prevents blanket impl conflicts**: We can have multiple blanket impls without conflicts
//! - **Type safety**: Only approved types can be Widgets
//! - **Future-proof**: We can add new widget types without breaking changes
//! - **Zero-cost**: Purely compile-time, no runtime overhead
//!
//! # Example
//!
//! ```rust,ignore
//! // ✅ Works - StatelessWidget automatically gets Widget impl
//! impl StatelessWidget for MyWidget {
//!     fn build(&self) -> BoxedWidget { /* ... */ }
//! }
//!
//! // ❌ Doesn't compile - Widget is sealed!
//! impl Widget for MyCustomWidget { /* ... */ }
//! ```

/// Sealed trait that controls which types can implement `Widget`
///
/// This trait is private and cannot be implemented outside this crate.
/// It serves as a gatekeeper for the `Widget` trait.
///
/// # Implementation
///
/// Types get `Sealed` implementation automatically when they implement:
/// - `StatelessWidget` - gets `ComponentElement` as ElementType
/// - `StatefulWidget` (via `Stateful` wrapper) - gets `StatefulElement` as ElementType
/// - `InheritedWidget` - gets `InheritedElement` as ElementType
/// - `ParentDataWidget` - gets `ParentDataElement` as ElementType
/// - `ProxyWidget` - gets appropriate Element type
pub trait Sealed {
    /// The concrete Element type created by this Widget
    ///
    /// This associated type is determined automatically based on which
    /// widget trait the type implements.
    ///
    /// The ElementType must implement `DynElement + Send + Sync + 'static`.
    type ElementType: crate::element::DynElement + Send + Sync + 'static;
}
