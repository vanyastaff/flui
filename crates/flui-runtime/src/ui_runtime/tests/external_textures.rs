//! A widget registers an external texture through its lifecycle capability;
//! a frame fed from another thread repaints the `Texture` showing it, reaches
//! the raster side through the presentation's registry, and unmounting the
//! owner retires the texture.

use std::sync::atomic::AtomicU32;

use flui_layer::TextureRegistration;
use flui_painting::paint::{Image, TextureId};
use flui_view::{Reactive, Signal};
use flui_widgets::Texture;
use parking_lot::Mutex;

use super::*;

type Shared = Arc<Mutex<Option<TextureRegistration>>>;

/// Registers a texture when mounted, shares the registration with the test
/// and shows the texture; dropping its state retires it.
#[derive(Clone)]
struct VideoSurface {
    shared: Shared,
}

struct VideoSurfaceState {
    shared: Shared,
    id: Option<TextureId>,
}

impl flui_view::View for VideoSurface {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for VideoSurface {
    type State = VideoSurfaceState;

    fn create_state(&self) -> Self::State {
        VideoSurfaceState {
            shared: Arc::clone(&self.shared),
            id: None,
        }
    }
}

impl ViewState<VideoSurface> for VideoSurfaceState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let registry = ctx
            .texture_registry()
            .expect("a presentation offers external textures");
        let registration = registry.register().expect("a texture id");
        self.id = Some(registration.id());
        *self.shared.lock() = Some(registration);
    }

    fn build(&self, _view: &VideoSurface, _ctx: &dyn BuildContext) -> impl IntoView {
        Texture::new(self.id.expect("registered in init_state"))
    }

    fn dispose(&mut self) {
        self.shared.lock().take();
    }
}

/// Shows the surface while `shown` is non-zero.
#[derive(Clone)]
struct Toggle {
    shown: Signal<u32>,
    surface: VideoSurface,
}

impl flui_view::View for Toggle {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Toggle {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        if self.shown.get(ctx) == 0 {
            SizedBox::square(1.0).boxed()
        } else {
            self.surface.clone().boxed()
        }
    }
}

struct Fixture {
    ui_runtime: UiRuntime,
    shared: Shared,
    shown: Signal<u32>,
    graph: Reactive,
    submits: Arc<AtomicU32>,
}

fn mounted() -> Fixture {
    let ui_runtime = UiRuntime::for_test();
    let graph = ui_runtime
        .presentation_widgets_for_test(ui_runtime.presentation_id())
        .with_build_owner(|owner| owner.signal_graph().clone());
    let shown = graph.signal(1_u32);
    let shared = Shared::default();
    ui_runtime
        .attach_root_widget(&Toggle {
            shown,
            surface: VideoSurface {
                shared: Arc::clone(&shared),
            },
        })
        .expect("root attaches");
    Fixture {
        ui_runtime,
        shared,
        shown,
        graph,
        submits: Arc::new(AtomicU32::new(0)),
    }
}

fn counting_sink(submits: &Arc<AtomicU32>) -> ScriptedSink {
    let submits = Arc::clone(submits);
    ScriptedSink::new(move |_, _| {
        submits.fetch_add(1, Ordering::SeqCst);
        crate::sink::SubmitVerdict::Presented
    })
}

/// A frame fed from another thread repaints the texture's widget, and the
/// presentation's registry hands that frame to the raster side once.
pub(crate) fn a_fed_frame_repaints_and_reaches_the_raster_side() {
    let fixture = mounted();
    let mut sink = counting_sink(&fixture.submits);
    fixture.ui_runtime.render_frame(&mut sink);
    fixture.ui_runtime.render_frame(&mut sink);
    assert_eq!(
        fixture.submits.load(Ordering::SeqCst),
        1,
        "nothing changed after the first frame"
    );
    let shared = Arc::clone(&fixture.shared);
    std::thread::spawn(move || {
        let registration = shared.lock();
        let registration = registration.as_ref().expect("registered");
        registration.submit_frame(Image::from_rgba8(1, 1, vec![9, 9, 9, 255]));
    })
    .join()
    .expect("producer thread");
    fixture.ui_runtime.render_frame(&mut sink);
    assert_eq!(
        fixture.submits.load(Ordering::SeqCst),
        2,
        "the new frame repaints the texture"
    );
    let registry = fixture
        .ui_runtime
        .texture_registry(fixture.ui_runtime.presentation_id())
        .expect("primary presentation");
    let id = fixture.shared.lock().as_ref().map(TextureRegistration::id);
    let updates = registry.take_updates();
    assert_eq!(updates.frames.len(), 1);
    assert_eq!(Some(updates.frames[0].0), id);
}

/// Unmounting the widget that owns the registration retires its texture.
pub(crate) fn unmounting_the_owner_retires_its_texture() {
    let fixture = mounted();
    let mut sink = counting_sink(&fixture.submits);
    fixture.ui_runtime.render_frame(&mut sink);
    let id = fixture
        .shared
        .lock()
        .as_ref()
        .map(TextureRegistration::id)
        .expect("registered");
    fixture
        .graph
        .writer_source()
        .write(|cx| fixture.shown.set(cx, 0))
        .expect("write");
    fixture.ui_runtime.render_frame(&mut sink);
    assert!(fixture.shared.lock().is_none(), "the owner was disposed");
    let registry = fixture
        .ui_runtime
        .texture_registry(fixture.ui_runtime.presentation_id())
        .expect("primary presentation");
    assert_eq!(registry.take_updates().retired, [id]);
}
