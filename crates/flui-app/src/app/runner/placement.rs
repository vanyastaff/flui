//! The desktop runner's use of
//! [`PlacementRecorder`](crate::app::placement_recorder): restoring a window's
//! placement before it opens, recording it while the window lives, and writing
//! the last one when it closes or the session ends.

use std::rc::Rc;
use std::time::Instant;

use flui_foundation::PresentationAddress;
use flui_platform::WindowOptions;
use flui_platform_api::DisplaySnapshot;

use super::host::{APP_RUNTIME, runtime_storage, with_owner_platform};
use crate::app::AppConfig;
use crate::app::placement_recorder::{
    FLUSH_BUDGET, PlacementRecorder, recall, restore_into, settle,
};

/// Open `options` where `config`'s window was last time, when the
/// configuration asks to remember placement and the host has storage for it.
///
/// The fit uses what the platform says of its windows before one exists: the
/// capabilities (a platform that cannot place a window at open keeps size and
/// mode only) and the frame a decorated window adds to its content size.
pub(super) fn restore(options: &mut WindowOptions, config: &AppConfig) {
    let Some(name) = config.placement else {
        return;
    };
    let Some(storage) = runtime_storage() else {
        tracing::warn!(
            storage = name.as_str(),
            "a window asks to remember its placement but the host has no storage; \
             set `with_storage_dir`"
        );
        return;
    };
    let Some(placement) = recall(&*storage, &name) else {
        return;
    };
    let restored = with_owner_platform(|owner| {
        let displays = owner.display_snapshot().ok()?;
        restore_into(
            options,
            &placement,
            &displays,
            &owner.window_capabilities(),
            |scale| owner.window_frame_inset(scale),
        );
        Some(())
    });
    if restored.flatten().is_none() {
        tracing::debug!(
            storage = name.as_str(),
            "no platform or displays to fit the stored window placement to"
        );
    }
}

/// Start recording the placement of the window presented at `address`, if its
/// configuration asks for it.
pub(super) fn keep(config: &AppConfig, address: PresentationAddress) {
    let Some(name) = config.placement else {
        return;
    };
    let Some(storage) = runtime_storage() else {
        return;
    };
    let Some(handle) = APP_RUNTIME.with(|slot| slot.borrow().window_commands.handle_for(address))
    else {
        return;
    };
    let recalled = recall(&*storage, &name);
    let recorder = Rc::new(PlacementRecorder::new(storage, name, recalled.as_ref()));
    let weak = Rc::downgrade(&recorder);
    let Ok((state, subscription)) = handle.subscribe(move |state| {
        if let Some(recorder) = weak.upgrade() {
            recorder.observe(state, &current_displays(), Instant::now());
        }
    }) else {
        return;
    };
    recorder.hold(subscription);
    recorder.observe(&state, &current_displays(), Instant::now());
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .placement_recorders
            .push((address, recorder));
    });
}

/// The window at `address` is closing: write its last placement and forget it.
pub(super) fn finish(address: PresentationAddress) {
    let recorder = APP_RUNTIME.with(|slot| {
        let mut state = slot.try_borrow_mut().ok()?;
        let at = state
            .placement_recorders
            .iter()
            .position(|(kept, _)| *kept == address)?;
        Some(state.placement_recorders.remove(at).1)
    });
    if let Some(recorder) = recorder {
        settle(&[recorder], Instant::now(), FLUSH_BUDGET);
    }
}

/// The session is ending: write every window's placement now, within one
/// budget for all of them.
pub(super) fn flush_all() {
    let recorders = recorders();
    settle(&recorders, Instant::now(), FLUSH_BUDGET);
}

/// Write the placements whose throttle interval is over. The owner calls it
/// after every turn, so a wake for a held write does it whatever the frames
/// are doing.
pub(super) fn tick() {
    let now = Instant::now();
    for recorder in recorders() {
        recorder.tick(now);
    }
}

fn recorders() -> Vec<Rc<PlacementRecorder>> {
    APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .map(|state| {
                state
                    .placement_recorders
                    .iter()
                    .map(|(_, recorder)| Rc::clone(recorder))
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The displays the host last sampled; none while it has not sampled yet.
fn current_displays() -> DisplaySnapshot {
    APP_RUNTIME
        .with(|slot| {
            slot.try_borrow()
                .ok()
                .and_then(|state| state.installed_host.displays().snapshot())
        })
        .unwrap_or_else(DisplaySnapshot::empty)
}
