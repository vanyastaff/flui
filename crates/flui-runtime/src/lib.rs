//! The frame runtime of FLUI (ADR-0083 §1).
//!
//! This crate is where a realm's per-presentation frame machinery lives, below
//! the hosts that drive it: the runners, the platform wiring and the raster
//! lane stay in `flui-app`, which is its only normal dependent. It is
//! internal: nothing here is an embedder API (ADR-0027 §9) except the
//! host-injection seam in [`execution`], which `flui-app` and the `flui`
//! facade re-export and which therefore carries the Stable promise
//! (ADR-0089 §1). It names no platform backend, windowing, GPU or engine type.
//!
//! Today it holds the presentation lanes that need nothing from the realm
//! core, the seam a frame leaves through, and the host loop's background
//! execution services:
//!
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
//!   classifies.

pub mod epoch;
pub mod execution;
pub mod held_input;
pub mod performance_stats;
#[cfg(feature = "hot-reload")]
pub mod reload;
pub mod semantics_host;
pub mod sink;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
