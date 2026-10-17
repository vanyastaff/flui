//! External textures a presentation draws through [`TextureLayer`]s, fed with
//! frames from any thread.
//!
//! A [`TextureRegistry`] belongs to one presentation. [`TextureRegistry::register`]
//! issues a [`TextureRegistration`]: an owned handle whose [`TextureId`] is never
//! issued again, which accepts frames until it is dropped. Each frame wakes the
//! presentation and notifies the id's listeners (the `Texture` widget repaints),
//! and the raster backend takes the newest frame per id before it renders, so a
//! producer that outruns the display only costs its skipped frames. Dropping a
//! registration retires its id; the backend releases the allocation on its next
//! update.
//!
//! [`TextureLayer`]: crate::TextureLayer

use std::{collections::HashMap, fmt, sync::Arc};

use flui_foundation::panic::PanicRecovery;
use flui_painting::paint::{Image, TextureId};
use parking_lot::Mutex;

/// A presentation's external textures. Cloning shares the registry.
#[derive(Clone)]
pub struct TextureRegistry {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct State {
    last_issued: u64,
    exhausted: bool,
    live: HashMap<TextureId, Entry>,
    retired: Vec<TextureId>,
    next_listener: u64,
}

#[derive(Default)]
struct Entry {
    pending: Option<Image>,
    revision: u64,
    listeners: Vec<(u64, Listener)>,
}

type Listener = Arc<dyn Fn() + Send + Sync>;

/// Every texture id has been issued; the registry refuses further ids rather
/// than reuse one a stale layer may still name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("texture ids are exhausted")]
pub struct TextureIdsExhausted;

/// What the raster backend applies before rendering: the newest frame of each
/// texture that received one, and the ids to release.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct TextureUpdates {
    /// The newest frame per texture since the previous update.
    pub frames: Vec<(TextureId, Image)>,
    /// Textures whose registration was dropped.
    pub retired: Vec<TextureId>,
}

impl TextureUpdates {
    /// Frames to apply with nothing retired, as a backend re-uploads the
    /// textures it holds after replacing its device.
    #[must_use]
    pub fn from_frames(frames: Vec<(TextureId, Image)>) -> Self {
        Self {
            frames,
            retired: Vec::new(),
        }
    }

    /// Whether there is nothing to apply.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty() && self.retired.is_empty()
    }
}

impl TextureRegistry {
    /// A registry whose frames and retirements call `wake` so the presentation
    /// produces a frame.
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                wake,
            }),
        }
    }

    /// Issues a texture id and the handle that feeds and owns it.
    ///
    /// # Errors
    ///
    /// [`TextureIdsExhausted`] once every id has been issued; the refusal is
    /// permanent.
    pub fn register(&self) -> Result<TextureRegistration, TextureIdsExhausted> {
        let mut state = self.shared.state.lock();
        if state.exhausted {
            return Err(TextureIdsExhausted);
        }
        let Some(next) = state.last_issued.checked_add(1) else {
            state.exhausted = true;
            return Err(TextureIdsExhausted);
        };
        state.last_issued = next;
        let id = TextureId::new(next);
        state.live.insert(id, Entry::default());
        Ok(TextureRegistration {
            registry: self.clone(),
            id,
        })
    }

    /// How many frames `id` has received, or `None` when it is not live.
    #[must_use]
    pub fn revision(&self, id: TextureId) -> Option<u64> {
        self.shared
            .state
            .lock()
            .live
            .get(&id)
            .map(|entry| entry.revision)
    }

    /// Calls `listener` after every frame `id` receives, until the returned
    /// guard drops. `None` when `id` is not live, or for good once every
    /// listener key has been issued: a reissued key could remove another
    /// guard's listener.
    pub fn listen(
        &self,
        id: TextureId,
        listener: impl Fn() + Send + Sync + 'static,
    ) -> Option<TextureListener> {
        let mut state = self.shared.state.lock();
        let key = state.next_listener;
        // `u64::MAX` is never issued, so the counter stays exhausted.
        let next = key.checked_add(1)?;
        let entry = state.live.get_mut(&id)?;
        entry.listeners.push((key, Arc::new(listener)));
        state.next_listener = next;
        Some(TextureListener {
            registry: self.clone(),
            id,
            key,
        })
    }

    /// Takes the newest pending frame of each live texture and the retired
    /// ids, for the raster backend to apply before it renders.
    #[must_use]
    pub fn take_updates(&self) -> TextureUpdates {
        let mut state = self.shared.state.lock();
        let frames = state
            .live
            .iter_mut()
            .filter_map(|(id, entry)| entry.pending.take().map(|frame| (*id, frame)))
            .collect();
        TextureUpdates {
            frames,
            retired: std::mem::take(&mut state.retired),
        }
    }

    fn submit(&self, id: TextureId, frame: Image) {
        let listeners: Vec<Listener> = {
            let mut state = self.shared.state.lock();
            let Some(entry) = state.live.get_mut(&id) else {
                return;
            };
            entry.pending = Some(frame);
            entry.revision += 1;
            entry
                .listeners
                .iter()
                .map(|(_, listener)| Arc::clone(listener))
                .collect()
        };
        // Outside the lock: a listener may reach this registry again. The
        // frame is already accepted, so a panicking listener skips neither
        // the others nor the wake; the first panic resumes afterwards.
        let mut recovery = PanicRecovery::new();
        for listener in listeners {
            recovery.run(|| listener());
        }
        recovery.run(|| (self.shared.wake)());
        recovery.finish();
    }

    fn retire(&self, id: TextureId) {
        let retired = {
            let mut state = self.shared.state.lock();
            let entry = state.live.remove(&id);
            state.retired.push(id);
            entry
        };
        // The entry's frame and listeners drop outside the lock; the id is
        // already queued, so a panicking capture does not skip the wake.
        let mut recovery = PanicRecovery::new();
        recovery.retire(retired);
        recovery.run(|| (self.shared.wake)());
        recovery.finish();
    }
}

impl fmt::Debug for TextureRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.shared.state.lock();
        f.debug_struct("TextureRegistry")
            .field("live", &state.live.len())
            .field("retired", &state.retired)
            .finish_non_exhaustive()
    }
}

/// An external texture's id and the right to feed it. Dropping it retires the
/// id for good.
#[must_use = "dropping the registration retires its texture"]
pub struct TextureRegistration {
    registry: TextureRegistry,
    id: TextureId,
}

impl TextureRegistration {
    /// The id a `Texture` widget or [`TextureLayer`](crate::TextureLayer) names.
    #[must_use]
    pub fn id(&self) -> TextureId {
        self.id
    }

    /// Hands over the texture's next frame, from any thread. A frame not yet
    /// drawn is replaced; the presentation wakes and the id's listeners run.
    pub fn submit_frame(&self, frame: Image) {
        self.registry.submit(self.id, frame);
    }
}

impl Drop for TextureRegistration {
    fn drop(&mut self) {
        self.registry.retire(self.id);
    }
}

impl fmt::Debug for TextureRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextureRegistration")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Keeps a frame listener installed; dropping it removes the listener.
#[must_use = "dropping the guard removes the listener"]
pub struct TextureListener {
    registry: TextureRegistry,
    id: TextureId,
    key: u64,
}

impl Drop for TextureListener {
    fn drop(&mut self) {
        let removed = {
            let mut state = self.registry.shared.state.lock();
            state.live.get_mut(&self.id).and_then(|entry| {
                let index = entry
                    .listeners
                    .iter()
                    .position(|(key, _)| *key == self.key)?;
                Some(entry.listeners.swap_remove(index))
            })
        };
        drop(removed);
    }
}

impl fmt::Debug for TextureListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextureListener")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The last listener key is issued once; after it every `listen` refuses,
    /// so no key an older guard holds is reissued.
    #[test]
    fn exhausted_listener_keys_refuse_for_good() {
        let registry = TextureRegistry::new(Arc::new(|| {}));
        let texture = registry.register().expect("id");
        let id = texture.id();
        registry.shared.state.lock().next_listener = u64::MAX - 1;
        let last = registry.listen(id, || {}).expect("the last key");
        assert_eq!(last.key, u64::MAX - 1);
        for _ in 0..2 {
            assert!(registry.listen(id, || {}).is_none());
        }
        assert_eq!(
            registry.shared.state.lock().live[&id].listeners.len(),
            1,
            "a refused listener is not installed"
        );
    }
}
