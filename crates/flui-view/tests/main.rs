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
#[path = "boxed_view_conditional_return.rs"]
mod boxed_view_conditional_return;
#[path = "build_context_tests.rs"]
mod build_context_tests;
#[path = "build_owner_tests.rs"]
mod build_owner_tests;
#[path = "derive_bon_stack.rs"]
mod derive_bon_stack;
#[path = "derive_smoke.rs"]
mod derive_smoke;
#[path = "dispatch_shim.rs"]
mod dispatch_shim;
#[path = "element_kind_non_exhaustive_smoke.rs"]
mod element_kind_non_exhaustive_smoke;
#[path = "element_slot_integration.rs"]
mod element_slot_integration;
#[path = "element_tree_tests.rs"]
mod element_tree_tests;
#[path = "flutter_parity_key_equality.rs"]
mod flutter_parity_key_equality;
#[path = "global_key.rs"]
mod global_key;
#[path = "global_key_reparent.rs"]
mod global_key_reparent;
#[path = "greeting_widget_loc_golden.rs"]
mod greeting_widget_loc_golden;
#[path = "inherited_dependency.rs"]
mod inherited_dependency;
#[path = "key_roundtrip.rs"]
mod key_roundtrip;
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "notifications.rs"]
mod notifications;
#[path = "production_reconcile_emits.rs"]
mod production_reconcile_emits;
#[path = "stateless_stateful_tests.rs"]
mod stateless_stateful_tests;
#[path = "trybuild_ui.rs"]
mod trybuild_ui;
#[path = "view_element_conversion_tests.rs"]
mod view_element_conversion_tests;
#[path = "view_reconcile_match.rs"]
mod view_reconcile_match;
