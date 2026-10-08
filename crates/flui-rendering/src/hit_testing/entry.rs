//! Canonical hit-test entry re-exported from `flui_interaction::routing`.
//!
//! Entries carry render identities, transforms, cursor contributions and
//! data-only owner-lane targets. Executable handlers remain in the
//! presentation's interaction lane, outside render storage.
//! [`PipelineOwner::hit_test`](crate::pipeline::PipelineOwner::hit_test)
//! produces the hit path through the render protocols.

pub use flui_interaction::routing::HitTestEntry;
