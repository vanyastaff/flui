//! The frame runtime of FLUI (ADR-0083 §1).
//!
//! This crate is where a realm and its per-presentation frame machinery live,
//! below the hosts that drive it: the runners, the platform wiring and the
//! raster lane stay in `flui-app`, which is its only normal dependent. It is
//! internal: nothing here is an embedder API (ADR-0027 §9) except the
//! host-injection seam in [`execution`] and the frame-failure vocabulary in
//! [`frame_failure`], which `flui-app` and the `flui` facade re-export and
//! which therefore carry the Stable promise (ADR-0089 §1). It names no
//! platform backend, windowing, GPU or engine type: a window is a
//! `flui_platform_api::PlatformWindow`, and a frame leaves through a
//! [`FrameSink`](sink::FrameSink) the host implements.
//!
//! - [`ui_realm`]: `UiRealm`, the owner-affine realm — the presentations it
//!   hosts, their frame transaction, input routing, lifecycle and command
//!   inbox;
//! - [`presentation`]: one presentation's owner-thread state (a realm keeps
//!   its presentations in a private, mount-ordered forest);
//! - [`lifecycle_state`]: the per-presentation application lifecycle
//!   derivation;
//! - [`frame_failure`]: what a contained frame failure reports and how the
//!   application's handler disposes of it;
//! - [`media_query_root`]: the `MediaQuery` a realm installs above each root
//!   widget;
//! - [`renderer_binding`]: the per-presentation rendering binding over a
//!   pipeline owner;
//! - [`epoch`]: the tree revision a presentation's frames advance and whether
//!   the current one has been acknowledged by a submit;
//! - [`held_input`]: the bounded pointer input a presentation retains while it
//!   has no committed tree, and its replay;
//! - [`execution`]: the loop-scoped background execution services (ADR-0047)
//!   — the compute and IO lanes, their bounded admission and shutdown, and
//!   the host-injection seam — which only the host constructs;
//! - [`performance_stats`]: the rolling frame-time window a presentation's
//!   performance overlay draws;
//! - `reload` (with the `hot-reload` feature): the development reload tier a
//!   realm applies, translated from the host's hot-reload driver;
//! - [`semantics_host`]: per-presentation semantics enablement and platform
//!   accessibility delivery;
//! - [`sink`]: the [`FrameSink`](sink::FrameSink) a frame is submitted
//!   through, and the [`SubmitVerdict`](sink::SubmitVerdict) the realm
//!   classifies;
//! - `testing` (with the `test-support` feature): a window double and a
//!   scripted sink for driving a realm headlessly.

pub mod epoch;
pub mod execution;
pub mod frame_failure;
pub mod held_input;
pub mod lifecycle_state;
pub mod media_query_root;
pub mod performance_stats;
pub mod presentation;
mod presentation_forest;
mod realm_services;
#[cfg(feature = "hot-reload")]
pub mod reload;
pub mod renderer_binding;
pub mod semantics_host;
pub mod sink;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod ui_realm;
