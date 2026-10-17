//! The overlay style a presentation's frames annotate at the window's top edge
//! reaches the window's system chrome once per change, only for frames the
//! sink accepted, and again after the native presentation detaches.

use parking_lot::Mutex;

use flui_platform_api::{PlatformSystemChrome, SystemUiOverlayStyle};
use flui_view::{Reactive, Signal};
use flui_widgets::AnnotatedRegion;

use super::*;

#[derive(Default)]
struct RecordingChrome(Mutex<Vec<SystemUiOverlayStyle>>);

impl PlatformSystemChrome for RecordingChrome {
    fn set_overlay_style(&self, style: SystemUiOverlayStyle) {
        self.0.lock().push(style);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The whole window annotated with the style `scene` selects: `1` is light,
/// anything else dark; the value also sizes the content, so equal styles can
/// still repaint.
#[derive(Clone)]
struct Styled {
    scene: Signal<u32>,
}

impl flui_view::View for Styled {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Styled {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let scene = self.scene.get(ctx);
        let style = if scene == 1 {
            SystemUiOverlayStyle::Light
        } else {
            SystemUiOverlayStyle::Dark
        };
        // As wide as the window allows, so it covers the top-edge probe.
        AnnotatedRegion::new(style, SizedBox::new(f64::INFINITY, 40.0 + f64::from(scene)))
    }
}

/// The presentation holds its window weakly, as a backend owns it; the test
/// keeps it alive in the fixture.
struct Fixture {
    ui_runtime: UiRuntime,
    chrome: Arc<RecordingChrome>,
    scene: Signal<u32>,
    graph: Reactive,
    window: Arc<dyn flui_platform_api::PlatformWindow>,
}

fn styled_runtime() -> Fixture {
    let chrome = Arc::new(RecordingChrome::default());
    let window: Arc<dyn flui_platform_api::PlatformWindow> = Arc::new(
        crate::testing::TestWindow::new()
            .focused(true)
            .with_system_chrome(Arc::clone(&chrome) as Arc<dyn PlatformSystemChrome>),
    );
    let ui_runtime = UiRuntime::for_test_with_window(Arc::clone(&window));
    let graph = ui_runtime
        .presentation_widgets_for_test(ui_runtime.presentation_id())
        .with_build_owner(|owner| owner.reactive().clone());
    let scene = graph.signal(0_u32);
    ui_runtime
        .attach_root_widget(&Styled { scene })
        .expect("root attaches");
    Fixture {
        ui_runtime,
        chrome,
        scene,
        graph,
        window,
    }
}

fn delivered(chrome: &RecordingChrome) -> Vec<SystemUiOverlayStyle> {
    chrome.0.lock().clone()
}

/// Each change is delivered once; repainting the same style is not.
pub(crate) fn each_style_change_is_delivered_once() {
    use SystemUiOverlayStyle::{Dark, Light};
    let Fixture {
        ui_runtime,
        chrome,
        scene,
        graph,
        window: _window,
    } = styled_runtime();
    let mut sink = ScriptedSink::always_presents();
    ui_runtime.render_frame(&mut sink);
    assert_eq!(delivered(&chrome), [Dark]);
    ui_runtime.render_frame(&mut sink);
    scene.set(&graph, 2).expect("write");
    ui_runtime.render_frame(&mut sink);
    assert_eq!(
        delivered(&chrome),
        [Dark],
        "an equal style repaints silently"
    );
    scene.set(&graph, 1).expect("write");
    ui_runtime.render_frame(&mut sink);
    assert_eq!(delivered(&chrome), [Dark, Light]);
}

/// A frame the sink did not accept announces nothing; the accepted retry does.
pub(crate) fn a_refused_frame_announces_nothing() {
    let Fixture {
        ui_runtime,
        chrome,
        window: _window,
        ..
    } = styled_runtime();
    let mut sink = ScriptedSink::fails_once_then_presents(crate::sink::SubmitVerdict::Retry);
    ui_runtime.render_frame(&mut sink);
    assert!(
        delivered(&chrome).is_empty(),
        "the refused frame was never shown"
    );
    ui_runtime.render_frame(&mut sink);
    assert_eq!(delivered(&chrome), [SystemUiOverlayStyle::Dark]);
}

/// Detaching the native presentation forgets the last delivery, so the next
/// frame resends even an unchanged style.
pub(crate) fn a_detached_presentation_resends_its_style() {
    use flui_platform_api::WindowExecutionState::{Detached, Running};
    let Fixture {
        ui_runtime,
        chrome,
        scene,
        graph,
        window: _window,
    } = styled_runtime();
    let mut sink = ScriptedSink::always_presents();
    ui_runtime.render_frame(&mut sink);
    let id = ui_runtime.presentation_id();
    ui_runtime.synchronize_window_snapshot(id, Detached, true, true);
    ui_runtime.synchronize_window_snapshot(id, Running, true, true);
    scene.set(&graph, 2).expect("write");
    ui_runtime.render_frame(&mut sink);
    assert_eq!(
        delivered(&chrome),
        [SystemUiOverlayStyle::Dark, SystemUiOverlayStyle::Dark]
    );
}
