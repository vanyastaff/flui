//! `Visibility` -- show/hide a child, optionally preserving its state via
//! `Offstage`. Verifies the build branches and animation policy documented in
//! `crates/flui-widgets/src/interaction/visibility.rs`.

use crate::common::{lay_out, lay_out_animated, loose, size};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use flui_animation::{Animation, AnimationController, DrivenController, Vsync};
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView, StatelessView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};
// Only the `#[cfg(debug_assertions)]` invalid-configuration tests drive a tree
// by hand and assert on the ErrorView substitution, which is debug-only.
use flui_widgets::{SizedBox, Visibility, VsyncScope};
use parking_lot::Mutex;

const FRAME: Duration = Duration::from_millis(20);

#[derive(Clone, StatefulView)]
struct AnimationProbe {
    observer: Rc<RefCell<Option<AnimationController>>>,
    found_ambient: Arc<Mutex<Option<bool>>>,
    init_count: Arc<AtomicUsize>,
    dispose_count: Arc<AtomicUsize>,
}

struct AnimationProbeState {
    observer: Rc<RefCell<Option<AnimationController>>>,
    found_ambient: Arc<Mutex<Option<bool>>>,
    init_count: Arc<AtomicUsize>,
    dispose_count: Arc<AtomicUsize>,
    owner: Option<DrivenController>,
}

impl StatefulView for AnimationProbe {
    type State = AnimationProbeState;

    fn create_state(&self) -> Self::State {
        AnimationProbeState {
            observer: self.observer.clone(),
            found_ambient: Arc::clone(&self.found_ambient),
            init_count: Arc::clone(&self.init_count),
            dispose_count: Arc::clone(&self.dispose_count),
            owner: None,
        }
    }
}

impl std::fmt::Debug for AnimationProbeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationProbeState")
            .finish_non_exhaustive()
    }
}

impl ViewState<AnimationProbe> for AnimationProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.init_count.fetch_add(1, Ordering::Relaxed);
        let ambient = VsyncScope::maybe_of(ctx);
        *self.found_ambient.lock() = Some(ambient.is_some());
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(ambient.as_ref());
        *self.observer.borrow_mut() = Some(owner.controller().clone());
        self.owner = Some(owner);
    }

    fn dispose(&mut self) {
        self.dispose_count.fetch_add(1, Ordering::Relaxed);
        drop(self.owner.take());
    }

    fn build(&self, _view: &AnimationProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

type AnimationProbeFixture = (
    AnimationProbe,
    Arc<Mutex<Option<bool>>>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
);

fn animation_probe(observer: &Rc<RefCell<Option<AnimationController>>>) -> AnimationProbeFixture {
    let found_ambient = Arc::new(Mutex::new(None));
    let init_count = Arc::new(AtomicUsize::new(0));
    let dispose_count = Arc::new(AtomicUsize::new(0));
    (
        AnimationProbe {
            observer: observer.clone(),
            found_ambient: Arc::clone(&found_ambient),
            init_count: Arc::clone(&init_count),
            dispose_count: Arc::clone(&dispose_count),
        },
        found_ambient,
        init_count,
        dispose_count,
    )
}

#[derive(Clone, StatelessView)]
struct VisibilityToggleHost {
    visible: Arc<AtomicBool>,
    mounted: Arc<AtomicBool>,
    probe: AnimationProbe,
}

impl StatelessView for VisibilityToggleHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let child: BoxedView = if self.mounted.load(Ordering::Relaxed) {
            Visibility::new(self.probe.clone())
                .visible(self.visible.load(Ordering::Relaxed))
                .maintain_state(true)
                .boxed()
        } else {
            SizedBox::shrink().boxed()
        };
        child
    }
}

pub(crate) fn maintained_child_mutes_and_resumes_without_remounting_as_visibility_changes() {
    let vsync = Vsync::new();
    let observer = Rc::new(RefCell::new(None));
    let (probe, found_ambient, init_count, dispose_count) = animation_probe(&observer);
    let visible = Arc::new(AtomicBool::new(true));
    let mounted = Arc::new(AtomicBool::new(true));
    let host = VisibilityToggleHost {
        visible: Arc::clone(&visible),
        mounted: Arc::clone(&mounted),
        probe,
    };
    let root = VsyncScope::new(vsync.clone(), host);
    let mut laid = lay_out_animated(root, loose(100.0), vsync);

    assert_eq!(*found_ambient.lock(), Some(true));
    assert_eq!(init_count.load(Ordering::Relaxed), 1);
    assert_eq!(dispose_count.load(Ordering::Relaxed), 0);

    let controller = observer
        .borrow()
        .clone()
        .expect("mounted controller observer");
    controller.forward().expect("animation should start");
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let visible_value = controller.value();
    assert!(visible_value > 0.0, "the visible controller should advance");

    visible.store(false, Ordering::Relaxed);
    laid.pump();
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let hidden_value = controller.value();
    assert_eq!(
        hidden_value, visible_value,
        "the hidden controller should remain at its visible value"
    );
    assert_eq!(init_count.load(Ordering::Relaxed), 1);
    assert_eq!(dispose_count.load(Ordering::Relaxed), 0);

    visible.store(true, Ordering::Relaxed);
    laid.pump();
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert!(
        controller.value() > hidden_value,
        "the controller should resume after becoming visible"
    );
    assert_eq!(init_count.load(Ordering::Relaxed), 1);
    assert_eq!(dispose_count.load(Ordering::Relaxed), 0);

    mounted.store(false, Ordering::Relaxed);
    laid.pump();
    assert_eq!(dispose_count.load(Ordering::Relaxed), 1);
    let unmounted_value = controller.value();
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert_eq!(
        controller.value(),
        unmounted_value,
        "disposing the probe should unregister it from ambient Vsync"
    );
}

pub(crate) fn hidden_without_maintain_state_shows_the_default_replacement() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0)).visible(false),
        loose(1000.0),
    );

    // Default replacement is SizedBox::shrink() -- the real 30x20 child must
    // be entirely absent, replaced by a zero-size box.
    assert_eq!(laid.size(laid.root()), size(0.0, 0.0));
}
