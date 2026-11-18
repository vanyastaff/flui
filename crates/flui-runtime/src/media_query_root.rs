//! Presentation-owned MediaQuery data and its inherited root publisher.
//! Platform updates run on the owner; changed values schedule a mounted root.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::Rect;
use flui_view::environment::{FloatingKeyboard, MediaQuery, MediaQueryData};
use flui_view::prelude::*;
use flui_view::{BoxedView, RebuildHandle};

/// Owner-local shared cell for the root media-query data.
///
/// The UI runtime mutates it from platform signals; the `MediaQueryRoot`
/// element reads it during build. Deliberately `!Send` (`Rc`/`RefCell`):
/// every write side runs on the UI runtime's owner thread.
#[derive(Default)]
pub(crate) struct MediaQuerySource {
    data: RefCell<MediaQueryData>,
    /// The frame of the keyboard floating over the window, published beside
    /// the media query and not in it (ADR-0372): a nested `MediaQuery` copies
    /// its data, and this is the window's alone.
    floating_keyboard: Cell<Option<Rect>>,
    rebuild: RefCell<Option<(u64, RebuildHandle)>>,
    generation: Cell<u64>,
}

impl std::fmt::Debug for MediaQuerySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaQuerySource")
            .field("data", &self.data.borrow())
            .field("floating_keyboard", &self.floating_keyboard.get())
            .field("generation", &self.generation.get())
            .finish_non_exhaustive()
    }
}

impl MediaQuerySource {
    /// Mutate the published data and schedule the root republish.
    ///
    /// A mutation before the root wrapper has mounted (bootstrap ordering)
    /// just updates the cell — the first build reads the fresh value, so
    /// the missing handle loses nothing.
    pub(crate) fn update(&self, mutate: impl FnOnce(&mut MediaQueryData)) {
        if let Some(handle) = self.commit(mutate) {
            handle.schedule(flui_view::RebuildReason::StateChange);
        }
    }

    /// Commit inherited data without waking before companion state is published.
    #[must_use = "schedule the returned root rebuild after publication guards release"]
    pub(crate) fn commit(&self, mutate: impl FnOnce(&mut MediaQueryData)) -> Option<RebuildHandle> {
        let changed = {
            let mut data = self.data.borrow_mut();
            let before = data.clone();
            mutate(&mut data);
            *data != before
        };
        if changed { self.root_rebuild() } else { None }
    }

    /// Commit the frame of the keyboard floating over the window, `None`
    /// while none floats, without waking: like [`Self::commit`], it hands
    /// back the root rebuild when the frame changed.
    #[must_use = "schedule the returned root rebuild after publication guards release"]
    pub(crate) fn commit_floating_keyboard(&self, frame: Option<Rect>) -> Option<RebuildHandle> {
        if self.floating_keyboard.replace(frame) == frame {
            None
        } else {
            self.root_rebuild()
        }
    }

    /// The mounted root's rebuild handle, cloned out of its slot so that the
    /// wake runs with no borrow of this source held.
    fn root_rebuild(&self) -> Option<RebuildHandle> {
        self.rebuild
            .borrow()
            .as_ref()
            .map(|(_, handle)| handle.clone())
    }

    pub(crate) fn from_window(
        window: &dyn flui_platform::PlatformWindow,
        device_pixel_ratio: f64,
    ) -> Self {
        let snapshot = window.state();
        Self {
            floating_keyboard: Cell::new(snapshot.insets.keyboard().floating_frame()),
            data: RefCell::new(MediaQueryData {
                size: snapshot.metrics.map_or(
                    snapshot.content_size.get(),
                    flui_foundation::geometry::WindowMetrics::logical,
                ),
                device_pixel_ratio,
                view_padding: snapshot.insets.safe_area(),
                view_insets: snapshot
                    .insets
                    .keyboard()
                    .motion()
                    .map_or(snapshot.insets.keyboard().target(), |motion| motion.from()),
                system_gesture_insets: snapshot.insets.system_gestures(),
                ..MediaQueryData::default()
            }),
            ..Self::default()
        }
    }

    fn snapshot(&self) -> MediaQueryData {
        self.data.borrow().clone()
    }

    fn install_rebuild_handle(self: &Rc<Self>, handle: RebuildHandle) -> MediaQueryRegistration {
        let token = self
            .generation
            .get()
            .checked_add(1)
            .expect("BUG: media query registration exhausted");
        self.generation.set(token);
        let displaced = self.rebuild.borrow_mut().replace((token, handle));
        drop(displaced);
        MediaQueryRegistration {
            source: Rc::downgrade(self),
            token,
        }
    }
}

/// Scoped ownership of one source's rebuild slot, minted per registration.
///
/// The token exists so a drop can tell *its* slot from a successor's: the
/// release happens only if the slot still holds the token this registration
/// installed. A stale registration therefore cannot clear a live one, which
/// is what makes the swap in [`MediaQueryRootState::did_update_view`] safe
/// regardless of drop order.
///
/// The interleaving that needs the token is not reachable today — a source
/// has exactly one live registration, installed by the one element
/// publishing it, and the old registration is dropped before the new one is
/// installed. The weak handle keeps the drop harmless for a source that has
/// already gone away, which *is* reachable: the tree can die before the
/// element's dispose runs.
struct MediaQueryRegistration {
    source: std::rc::Weak<MediaQuerySource>,
    token: u64,
}
impl Drop for MediaQueryRegistration {
    fn drop(&mut self) {
        if let Some(source) = self.source.upgrade() {
            let mut slot = source.rebuild.borrow_mut();
            if slot.as_ref().is_some_and(|(token, _)| *token == self.token) {
                slot.take();
            }
        }
    }
}

/// Publishes one presentation's [`MediaQuerySource`] as the root `MediaQuery`,
/// and the window's floating keyboard, while one floats, as a provided
/// `FloatingKeyboard`.
///
/// Installed by the primary root attach paths and the addressed sized attach
/// used by headless multi-window consumers. Each wrapper publishes only its
/// own presentation's source; no caller outside the runtime mutates that cell.
#[derive(Clone)]
pub(crate) struct MediaQueryRoot {
    source: Rc<MediaQuerySource>,
    child: BoxedView,
}

impl MediaQueryRoot {
    pub(crate) fn new(source: Rc<MediaQuerySource>, child: BoxedView) -> Self {
        Self { source, child }
    }
}

impl flui_view::View for MediaQueryRoot {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

pub(crate) struct MediaQueryRootState {
    source: Rc<MediaQuerySource>,
    registration: Option<MediaQueryRegistration>,
    handle: RebuildHandle,
}

impl flui_view::StatefulView for MediaQueryRoot {
    type State = MediaQueryRootState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        MediaQueryRootState {
            source: Rc::clone(&self.source),
            registration: None,
            handle: ctx.rebuild_handle(),
        }
    }
}

impl flui_view::ViewState<MediaQueryRoot> for MediaQueryRootState {
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
        self.registration = Some(self.source.install_rebuild_handle(self.handle.clone()));
    }

    fn did_update_view(&mut self, _old_view: &MediaQueryRoot, new_view: &MediaQueryRoot) {
        if !Rc::ptr_eq(&self.source, &new_view.source) {
            // A registration is only valid for the source that minted it, so
            // a new source means releasing the old slot before taking a new
            // one. The ui_runtime cannot reach this today: it hands the same
            // presentation-owned source to every attach, so the `ptr_eq`
            // above holds and this branch stays cold. It is what keeps
            // `MediaQueryRoot` correct as a plain view — a wrapper that can
            // be re-rendered with a different source — rather than only as
            // the ui_runtime's root wrapper.
            self.registration.take();
            self.source = Rc::clone(&new_view.source);
            self.registration = Some(self.source.install_rebuild_handle(self.handle.clone()));
        }
    }

    fn dispose(&mut self) {
        self.registration.take();
    }

    /// The window's floating keyboard while one floats, and nothing
    /// otherwise: a root that publishes nothing adds no layer to every
    /// node's inherited scope.
    fn provide(
        &self,
        _view: &MediaQueryRoot,
        _ctx: &dyn BuildContext,
        values: &mut flui_view::Provided<'_>,
    ) {
        if let Some(frame) = self.source.floating_keyboard.get() {
            values.value(FloatingKeyboard::new(frame));
        }
    }

    fn build(&self, view: &MediaQueryRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        MediaQuery::new(self.source.snapshot(), view.child.clone())
    }
}
