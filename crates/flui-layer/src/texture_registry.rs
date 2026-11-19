//! External textures a presentation draws through [`TextureLayer`]s, fed with
//! frames from any thread.
//!
//! A [`TextureRegistry`] belongs to one presentation. [`TextureRegistry::register`]
//! issues a [`TextureRegistration`]: an owned handle whose [`TextureId`] is never
//! issued again, which accepts frames until it is dropped. Each frame is held as
//! its id's pending frame and wakes the presentation, as Flutter's
//! `markTextureFrameAvailable` schedules a frame: the runtime's next frame,
//! finding the registry holding updates ([`TextureRegistry::has_updates`]),
//! publishes the version its scene holds even when nothing in the tree changed,
//! and the raster host takes the newest frame per id
//! ([`TextureRegistry::take_updates`]) with that publication (ADR-0304 §8). A
//! producer that outruns the display only costs its skipped frames. Dropping a
//! registration retires its id; the backend releases the allocation on its next
//! update.
//!
//! [`TextureLayer`]: crate::TextureLayer

use std::{fmt, sync::Arc};

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
    /// Each live texture's frame not yet taken.
    live: rustc_hash::FxHashMap<TextureId, Option<Image>>,
    retired: Vec<TextureId>,
}

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

    /// Fold `newer`, taken after these, into these, as one update that
    /// applies both: a texture's frame in `newer` replaces its frame here, a
    /// texture `newer` retires loses its frame here, and the retirements of
    /// both are kept. An id is never issued again, so no frame follows its
    /// retirement.
    pub fn merge(&mut self, newer: Self) {
        let Self { frames, retired } = newer;
        if self.is_empty() {
            *self = Self { frames, retired };
            return;
        }
        self.frames.retain(|(id, _)| {
            !retired.contains(id) && !frames.iter().any(|(newer_id, _)| newer_id == id)
        });
        self.frames.extend(frames);
        self.retired.extend(retired);
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
        state.live.insert(id, None);
        Ok(TextureRegistration {
            registry: self.clone(),
            id,
        })
    }

    /// Whether a frame or a retirement waits for [`Self::take_updates`]: what
    /// the presentation's next frame publishes the held version for, when
    /// nothing in its tree changed (ADR-0304 §8).
    #[must_use]
    pub fn has_updates(&self) -> bool {
        let state = self.shared.state.lock();
        !state.retired.is_empty() || state.live.values().any(Option::is_some)
    }

    /// Takes the newest pending frame of each live texture and the retired
    /// ids, for the raster backend to apply before it renders.
    #[must_use]
    pub fn take_updates(&self) -> TextureUpdates {
        let mut state = self.shared.state.lock();
        let frames = state
            .live
            .iter_mut()
            .filter_map(|(id, pending)| pending.take().map(|frame| (*id, frame)))
            .collect();
        TextureUpdates {
            frames,
            retired: std::mem::take(&mut state.retired),
        }
    }

    fn submit(&self, id: TextureId, frame: Image) {
        // The frame a newer one replaces drops after the lock.
        let replaced = {
            let mut state = self.shared.state.lock();
            let Some(pending) = state.live.get_mut(&id) else {
                return;
            };
            pending.replace(frame)
        };
        // Outside the lock: the wake is the host's, and may reach this
        // registry again. The frame is already pending, so a wake that panics
        // leaves it for whatever frame the presentation draws next.
        (self.shared.wake)();
        drop(replaced);
    }

    fn retire(&self, id: TextureId) {
        let retired = {
            let mut state = self.shared.state.lock();
            let pending = state.live.remove(&id);
            state.retired.push(id);
            pending
        };
        // The id is already queued, so a wake that panics leaves the
        // retirement for the presentation's next frame; the frame it never
        // drew drops after the wake, outside the lock.
        (self.shared.wake)();
        drop(retired);
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
    /// drawn is replaced, and the presentation wakes to draw it.
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
