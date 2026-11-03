//! A widget registers an external texture through its lifecycle capability;
//! a frame fed from another thread repaints the `Texture` showing it, and
//! nothing around it, reaches the raster side through the presentation's
//! registry, and unmounting the owner retires the texture.

use std::sync::atomic::AtomicU32;

use flui_foundation::geometry::{Rect, Size};
use flui_layer::{DamageRect, DamageRegion, LayerDiffer, TextureRegistration};
use flui_painting::Alignment;
use flui_painting::paint::{Image, TextureId};
use flui_painting::styling::Color;
use flui_view::Signal;
use flui_widgets::{Align, ColoredBox, Offstage, Padding, Texture};
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

/// The test window's surface, in physical pixels at scale 1.
const SURFACE: (u32, u32) = (800, 600);

/// Where the root puts the surface.
#[derive(Clone, Copy)]
enum Placement {
    /// The surface is the whole root; hiding it unmounts it.
    Whole,
    /// A 20-px surface at (100, 100) over a background; hiding it keeps it
    /// mounted, offstage.
    Small,
}

/// Shows the surface while `shown` is non-zero.
#[derive(Clone)]
struct Toggle {
    shown: Signal<u32>,
    surface: VideoSurface,
    placement: Placement,
}

impl flui_view::View for Toggle {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Toggle {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let shown = self.shown.get(ctx) != 0;
        match self.placement {
            Placement::Whole if shown => self.surface.clone().boxed(),
            Placement::Whole => SizedBox::square(1.0).boxed(),
            Placement::Small => ColoredBox::new(Color::BLUE)
                .child(
                    Align::new(Alignment::TOP_LEFT).child(
                        Padding::only(100.0, 100.0, 0.0, 0.0).child(
                            Offstage::new()
                                .offstage(!shown)
                                .child(SizedBox::new(20.0, 20.0).child(self.surface.clone())),
                        ),
                    ),
                )
                .boxed(),
        }
    }
}

/// The presentation holds its window weakly, as a backend owns it; the
/// fixture keeps it alive.
struct Fixture {
    ui_runtime: UiRuntime,
    shared: Shared,
    shown: Signal<u32>,
    writer: flui_view::WriterSource,
    submits: Arc<AtomicU32>,
    _window: Rc<dyn flui_platform::PlatformWindow>,
}

fn mounted(placement: Placement) -> Fixture {
    let window: Rc<dyn flui_platform::PlatformWindow> =
        Rc::new(crate::testing::TestWindow::new().focused(true).with_sizes(
            Size::new(
                i32::try_from(SURFACE.0).expect("the width fits"),
                i32::try_from(SURFACE.1).expect("the height fits"),
            ),
            Size::new(f64::from(SURFACE.0), f64::from(SURFACE.1)),
        ));
    let ui_runtime = UiRuntime::for_test_with_window(Rc::clone(&window));
    let graph = ui_runtime
        .presentation_widgets_for_test(ui_runtime.presentation_id())
        .with_build_owner(|owner| owner.signal_graph().clone());
    let shown = graph.signal(1_u32);
    let writer = ui_runtime
        .presentation_widgets_for_test(ui_runtime.presentation_id())
        .with_build_owner(flui_view::BuildOwner::writer_source);
    let shared = Shared::default();
    ui_runtime
        .attach_root_widget(&Toggle {
            shown,
            surface: VideoSurface {
                shared: Arc::clone(&shared),
            },
            placement,
        })
        .expect("root attaches");
    Fixture {
        ui_runtime,
        shared,
        shown,
        writer,
        submits: Arc::new(AtomicU32::new(0)),
        _window: window,
    }
}

fn counting_sink(submits: &Arc<AtomicU32>) -> ScriptedSink {
    let submits = Arc::clone(submits);
    ScriptedSink::new(move |_, _| {
        submits.fetch_add(1, Ordering::SeqCst);
        crate::sink::SubmitVerdict::Presented
    })
}

/// A sink that diffs every scene it is handed against the one before, as the
/// raster lane does, and keeps each frame's damage.
fn damage_sink(damage: &Arc<Mutex<Vec<DamageRegion>>>) -> ScriptedSink {
    let damage = Arc::clone(damage);
    let mut differ = LayerDiffer::default();
    ScriptedSink::new(move |_, scene| {
        damage.lock().push(differ.diff(scene, SURFACE));
        crate::sink::SubmitVerdict::Presented
    })
}

/// Feeds the registered texture one frame from another thread, as a decoder
/// does.
fn feed(shared: &Shared, shade: u8) {
    let shared = Arc::clone(shared);
    std::thread::spawn(move || {
        let registration = shared.lock();
        let registration = registration.as_ref().expect("registered");
        registration.submit_frame(Image::from_rgba8(1, 1, vec![shade, shade, shade, 255]));
    })
    .join()
    .expect("producer thread");
}

/// A frame fed from another thread repaints the texture's widget, and the
/// presentation's registry hands that frame to the raster side once.
pub(crate) fn a_fed_frame_repaints_and_reaches_the_raster_side() {
    let fixture = mounted(Placement::Whole);
    let mut sink = counting_sink(&fixture.submits);
    fixture.ui_runtime.render_frame(&mut sink);
    fixture.ui_runtime.render_frame(&mut sink);
    assert_eq!(
        fixture.submits.load(Ordering::SeqCst),
        1,
        "nothing changed after the first frame"
    );
    feed(&fixture.shared, 9);
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

/// A frame fed to a small texture over a background damages the texture's
/// box alone: the texture's layer repaints and the background is replayed.
pub(crate) fn a_fed_frame_damages_only_the_texture() {
    let fixture = mounted(Placement::Small);
    let damage = Arc::new(Mutex::new(Vec::new()));
    let mut sink = damage_sink(&damage);
    fixture.ui_runtime.render_frame(&mut sink);
    fixture.ui_runtime.render_frame(&mut sink);
    assert_eq!(
        damage.lock().len(),
        1,
        "nothing changed after the first frame"
    );

    feed(&fixture.shared, 9);
    fixture.ui_runtime.render_frame(&mut sink);
    let damage = damage.lock();
    assert_eq!(damage.len(), 2, "the texture's frame is submitted");
    let texture = DamageRect::covering(Rect::from_ltrb(100.0, 100.0, 120.0, 120.0), SURFACE)
        .expect("the texture is on the surface");
    assert_eq!(
        damage[1],
        DamageRegion::Partial(texture),
        "a texture frame damages the texture's box, not its enclosing boundary"
    );
}

/// A texture an ancestor hides while it stays mounted stops costing frames:
/// at most one frame follows the producer, and the texture's pending paint
/// absorbs the rest.
pub(crate) fn a_hidden_texture_stops_painting_after_one_frame() {
    let fixture = mounted(Placement::Small);
    let mut sink = counting_sink(&fixture.submits);
    fixture.ui_runtime.render_frame(&mut sink);
    fixture
        .writer
        .write(|cx| fixture.shown.set(cx, 0))
        .expect("write");
    fixture.ui_runtime.render_frame(&mut sink);
    assert!(
        fixture.shared.lock().is_some(),
        "an offstage texture stays mounted"
    );
    let hidden = fixture.submits.load(Ordering::SeqCst);

    for shade in 1..=3 {
        feed(&fixture.shared, shade);
        fixture.ui_runtime.render_frame(&mut sink);
    }
    assert!(
        fixture.submits.load(Ordering::SeqCst) <= hidden + 1,
        "a hidden texture costs at most one frame, not one per producer frame"
    );
}

/// Unmounting the widget that owns the registration retires its texture.
pub(crate) fn unmounting_the_owner_retires_its_texture() {
    let fixture = mounted(Placement::Whole);
    let mut sink = counting_sink(&fixture.submits);
    fixture.ui_runtime.render_frame(&mut sink);
    let id = fixture
        .shared
        .lock()
        .as_ref()
        .map(TextureRegistration::id)
        .expect("registered");
    fixture
        .writer
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
