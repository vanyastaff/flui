// The application, and the entry points the platforms call.
//
// `src/main.rs` mounts `CounterApp` on the desktop; `android_main` below is
// what Android's `NativeActivity` calls once it has loaded this crate as a
// `cdylib` (`flui build android`).

use flui::prelude::*;
use flui::widgets::{SafeArea, column};
#[cfg(target_os = "android")]
use flui::native::android::{AndroidApp, run_app_android};

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    // `run_app_android` logs why it could not start.
    let _started = run_app_android(app, CounterApp, flui::AppLaunch::new());
}

/// The counter application mounted by desktop and Android entry points.
#[derive(Clone, Debug, StatelessView)]
pub struct CounterApp;

impl StatelessView for CounterApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SafeArea::new().child(CounterView)
    }
}

#[derive(Clone, StatefulView)]
struct CounterView;

#[derive(Default)]
struct CounterState {
    count: Signal<usize>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState::default()
    }
}

impl ViewState<CounterView> for CounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count = ctx.signal(0);
    }

    fn build(&self, _view: &CounterView, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;

        Center::new().child(
            Column::new(column![
                Text::new("You have pushed the button this many times:"),
                SizedBox::height(16.0),
                Text::new(count.get(ctx).to_string()),
                SizedBox::height(16.0),
                RawButton::new(Text::new("Increment"))
                    .on_press(move |cx| count.update(cx, |n| *n += 1)),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui::geometry::Size;
    use flui::testing::widgets::{lay_out, tight};

    /// The tree the application is mounted as: the app under the root
    /// `MediaQuery` the app runner attaches it to.
    ///
    /// The runner supplies this root (`flui-app`'s `MediaQueryRoot`), so
    /// `main` does not — but a test that mounts the app directly has to, or it
    /// is not mounting the tree the application runs as. `SafeArea` reads its
    /// insets from that root and panics without one:
    ///
    /// ```text
    /// MediaQuery::of called with no MediaQuery ancestor in the tree
    /// ```
    ///
    /// The data describes the surface the constraints describe, with no
    /// insets, so the geometry assertions below are exact.
    fn app_tree(width: f64, height: f64) -> MediaQuery {
        MediaQuery::new(
            MediaQueryData {
                size: Size::new(width, height),
                ..MediaQueryData::default()
            },
            CounterApp,
        )
    }

    #[test]
    fn counter_responds_to_pointer_input() {
        let mut app = lay_out(app_tree(480.0, 320.0), tight(480.0, 320.0));
        assert!(app.find_text("0").is_some());
        for (previous, next) in [("0", "1"), ("1", "2")] {
            let label = app.find_text("Increment").expect("increment button label");
            let offset = app.absolute_offset(label);
            let size = app.size(label);
            let x = offset.dx + size.width / 2.0;
            let y = offset.dy + size.height / 2.0;
            app.dispatch_pointer_down(x, y);
            app.dispatch_pointer_up(x, y);
            app.tick();
            assert!(
                app.find_text(next).is_some(),
                "counter should display {next}"
            );
            assert!(app.find_text(previous).is_none());
        }
        app.pump_widget(app_tree(480.0, 320.0));
        assert!(
            app.find_text("2").is_some(),
            "state survives a parent rebuild"
        );
        let independent = lay_out(app_tree(480.0, 320.0), tight(480.0, 320.0));
        assert!(
            independent.find_text("0").is_some(),
            "each app owns its state"
        );
    }

    /// The counter is centred on the surface, not merely wrapped in a
    /// `Center`.
    ///
    /// A `Column` fills the height it is given (`MainAxisSize::Max` is the
    /// default) and packs its children at the top (`MainAxisAlignment::Start`),
    /// so a `Center` around it has no slack to centre anything in: the wrap
    /// looks like the centreing and is not. This asserts the geometry the wrap
    /// is meant to produce — the content block's vertical centre lands on the
    /// surface's, within a pixel of rounding.
    #[test]
    fn counter_content_is_centred() {
        const WIDTH: f64 = 480.0;
        const HEIGHT: f64 = 320.0;
        let app = lay_out(app_tree(WIDTH, HEIGHT), tight(WIDTH, HEIGHT));

        let top = app
            .find_text("You have pushed the button this many times:")
            .expect("prompt text");
        let bottom = app.find_text("Increment").expect("increment button label");
        let top_offset = app.absolute_offset(top).dy;
        let bottom_edge = app.absolute_offset(bottom).dy + app.size(bottom).height;

        let content_centre = f64::midpoint(top_offset, bottom_edge);
        assert!(
            (content_centre - HEIGHT / 2.0).abs() <= 1.0,
            "content spans y {top_offset}..{bottom_edge} of a {HEIGHT}pt surface, so its centre is \
             {content_centre} rather than {}; a `Column` fills the height and packs its children at \
             the top unless it is told to centre them (`MainAxisAlignment::Center`)",
            HEIGHT / 2.0
        );
    }
}
