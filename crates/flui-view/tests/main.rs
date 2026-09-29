//! Single-binary consolidation of flui-view's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `view_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`), so file-relative
//! paths (`include_str!("fixtures/greeting.rs")`) and manifest-relative
//! paths (trybuild's `tests/ui/`) keep working unchanged.
//!
//! Convention: tests that WRITE process-global state (e.g. the
//! error-view builder) live in their own [[test]] target instead —
//! process isolation beats opt-in locking. See error_view_recovery.

#[path = "ancestor_finders.rs"]
mod ancestor_finders;
#[path = "build_owner_tests.rs"]
mod build_owner_tests;
#[path = "dense_reconcile_containment.rs"]
mod dense_reconcile_containment;
#[path = "dense_update_containment.rs"]
mod dense_update_containment;
#[path = "global_key.rs"]
mod global_key;
#[path = "global_key_duplication.rs"]
mod global_key_duplication;
#[path = "global_key_reparent.rs"]
mod global_key_reparent;
#[path = "inherited_dependency.rs"]
mod inherited_dependency;
#[path = "lifecycle_panic_containment.rs"]
mod lifecycle_panic_containment;
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "notifications.rs"]
mod notifications;
#[path = "orphaned_render_mount.rs"]
mod orphaned_render_mount;
#[path = "production_reconcile_emits.rs"]
mod production_reconcile_emits;
#[path = "reconcile_capture.rs"]
mod reconcile_capture;
#[path = "recovered_panics.rs"]
mod recovered_panics;
#[path = "signal_reads.rs"]
mod signal_reads;
#[path = "stateless_stateful_tests.rs"]
mod stateless_stateful_tests;
#[path = "trybuild_ui.rs"]
mod trybuild_ui;
#[path = "writer_source.rs"]
mod writer_source;
