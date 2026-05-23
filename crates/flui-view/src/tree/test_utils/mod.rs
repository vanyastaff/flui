//! In-tree test utilities for the reconciler.
//!
//! Plan §U14 / FR-035. Public only under `cfg(any(test, feature =
//! "test-utils"))` so downstream test crates (notably §U18's
//! 6-permutation corpus and §U19's GlobalKey reparenting tests)
//! can re-use the [`ReconcileEventCollector`] without duplicating
//! the `Visit` machinery, while production builds keep the module
//! gone.

pub mod reconcile_event_collector;

pub use reconcile_event_collector::{CollectedEvent, ReconcileEventCollector};
