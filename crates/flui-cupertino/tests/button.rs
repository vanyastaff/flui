//! Integration tests for [`CupertinoButton`] — tap firing, the disabled
//! swallow, the press-opacity timeline under a real vsync, and per-size
//! geometry reaching the mounted render tree.

#![allow(clippy::unwrap_used)]

mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use common::{lay_out, lay_out_animated, loose, tight};
use flui_animation::Vsync;
use flui_cupertino::{CupertinoButton, CupertinoButtonSize, CupertinoColors};
use flui_types::platform::Brightness;
use flui_widgets::SizedBox;
use flui_widgets::animated::VsyncScope;
use flui_widgets::{MediaQuery, MediaQueryData};

/// A tap on an enabled button reaches `on_pressed` — proving `GestureDetector`
/// is actually wired, not merely constructed.
#[test]
fn tap_fires_on_pressed() {
    let tapped = Rc::new(Cell::new(false));
    let tapped_for_closure = Rc::clone(&tapped);

    let laid = lay_out(
        CupertinoButton::new(SizedBox::shrink()).on_pressed(move || tapped_for_closure.set(true)),
        tight(100.0, 44.0),
    );

    laid.dispatch_pointer_down(50.0, 22.0);
    laid.dispatch_pointer_up(50.0, 22.0);

    assert!(
        tapped.get(),
        "tapping an enabled CupertinoButton should fire on_pressed"
    );
}

/// A disabled button (no `on_pressed`/`on_long_press`) swallows nothing:
/// `GestureDetector` is built with no `on_tap` closure at all (`enabled()` is
/// false — see `src/button.rs`'s `button_with_no_handlers_is_disabled` unit
/// test), so a tap simply does not recognize. There is no handler to assert
/// "did not fire" against; the observable contract at the widget-tree level
/// is that dispatching a full down+up sequence over a disabled button
/// completes without panicking (a `GestureDetector` with an absent `on_tap`
/// closure is a real, exercised code path here, not a hypothetical) —
/// mirrors `flui_material::InkWell`'s documented `enabled` contract.
#[test]
fn disabled_button_swallows_the_tap_without_panicking() {
    let laid = lay_out(CupertinoButton::new(SizedBox::shrink()), tight(100.0, 44.0));

    laid.dispatch_pointer_down(50.0, 22.0);
    laid.dispatch_pointer_up(50.0, 22.0);
}

/// Per-size geometry: with an empty child, `ConstrainedBox`'s
/// `kCupertinoButtonMinSize` floor is the only thing establishing the
/// button's size — reaches the mounted render tree, not just the pure
/// `size_min_dimension` table function.
#[test]
fn per_size_minimum_geometry_reaches_the_mounted_render_tree() {
    let small = lay_out(
        CupertinoButton::new(SizedBox::shrink())
            .size_style(CupertinoButtonSize::Small)
            .on_pressed(|| {}),
        loose(200.0),
    );
    assert_eq!(small.size(small.root()), common::size(28.0, 28.0));

    let medium = lay_out(
        CupertinoButton::new(SizedBox::shrink())
            .size_style(CupertinoButtonSize::Medium)
            .on_pressed(|| {}),
        loose(200.0),
    );
    assert_eq!(medium.size(medium.root()), common::size(32.0, 32.0));

    let large = lay_out(
        CupertinoButton::new(SizedBox::shrink()).on_pressed(|| {}),
        loose(200.0),
    );
    assert_eq!(large.size(large.root()), common::size(44.0, 44.0));
}

/// An explicit `minimum_size(0.0, 0.0)` genuinely removes the floor —
/// matching the oracle's `minimumSize?.width ?? ...` chain, where a
/// caller-supplied `Size.zero` passes straight through and is never
/// re-routed to `kCupertinoButtonMinSize`/`kMinInteractiveDimensionCupertino`.
/// With no floor and an empty child, the button's size collapses to just its
/// large-style padding (20 horizontal, 16 vertical, each doubled) — well
/// under the 44×44 the per-size default floor would otherwise force.
#[test]
fn explicit_minimum_size_zero_removes_the_floor() {
    let laid = lay_out(
        CupertinoButton::new(SizedBox::shrink())
            .minimum_size(0.0, 0.0)
            .on_pressed(|| {}),
        loose(200.0),
    );
    assert_eq!(laid.size(laid.root()), common::size(40.0, 32.0));
}

/// The press-opacity timeline under a real vsync: tapping fades the button
/// toward `pressed_opacity` over `K_FADE_OUT_DURATION` (120ms), then back to
/// full opacity over `K_FADE_IN_DURATION` (180ms) — driven through the
/// harness's virtual clock, not asserted as a pure function of time.
///
/// Ticks in small (~20ms) increments — matching real per-frame cadence —
/// rather than a few large jumps: the release fade is started reentrantly
/// from inside the press fade's own `AnimationStatus::Completed` status
/// listener (see `src/button.rs`'s `init_state`), and a single large
/// `pump_for` spanning that restart lands its very next tick at an
/// oversized elapsed delta relative to the just-started run, which this
/// harness's virtual clock cannot re-anchor mid-jump — a documented
/// precision limit of driving a reentrant restart through coarse ticks, not
/// a claim about real per-frame-driven usage (which never jumps 90ms
/// between frames).
#[test]
fn press_opacity_fades_out_then_back_in_over_the_oracle_durations() {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            CupertinoButton::new(SizedBox::new(60.0, 40.0)).on_pressed(|| {}),
        ),
        tight(60.0, 40.0),
        vsync,
    );

    let opacity_id = laid
        .find_by_render_type("RenderOpacity")
        .expect("CupertinoButton should mount a FadeTransition -> Opacity render node");

    let read_opacity = |laid: &common::LaidOut| -> f32 {
        laid.render_property(opacity_id, "opacity")
            .expect("RenderOpacity should report its opacity diagnostic")
            .parse()
            .expect("opacity diagnostic should be a plain float")
    };

    assert!(
        (read_opacity(&laid) - 1.0).abs() < 1e-3,
        "opacity should start at full (1.0) before any tap"
    );

    laid.dispatch_pointer_down(30.0, 20.0);
    laid.dispatch_pointer_up(30.0, 20.0);

    // Anchor the run (first tick after `animate_to_curved` starts it), then
    // tick in ~20ms steps through the full press (120ms) + release (180ms)
    // sequence, tracking the lowest opacity reached (the press floor) and
    // whether it later rises again (the release).
    laid.pump_for(Duration::from_millis(1));
    let mut lowest = read_opacity(&laid);
    let mut lowest_at_ms = 0u64;
    let mut elapsed_ms = 0u64;
    let mut recovered_past_lowest = false;
    for _ in 0..20 {
        laid.pump_for(Duration::from_millis(20));
        elapsed_ms += 20;
        let opacity = read_opacity(&laid);
        if opacity < lowest {
            lowest = opacity;
            lowest_at_ms = elapsed_ms;
        } else if elapsed_ms > lowest_at_ms && opacity > lowest + 0.1 {
            recovered_past_lowest = true;
        }
    }

    assert!(
        lowest < 0.55,
        "the press-out fade should reach close to the 0.4 pressed-opacity floor; \
         lowest observed was {lowest} at {lowest_at_ms}ms"
    );
    assert!(
        recovered_past_lowest,
        "after reaching its lowest ({lowest} at {lowest_at_ms}ms), opacity should rise back \
         toward full during the release fade, not stay pinned at the floor"
    );

    let settled = read_opacity(&laid);
    assert!(
        (settled - 1.0).abs() < 1e-2,
        "once both fades complete (400ms total ticked, comfortably past 120ms + 180ms), \
         opacity ({settled}) should have returned to ~1.0"
    );
}

/// `resolve_background_color`'s `Plain`/`Filled` alpha multiplier is the
/// oracle's `widget.color?.opacity` (`button.dart`, oracle tag `3.44.0`) —
/// read off the ORIGINAL, never-resolved `widget.color`, which for a
/// `CupertinoDynamicColor` is always its light/normal variant (see
/// `src/button.rs`'s `resolve_background_color` doc for the full mechanism
/// citation). `CupertinoColors.separator` is alpha 73 light / 153 dark
/// (`colors.dart`, oracle tag `3.44.0`): a `.separator`-colored `Plain`
/// button under a Dark theme resolves its RGB to the dark variant
/// (`84, 84, 88`) but keeps the LIGHT variant's alpha (`73`), matching real
/// Flutter exactly.
///
/// Red-check: change `resolve_background_color`'s `Plain`/`Filled` branch to
/// use the RESOLVED color's own alpha (`base_color.a`) instead of
/// `view.color`'s — this assertion fails with `a: 153`, not `a: 73`.
#[test]
fn background_dynamic_color_keeps_the_light_variants_alpha_under_a_dark_theme() {
    let dark = MediaQueryData {
        platform_brightness: Brightness::Dark,
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        MediaQuery::new(
            dark,
            CupertinoButton::new(SizedBox::shrink())
                .color(CupertinoColors::SEPARATOR)
                .on_pressed(|| {}),
        ),
        tight(100.0, 44.0),
    );

    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("CupertinoButton always paints a DecoratedBox background");
    let decoration = laid
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox always reports its decoration");

    assert!(
        decoration.contains("r: 84, g: 84, b: 88, a: 73"),
        "a Dynamic background under Dark theme must resolve the dark RGB but keep the \
         light-variant alpha (oracle-faithful, not a bug): {decoration}"
    );
}
