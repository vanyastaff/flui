//! A widget registers an external texture through its lifecycle capability;
//! a frame fed from another thread rebuilds and repaints nothing: the
//! presentation publishes the version it holds, the raster side takes the
//! frame with it from the registry, and unmounting the owner retires the
//! texture.

use flui_foundation::geometry::Rect;
use flui_layer::{DamageRect, DamageRegion, TextureRegistration};
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
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for VideoSurface {
    type State = VideoSurfaceState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
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
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
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
    _window: Rc<dyn flui_platform::PlatformWindow>,
}

fn mounted(placement: Placement) -> Fixture {
    let window: Rc<dyn flui_platform::PlatformWindow> = Rc::new(
        flui_testing::MockWindow::new()
            .focused(true)
            .with_size(f64::from(SURFACE.0), f64::from(SURFACE.1)),
    );
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
        _window: window,
    }
}

/// What a sink saw of one publication: the version it names, its scene
/// damage, and the texture frames it took from the registry with it.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    version: flui_layer::SceneVersionId,
    damage: DamageRegion,
    frames: Vec<(TextureId, u8)>,
}

/// A sink that takes the presentation's texture updates with every frame it
/// presents, as the raster host does, and answers `verdict` for each call;
/// a frame it does not present leaves them in the registry.
fn taking_sink(
    fixture: &Fixture,
    seen: &Arc<Mutex<Vec<Seen>>>,
    mut verdict: impl FnMut(u32) -> crate::sink::SubmitVerdict + Send + 'static,
) -> ScriptedSink {
    let registry = fixture
        .ui_runtime
        .texture_registry(fixture.ui_runtime.presentation_id())
        .expect("primary presentation");
    let seen = Arc::clone(seen);
    ScriptedSink::new(move |index, publication| {
        let answer = verdict(index);
        let frames = if answer == crate::sink::SubmitVerdict::Presented {
            registry
                .take_updates()
                .frames
                .iter()
                .map(|(id, image)| (*id, image.data()[0]))
                .collect()
        } else {
            Vec::new()
        };
        seen.lock().push(Seen {
            version: publication.scene.id(),
            damage: publication.damage,
            frames,
        });
        answer
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

fn texture_id(fixture: &Fixture) -> TextureId {
    fixture
        .shared
        .lock()
        .as_ref()
        .map(TextureRegistration::id)
        .expect("registered")
}

/// A frame fed from another thread, with nothing changed in the tree, wakes
/// the runtime, and its frame publishes the version the scene holds, with no
/// scene damage, for the raster side to take the frame with; the frame after
/// publishes nothing.
pub(crate) fn a_fed_frame_republishes_the_held_version() {
    let fixture = mounted(Placement::Small);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = taking_sink(&fixture, &seen, |_| crate::sink::SubmitVerdict::Presented);
    fixture.ui_runtime.render_frame(&mut sink);
    fixture.ui_runtime.render_frame(&mut sink);
    assert_eq!(
        seen.lock().len(),
        1,
        "nothing changed after the first frame"
    );
    fixture.ui_runtime.mark_rendered();

    feed(&fixture.shared, 9);
    assert!(
        fixture.ui_runtime.needs_redraw(),
        "a texture frame asks for a frame"
    );
    fixture.ui_runtime.render_frame(&mut sink);
    fixture.ui_runtime.render_frame(&mut sink);
    let seen = seen.lock();
    assert_eq!(seen.len(), 2, "the texture frame is published once");
    assert_eq!(
        seen[1],
        Seen {
            version: seen[0].version,
            damage: DamageRegion::Unchanged,
            frames: vec![(texture_id(&fixture), 9)],
        },
        "the held version, no scene damage, and the frame"
    );
}

/// A texture an ancestor hides while it stays mounted repaints nothing for
/// the frames its producer feeds: each goes out with the held version, which
/// draws no user of the texture.
pub(crate) fn a_hidden_texture_repaints_nothing() {
    let fixture = mounted(Placement::Small);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = taking_sink(&fixture, &seen, |_| crate::sink::SubmitVerdict::Presented);
    fixture.ui_runtime.render_frame(&mut sink);
    fixture
        .writer
        .write(|ctx| fixture.shown.set(ctx, 0))
        .expect("write");
    fixture.ui_runtime.render_frame(&mut sink);
    assert!(
        fixture.shared.lock().is_some(),
        "an offstage texture stays mounted"
    );
    let hidden = seen.lock().last().expect("the hiding frame").version;

    for shade in 1..=3 {
        feed(&fixture.shared, shade);
        fixture.ui_runtime.render_frame(&mut sink);
    }
    let seen = seen.lock();
    assert_eq!(seen.len(), 5, "each fed frame goes out: {seen:?}");
    assert!(
        seen[2..]
            .iter()
            .all(|frame| frame.version == hidden && frame.damage == DamageRegion::Unchanged),
        "a hidden texture's frames repaint nothing: {seen:?}"
    );
}

/// Unmounting the widget that owns the registration retires its texture.
pub(crate) fn unmounting_the_owner_retires_its_texture() {
    let fixture = mounted(Placement::Whole);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = {
        let seen = Arc::clone(&seen);
        ScriptedSink::new(move |_, publication| {
            seen.lock().push(publication.scene.id());
            crate::sink::SubmitVerdict::Presented
        })
    };
    fixture.ui_runtime.render_frame(&mut sink);
    let id = texture_id(&fixture);
    fixture
        .writer
        .write(|ctx| fixture.shown.set(ctx, 0))
        .expect("write");
    fixture.ui_runtime.render_frame(&mut sink);
    assert!(fixture.shared.lock().is_none(), "the owner was disposed");
    let registry = fixture
        .ui_runtime
        .texture_registry(fixture.ui_runtime.presentation_id())
        .expect("primary presentation");
    assert_eq!(registry.take_updates().retired, [id]);
}

/// A texture frame whose publication the surface withheld is published
/// again: the registry keeps it until a frame reaches the raster side, and
/// the retry takes it.
pub(crate) fn a_texture_frame_not_taken_is_published_again() {
    let fixture = mounted(Placement::Small);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = taking_sink(&fixture, &seen, |index| {
        if index == 1 {
            crate::sink::SubmitVerdict::NotShown
        } else {
            crate::sink::SubmitVerdict::Presented
        }
    });
    fixture.ui_runtime.render_frame(&mut sink);
    feed(&fixture.shared, 9);
    fixture.ui_runtime.render_frame(&mut sink);
    fixture.ui_runtime.render_frame(&mut sink);
    fixture.ui_runtime.render_frame(&mut sink);

    let seen = seen.lock();
    assert_eq!(
        seen.len(),
        3,
        "the refused frame is retried, once: {seen:?}"
    );
    assert_eq!(seen[1].version, seen[0].version);
    assert!(seen[1].frames.is_empty(), "the refused frame took nothing");
    assert_eq!(seen[2].version, seen[0].version, "the retry republishes");
    assert_eq!(
        seen[2].frames,
        [(texture_id(&fixture), 9)],
        "and the frame reaches the raster side"
    );
}

/// A frame the sink did not show leaves its damage owed: the retry is
/// published against the version still on screen and its damage covers the
/// box the unshown frame changed, though the retry itself changes nothing
/// more.
pub(crate) fn a_refused_submit_keeps_its_damage_owed() {
    let fixture = mounted(Placement::Small);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = {
        let seen = Arc::clone(&seen);
        ScriptedSink::new(move |index, publication| {
            seen.lock().push((publication.base, publication.damage));
            if index == 1 {
                crate::sink::SubmitVerdict::Retry
            } else {
                crate::sink::SubmitVerdict::Presented
            }
        })
    };
    fixture.ui_runtime.render_frame(&mut sink);
    fixture
        .writer
        .write(|ctx| fixture.shown.set(ctx, 0))
        .expect("write");
    fixture.ui_runtime.render_frame(&mut sink);
    fixture.ui_runtime.render_frame(&mut sink);

    let seen = seen.lock();
    assert_eq!(seen.len(), 3, "the refused frame is retried");
    assert!(
        seen[1].0.is_some(),
        "the change is published against the first"
    );
    assert_eq!(
        seen[2].0, seen[1].0,
        "the retry is published against the version still on screen"
    );
    let texture = DamageRect::try_covering(Rect::from_ltrb(100.0, 100.0, 120.0, 120.0), SURFACE)
        .expect("finite")
        .expect("the texture is on the surface");
    let DamageRegion::Region(region) = seen[2].1 else {
        panic!("the retry is drawn as a region: {:?}", seen[2].1);
    };
    assert!(
        region
            .rects()
            .iter()
            .any(|rect| rect.left() <= texture.left()
                && rect.top() <= texture.top()
                && rect.right() >= texture.right()
                && rect.bottom() >= texture.bottom()),
        "the retry still owes the box the unshown frame changed: {region:?}"
    );
}
