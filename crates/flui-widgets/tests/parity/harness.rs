//! Parity-harness shim — additive test-only primitives layered on [`LaidOut`].
//!
//! Three free functions extend the existing headless layout harness for
//! Flutter-parity tests:
//!
//! - [`screen()`] — the 800 × 600 tight constraints Flutter uses as its default
//!   test surface (matching `TestView.physicalSize`).
//! - [`screen_of(w, h)`] — parameterised tight surface.
//! - [`pump_widget(root, constraints)`] — initial mount, identical to
//!   [`common::lay_out`] but named after Flutter's `tester.pumpWidget` for
//!   muscle-memory alignment.
//!
//! Root-swap / subsequent `pumpWidget` calls are [`LaidOut::pump_widget`]
//! (a method, not this free function) — the distinction mirrors Flutter: the
//! first `pumpWidget` mounts; later ones swap.
//!
//! `find_text` is provided here rather than on `LaidOut` so the parity tests
//! can import it as a harness helper even though its implementation delegates
//! to `LaidOut::find_text`.

use flui_rendering::constraints::BoxConstraints;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::View;

use crate::common::{LaidOut, lay_out};

/// 800 × 600 tight constraints — Flutter's default test-surface size.
///
/// Flutter reference: `TestView.physicalSize` defaults to `Size(800.0, 600.0)`
/// (logical pixels at device-pixel ratio 1.0).
pub fn screen() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(800.0), px(600.0)))
}

/// Tight constraints for an arbitrary surface of `width × height` logical pixels.
pub fn screen_of(width: f32, height: f32) -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(width), px(height)))
}

/// Mount `root` under `constraints` and return the laid-out tree — Flutter's
/// `tester.pumpWidget(root)` (first / initial call).
///
/// For subsequent root-swaps use [`LaidOut::pump_widget`].
pub fn pump_widget(root: impl View, constraints: BoxConstraints) -> LaidOut {
    lay_out(root, constraints)
}
