//! Accessibility semantics widgets.
//!
//! These widgets are thin `RenderView` wrappers over the semantics proxy render
//! objects in `flui-objects`, split into `Semantics`, `MergeSemantics`, and
//! `ExcludeSemantics`.
//!
//! # Action handlers are owner-local and receive an `EventContext`
//!
//! [`Semantics`]'s `on_*` builders register handlers that assistive technology
//! can invoke, and each receives the `&mut EventContext<'_>` a write through the
//! widget's `WriterSource` opens (ADR-0086), exactly like a pointer callback:
//! "activation toggles this control's own state" is a signal write.
//!
//! The widget's closures do not go into the configuration a semantics node
//! carries. They stay in the owner's interaction lane as one table per node,
//! and the configuration advertises each action through one
//! `flui_semantics::SemanticsActionHandler` that holds only the lane's ticket.
//! A rebuild replaces the table under the same ticket and reuses the same
//! handler, so the configuration compares equal. Invoking the handler resolves
//! the ticket inside the UI runtime and runs the closure there.
//!
//! An action invoked outside its UI runtime — no interaction lane active on the
//! calling thread — has no owner to run in and is dropped with a warning. A
//! [`Semantics`] mounted without an owner lane (a detached render-object
//! context) advertises none of these actions, so no platform sees a control
//! that nothing can run. [`Semantics::from_configuration`] still takes raw
//! handlers; that is the `flui-semantics` surface, below this widget.
//! `ARCHITECTURE.md`, "`Semantics` action handlers are owner-local `EventContext`
//! callbacks", records the design.

mod actions;
mod annotation;
mod exclude;
mod indexed;
mod merge;
mod sliver;

pub use annotation::Semantics;
pub(crate) use annotation::release_part;
pub use exclude::ExcludeSemantics;
pub use indexed::IndexedSemantics;
pub use merge::MergeSemantics;
pub use sliver::SliverSemantics;
