//! The realm-owned live source behind the root [`MediaQuery`], and the
//! stateful wrapper that publishes it into the widget tree.
//!
//! Nothing used to install an ambient `MediaQuery` at all — the
//! `platform_brightness` republish this crate's docs promised was
//! unimplemented, and every `MediaQuery::maybe_of` consumer fell back to
//! defaults. The realm now owns one [`MediaQuerySource`] per presentation:
//! the platform's resize path keeps `size`/`device_pixel_ratio` current,
//! the appearance path keeps `platform_brightness` current, and the
//! [`MediaQueryRoot`] wrapper re-publishes on every change through the
//! rebuild handle it registered at mount (ADR-0018: the capability is
//! acquired in `init_state`, used later from the owner thread).

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
    rebuild: Cell<Option<RebuildHandle>>,
}

impl MediaQuerySource {
    /// Mutate the published data and schedule the root republish.
    ///
    /// A mutation before the root wrapper has mounted (bootstrap ordering)
    /// just updates the cell — the first build reads the fresh value, so
    /// the missing handle loses nothing.
    pub(crate) fn update(&self, mutate: impl FnOnce(&mut MediaQueryData)) {
        mutate(&mut self.data.borrow_mut());
        if let Some(handle) = self.rebuild.take() {
            handle.schedule(flui_view::RebuildReason::StateChange);
            // The handle is reusable; put it back for the next signal.
            self.rebuild.set(Some(handle));
        }
    }

    fn snapshot(&self) -> MediaQueryData {
        self.data.borrow().clone()
    }

    fn install_rebuild_handle(&self, handle: RebuildHandle) {
        self.rebuild.set(Some(handle));
    }
}

/// Publishes the realm's [`MediaQuerySource`] as the root `MediaQuery`.
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
}

impl flui_view::StatefulView for MediaQueryRoot {
    type State = MediaQueryRootState;

    fn create_state(&self) -> Self::State {
        MediaQueryRootState {
            source: Rc::clone(&self.source),
        }
    }
}

impl flui_view::ViewState<MediaQueryRoot> for MediaQueryRootState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.source.install_rebuild_handle(ctx.rebuild_handle());
    }

    fn did_update_view(&mut self, _old_view: &MediaQueryRoot, new_view: &MediaQueryRoot) {
        self.source = Rc::clone(&new_view.source);
    }

    fn build(&self, view: &MediaQueryRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        MediaQuery::new(self.source.snapshot(), view.child.clone())
    }
}
