//! Presentation-owned MediaQuery data and its inherited root publisher.
//! Platform updates run on the owner; changed values schedule a mounted root.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_view::prelude::*;
use flui_view::{BoxedView, RebuildHandle};
use flui_widgets::{MediaQuery, MediaQueryData};

/// Owner-local shared cell for the root media-query data.
///
/// The realm mutates it from platform signals; the [`MediaQueryRoot`]
/// element reads it during build. Deliberately `!Send` (`Rc`/`RefCell`):
/// every write side runs on the realm's owner thread.
#[derive(Default)]
pub(crate) struct MediaQuerySource {
    data: RefCell<MediaQueryData>,
    rebuild: RefCell<Option<(u64, RebuildHandle)>>,
    generation: Cell<u64>,
}

impl MediaQuerySource {
    /// Mutate the published data and schedule the root republish.
    ///
    /// A mutation before the root wrapper has mounted (bootstrap ordering)
    /// just updates the cell — the first build reads the fresh value, so
    /// the missing handle loses nothing.
    pub(crate) fn update(&self, mutate: impl FnOnce(&mut MediaQueryData)) {
        let changed = {
            let mut data = self.data.borrow_mut();
            let before = data.clone();
            mutate(&mut data);
            *data != before
        };
        if changed {
            let handle = self
                .rebuild
                .borrow()
                .as_ref()
                .map(|(_, handle)| handle.clone());
            if let Some(handle) = handle {
                handle.schedule(flui_view::RebuildReason::StateChange);
            }
        }
    }

    pub(crate) fn from_window(window: &dyn flui_platform_api::PlatformWindow) -> Self {
        Self {
            data: RefCell::new(MediaQueryData {
                size: window.logical_size(),
                device_pixel_ratio: window.scale_factor() as f32,
                padding: window.safe_area_insets(),
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

/// Publishes one presentation's [`MediaQuerySource`] as the root `MediaQuery`.
///
/// Currently installed from the `primary()` attach path only (`UiRealm::
/// attach_root_widget_entered` and its sized variant), so a non-primary
/// presentation's source is written by the addressed arms and not yet read —
/// see `docs/BETA.md` § "iOS safe-area layout" for why that split is stated
/// rather than hidden.
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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

pub(crate) struct MediaQueryRootState {
    source: Rc<MediaQuerySource>,
    registration: Option<MediaQueryRegistration>,
    handle: Option<RebuildHandle>,
}

impl flui_view::StatefulView for MediaQueryRoot {
    type State = MediaQueryRootState;

    fn create_state(&self) -> Self::State {
        MediaQueryRootState {
            source: Rc::clone(&self.source),
            registration: None,
            handle: None,
        }
    }
}

impl flui_view::ViewState<MediaQueryRoot> for MediaQueryRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let handle = ctx.rebuild_handle();
        self.registration = Some(self.source.install_rebuild_handle(handle.clone()));
        self.handle = Some(handle);
    }

    fn did_update_view(&mut self, _old_view: &MediaQueryRoot, new_view: &MediaQueryRoot) {
        if !Rc::ptr_eq(&self.source, &new_view.source) {
            // A registration is only valid for the source that minted it, so
            // a new source means releasing the old slot before taking a new
            // one. The realm cannot reach this today: it hands the same
            // presentation-owned source to every attach, so the `ptr_eq`
            // above holds and this branch stays cold. It is what keeps
            // `MediaQueryRoot` correct as a plain view — a wrapper that can
            // be re-rendered with a different source — rather than only as
            // the realm's root wrapper.
            self.registration.take();
            self.source = Rc::clone(&new_view.source);
            if let Some(handle) = self.handle.clone() {
                self.registration = Some(self.source.install_rebuild_handle(handle));
            }
        }
    }

    fn dispose(&mut self) {
        self.registration.take();
        self.handle.take();
    }

    fn build(&self, view: &MediaQueryRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        MediaQuery::new(self.source.snapshot(), view.child.clone())
    }
}
