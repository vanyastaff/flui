//! The frame runtime of FLUI (ADR-0083 §1).
//!
//! This crate is where a realm's per-presentation frame machinery lives, below
//! the hosts that drive it: the runners, the platform wiring and the raster
//! lane stay in `flui-app`, which is its only normal dependent. It is
//! internal: nothing here is an embedder API (ADR-0027 §9), and it names no
//! platform backend, windowing, GPU or engine type.
//!
//! Today it holds the presentation lanes that need nothing from the realm
//! core:
//!
//! - [`epoch`]: the tree revision a presentation's frames advance and whether
//!   the current one has been acknowledged by a submit;
//! - [`held_input`]: the bounded pointer input a presentation retains while it
//!   has no committed tree, and its replay;
//! - [`semantics_host`]: per-presentation semantics enablement and platform
//!   accessibility delivery.

pub mod epoch;
pub mod held_input;
pub mod semantics_host;
