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
    /// A 20-px surface beside a leaf whose layout waits for input at any
    /// surface width but the test window's.
    Gated,
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
            Placement::Gated => flui_widgets::Column::new((
                WidthGate,
                SizedBox::new(20.0, 20.0).child(self.surface.clone()),
            ))
            .boxed(),
        }
    }
}

/// A 10-px leaf whose layout fails as text whose context is busy, so the
/// pipeline waits for input, at any width but the test window's.
#[derive(Debug)]
struct WidthGateBox;

impl flui_foundation::inspect::Inspect for WidthGateBox {
    fn kind(&self) -> &'static str {
        "WidthGateBox"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::prelude::RenderBox for WidthGateBox {
    type Arity = flui_rendering::prelude::Leaf;
    type ParentData = flui_rendering::prelude::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::prelude::BoxLayoutContext<
            '_,
            flui_rendering::prelude::Leaf,
            flui_rendering::prelude::BoxParentData,
        >,
    ) -> flui_rendering::RenderResult<flui_foundation::geometry::Size> {
        #[expect(
            clippy::float_cmp,
            reason = "the window's width is an exact logical extent at scale 1"
        )]
        if ctx.constraints().max_width != f64::from(SURFACE.0) {
            return Err(flui_rendering::RenderError::TextContextBusy);
        }
        Ok(flui_foundation::geometry::Size::new(10.0, 10.0))
    }

    fn paint(
        &self,
        _ctx: &mut flui_rendering::prelude::PaintContext<'_, flui_rendering::prelude::Leaf>,
    ) {
    }
}

#[derive(Clone, Copy)]
struct WidthGate;

impl flui_view::RenderView for WidthGate {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = WidthGateBox;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> WidthGateBox {
        WidthGateBox
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut WidthGateBox,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl flui_view::View for WidthGate {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
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

/// A texture an ancestor hides while it stays mounted costs nothing for the
/// frames its producer feeds: the held version draws no user of it, so no
/// frame is published and none stays owed; the newest frame waits in the
/// registry for the next publication.
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

    for shade in 1..=3 {
        feed(&fixture.shared, shade);
        fixture.ui_runtime.render_frame(&mut sink);
        assert!(
            !fixture.ui_runtime.has_pending_work(),
            "a hidden texture's frame owes no frame"
        );
    }
    let seen = seen.lock().clone();
    assert_eq!(seen.len(), 2, "nothing is published: {seen:?}");
    let registry = fixture
        .ui_runtime
        .texture_registry(fixture.ui_runtime.presentation_id())
        .expect("primary presentation");
    let frames = registry.take_updates().frames;
    assert_eq!(frames.len(), 1, "the newest frame waits, one per texture");
    assert_eq!(frames[0].1.data()[0], 3);
}

/// A frame fed while the previous one is submitted, after the raster side
/// took the updates, outlives the redraw mark the frame's end clears: the
/// runner's gate still sees it, and its next frame publishes it.
pub(crate) fn a_frame_fed_during_a_submit_is_still_owed() {
    let fixture = mounted(Placement::Small);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = {
        let registry = fixture
            .ui_runtime
            .texture_registry(fixture.ui_runtime.presentation_id())
            .expect("primary presentation");
        let (seen, shared) = (Arc::clone(&seen), Arc::clone(&fixture.shared));
        ScriptedSink::new(move |index, publication| {
            let frames = registry
                .take_updates()
                .frames
                .iter()
                .map(|(id, image)| (*id, image.data()[0]))
                .collect();
            seen.lock().push(Seen {
                version: publication.scene.id(),
                damage: publication.damage,
                frames,
            });
            if index == 1 {
                // The producer's next frame lands while this one renders.
                feed(&shared, 10);
            }
            crate::sink::SubmitVerdict::Presented
        })
    };
    fixture.ui_runtime.render_frame(&mut sink);
    feed(&fixture.shared, 9);
    fixture.ui_runtime.render_frame(&mut sink);
    assert!(
        !fixture.ui_runtime.needs_redraw(),
        "the frame's end cleared the wake the feed raised"
    );
    assert!(
        fixture.ui_runtime.has_pending_work(),
        "the runner's gate still sees the texture frame"
    );
    fixture.ui_runtime.render_frame(&mut sink);
    let seen = seen.lock();
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert_eq!(seen[2].version, seen[0].version, "the held version again");
    assert_eq!(seen[2].frames, [(texture_id(&fixture), 10)]);
    assert!(!fixture.ui_runtime.has_pending_work(), "and nothing more");
}

/// A window that refuses the version costs no frame per texture frame
/// (ADR-0334 §4): the refused version is not published again for one, the
/// runner's gate sees nothing owed, and the newest frame waits, one per
/// texture, for the frame that applies a version.
pub(crate) fn a_refused_window_costs_no_texture_frame() {
    let fixture = mounted(Placement::Small);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = taking_sink(&fixture, &seen, |_| crate::sink::SubmitVerdict::Refused);
    fixture.ui_runtime.render_frame(&mut sink);
    for shade in 1..=3 {
        feed(&fixture.shared, shade);
        fixture.ui_runtime.render_frame(&mut sink);
        assert!(
            !fixture.ui_runtime.has_pending_work(),
            "a refused window owes no texture frame"
        );
    }
    let seen = seen.lock().clone();
    assert_eq!(seen.len(), 1, "only the refused frame: {seen:?}");
    let registry = fixture
        .ui_runtime
        .texture_registry(fixture.ui_runtime.presentation_id())
        .expect("primary presentation");
    let frames = registry.take_updates().frames;
    assert_eq!(frames.len(), 1, "the newest frame waits, one per texture");
    assert_eq!(frames[0].1.data()[0], 3);
}

/// A window that shows the version it refused takes texture frames again:
/// of two republishes of one version in flight, the first is refused and the
/// second shown, and the next texture frame is published with that version.
pub(crate) fn a_shown_refused_version_takes_texture_frames_again() {
    use crate::sink::{FrameSettlement, SubmitVerdict};

    let mut fixture = mounted(Placement::Small);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = taking_sink(&fixture, &seen, |index| match index {
        1 | 2 => SubmitVerdict::Queued,
        _ => SubmitVerdict::Presented,
    })
    .holding(std::num::NonZeroUsize::new(2).expect("two"));
    fixture.ui_runtime.render_frame(&mut sink);
    for shade in [1, 2] {
        feed(&fixture.shared, shade);
        fixture.ui_runtime.render_frame(&mut sink);
    }
    let [_, refused, shown] = sink.epochs[..] else {
        panic!("the first frame and two republishes, got {:?}", sink.epochs);
    };
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: fixture.ui_runtime.id(),
        presentation_id: fixture.ui_runtime.presentation_id(),
    };
    for (epoch, verdict) in [
        (refused, SubmitVerdict::Refused),
        (shown, SubmitVerdict::Presented),
    ] {
        let _ = fixture
            .ui_runtime
            .settle_frame(FrameSettlement::new(address, epoch, verdict));
    }
    feed(&fixture.shared, 3);
    fixture.ui_runtime.render_frame(&mut sink);

    let seen = seen.lock().clone();
    assert_eq!(
        seen.len(),
        4,
        "the shown version takes the next texture frame: {seen:?}"
    );
    assert!(
        seen.iter().all(|frame| frame.version == seen[0].version),
        "{seen:?}"
    );
    assert_eq!(seen[3].frames, [(texture_id(&fixture), 3)]);
}

/// A texture frame fed while the window's new size cannot be laid out (the
/// layout waits for input that has not come) owes no frame: the held version
/// was applied for the old size, and the repaint the new size asks for is
/// what carries the frame. The runner's gate sees nothing to do, rather
/// than a no-present frame every period.
pub(crate) fn a_texture_frame_while_a_new_size_waits_owes_nothing() {
    let fixture = mounted(Placement::Gated);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut sink = taking_sink(&fixture, &seen, |_| crate::sink::SubmitVerdict::Presented);
    fixture.ui_runtime.render_frame(&mut sink);
    assert_eq!(seen.lock().len(), 1, "the first frame is shown");

    let mut resized = taking_sink(&fixture, &seen, |_| crate::sink::SubmitVerdict::Presented)
        .with_size(SURFACE.0 - 100, SURFACE.1);
    feed(&fixture.shared, 9);
    fixture.ui_runtime.render_frame(&mut resized);
    assert!(
        fixture
            .ui_runtime
            .presentations
            .primary()
            .pipeline()
            .with(flui_rendering::pipeline::PipelineOwner::layout_waits_for_input),
        "the new size's layout waits for input"
    );
    assert!(
        !fixture.ui_runtime.has_pending_work(),
        "the texture frame owes no frame under the new size"
    );
    let seen = seen.lock().clone();
    assert_eq!(seen.len(), 1, "nothing more is published: {seen:?}");
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
