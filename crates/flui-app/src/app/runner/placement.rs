//! The desktop runner's use of [`PlacementKeeper`](crate::app::placement_keeper):
//! restoring a window's placement before it opens, keeping it while the window
//! lives, and writing the last one when it closes or the session ends.

use std::rc::Rc;
use std::time::Instant;

use flui_foundation::PresentationAddress;
use flui_platform::WindowOptions;
use flui_platform_api::DisplaySnapshot;

use super::host::APP_RUNTIME;
use super::host::{runtime_storage, with_owner_platform};
use crate::app::AppConfig;
use crate::app::placement_keeper::{PlacementKeeper, recall, restore_into};

/// Open `options` where `config`'s window was last time, when the
/// configuration asks to remember placement and the host has storage for it.
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
    let displays = with_owner_platform(|owner| owner.display_snapshot().ok())
        .flatten()
        .unwrap_or_else(DisplaySnapshot::empty);
    restore_into(options, &placement, &displays);
}

/// Start remembering the placement of the window presented at `address`, if its
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
    let keeper = Rc::new(PlacementKeeper::new(
        storage,
        name,
        recalled.as_ref(),
        Instant::now,
        current_displays,
    ));
    let weak = Rc::downgrade(&keeper);
    let Ok((state, subscription)) = handle.subscribe(move |state| {
        if let Some(keeper) = weak.upgrade() {
            keeper.observe(state);
        }
    }) else {
        return;
    };
    keeper.hold(subscription);
    keeper.observe(&state);
    APP_RUNTIME.with(|slot| slot.borrow_mut().placement_keepers.push((address, keeper)));
}

/// The window at `address` is closing: write its last placement and forget it.
pub(super) fn finish(address: PresentationAddress) {
    let keeper = APP_RUNTIME.with(|slot| {
        let mut state = slot.try_borrow_mut().ok()?;
        let at = state
            .placement_keepers
            .iter()
            .position(|(kept, _)| *kept == address)?;
        Some(state.placement_keepers.remove(at).1)
    });
    if let Some(keeper) = keeper {
        keeper.flush();
    }
}

/// The session is ending: write every window's placement now.
pub(super) fn flush_all() {
    let keepers: Vec<_> = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .map(|state| {
                state
                    .placement_keepers
                    .iter()
                    .map(|(_, keeper)| Rc::clone(keeper))
                    .collect()
            })
            .unwrap_or_default()
    });
    for keeper in keepers {
        keeper.flush();
    }
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
