//! The desktop runner's use of
//! [`PlacementRecorder`](crate::app::placement_recorder): restoring a window's
//! placement before it opens, recording it while the window lives, and writing
//! the last one when it closes or the session ends.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use flui_foundation::PresentationAddress;
use flui_platform::{DisplaySnapshot, Storage, StorageName, WindowOptions, WindowPlacement};

use super::host::{APP_RUNTIME, runtime_storage, with_owner_platform};
use crate::app::AppConfig;
use crate::app::placement_recorder::{
    FLUSH_BUDGET, PlacementRecorder, recall, restore_into, settle,
};

/// What a window that remembers its placement opens with: where the
/// placement is kept, and the one read there before the window opens.
///
/// Storage is resolved and read once for a window, here, and the value
/// travels with the window to its recorder, which so starts from the
/// placement the window really opened from.
pub(super) struct RememberedPlacement {
    storage: Arc<dyn Storage>,
    name: StorageName,
    /// `None` on a first run, and when the stored value could not be used.
    recalled: Option<WindowPlacement>,
}

impl RememberedPlacement {
    /// What `config`'s window remembers, read from the host's storage now;
    /// `None` when the configuration asks for no placement or the host has
    /// no storage to keep one in.
    pub(super) fn recall(config: &AppConfig) -> Option<Self> {
        let name = config.placement?;
        let Some(storage) = runtime_storage() else {
            tracing::warn!(
                storage = name.as_str(),
                "a window asks to remember its placement but the host has no storage; \
                 give the application an identifier or set `AppConfig::storage_dir`"
            );
            return None;
        };
        let recalled = recall(&*storage, &name);
        Some(Self {
            storage,
            name,
            recalled,
        })
    }

    /// Open `options` where the window was last time, on `owner`.
    ///
    /// The fit uses what the platform says of its windows before one exists:
    /// the capabilities (a platform that cannot place a window at open keeps
    /// size and mode only) and the frame a decorated window adds to its
    /// content size.
    pub(super) fn restore_into(
        &self,
        options: &mut WindowOptions,
        owner: &flui_native::OwnerPlatform,
    ) {
        let Some(placement) = &self.recalled else {
            return;
        };
        let Ok(displays) = owner.display_snapshot() else {
            tracing::debug!(
                storage = self.name.as_str(),
                "no displays to fit the stored window placement to"
            );
            return;
        };
        restore_into(
            options,
            placement,
            &displays,
            &owner.window_capabilities(),
            |scale| owner.window_frame_inset(scale),
        );
    }

    /// Start recording the placement of the window presented at `address`,
    /// which opened with this.
    pub(super) fn keep(self, address: PresentationAddress) {
        let Self {
            storage,
            name,
            recalled,
        } = self;
        let Some(handle) =
            APP_RUNTIME.with(|slot| slot.borrow().window_commands.handle_for(address))
        else {
            return;
        };
        // Storage answers on its own thread: wake the owner, which takes the answer
        // at its next turn.
        let Some(proxy) = with_owner_platform(flui_native::OwnerPlatform::proxy) else {
            return;
        };
        let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = proxy.wake();
        });
        let recorder = Rc::new(PlacementRecorder::new(
            storage,
            name,
            recalled.as_ref(),
            wake_owner,
        ));
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

/// The session is ending: write every window's placement now, by
/// `deadline`, the session end's own (ADR-0260), so window metadata never
/// spends the documents' time.
pub(super) fn flush_all_by(deadline: Instant) {
    let recorders = recorders();
    let now = Instant::now();
    settle(&recorders, now, deadline.saturating_duration_since(now));
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
        slot.try_borrow().map_or_default(|state| {
            state
                .placement_recorders
                .iter()
                .map(|(_, recorder)| Rc::clone(recorder))
                .collect()
        })
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

#[cfg(all(test, feature = "testing"))]
#[path = "placement_tests.rs"]
mod tests;
