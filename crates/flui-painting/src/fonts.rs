//! The font faces this crate embeds, exposed as bytes.
//!
//! `flui-painting` owns the process-wide `FontSystem` (ADR-0016), so it is also
//! the crate that guarantees that system is *usable*: a host with no fonts
//! installed — a container, CI, and notably iOS, which ships no system fonts a
//! plain `FontSystem::new()` can see — would otherwise hand the shaper an empty
//! database, and the first shaped run panics inside cosmic-text with
//! `no default font found`.
//!
//! `font_system_arc()` installs the face below when host discovery finds no
//! Latin-capable face, so every image that links this crate — the host binary
//! and a hot-reload worker `cdylib` alike — gets a working shaper without
//! depending on `flui-engine`'s renderer to prime it. That last point is why
//! the asset lives here and not in the engine: a worker statically links its
//! own `flui-painting`, never `flui-engine`, so a fallback installed only by
//! the engine never reaches the worker's `FontSystem`.

/// The embedded text fallback face (Roboto Regular).
///
/// Apache-2.0; see the workspace licence inventory.
pub const ROBOTO_REGULAR: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
