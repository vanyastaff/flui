//! Interactive motion lab, using the same widgets as application code.
//!
//! Run `cargo run --example motion_lab --features material -- --full`, or
//! replace `--full` with `--reduce` / `--system`. The default follows the host.
//! With `--system` on Windows, toggle Settings > Accessibility > Visual effects
//! > Animation effects while a transition is running.
//!
//! Change width, then height before width finishes: the first property's
//! deadline survives the second change. Reverse a target repeatedly to inspect
//! continuity. Under Reduce, property transitions settle on the next frame,
//! while the activity indicator and the three-second snack-bar timer continue.
//! Drag the card, or open the drawer and release it halfway through a drag.

use std::time::Duration;

use flui::animation::MotionPreference;
use flui::foundation::geometry::Angle;
use flui::material::ScaffoldScope;
use flui::painting::styling::Color;
use flui::prelude::*;
use flui::widgets::{
    ActivityIndicator, AnimatedContainer, AnimatedOpacity, AnimatedRotation, Dismissible, column,
    row,
};

const BLUE: Color = Color::rgb(35, 94, 180);
const ORANGE: Color = Color::rgb(216, 102, 35);
const LANE: Color = Color::rgb(235, 240, 247);
const TRANSITION: Duration = Duration::from_millis(1600);

#[derive(Clone, StatelessView)]
pub(crate) struct MotionLab;

impl StatelessView for MotionLab {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(
            ThemeData::light(),
            ScaffoldMessenger::new(
                Scaffold::new()
                    .app_bar(AppBar::new().title(Text::new("Motion lab")))
                    .drawer(
                        Drawer::new().child(Padding::all(24.0).child(Column::new(column![
                            SizedBox::height(40.0),
                            Text::new("Drag this panel sideways, then release."),
                            SizedBox::height(16.0),
                            Text::new("Tap the shaded area to close it."),
                        ]))),
                    )
                    .body(LabControls),
            ),
        )
    }
}

#[derive(Clone, StatefulView)]
struct LabControls;

#[derive(Default)]
struct LabState {
    wide: Signal<bool>,
    tall: Signal<bool>,
    faded: Signal<bool>,
    turned: Signal<bool>,
    dismissed: Signal<bool>,
}

impl StatefulView for LabControls {
    type State = LabState;

    fn create_state(&self) -> Self::State {
        LabState::default()
    }
}

impl ViewState<LabControls> for LabState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.wide = ctx.signal(false);
        self.tall = ctx.signal(false);
        self.faded = ctx.signal(false);
        self.turned = ctx.signal(false);
        self.dismissed = ctx.signal(false);
    }

    fn build(&self, _view: &LabControls, ctx: &dyn BuildContext) -> impl IntoView {
        let wide = self.wide;
        let tall = self.tall;
        let faded = self.faded;
        let turned = self.turned;
        let dismissed = self.dismissed;
        let drawer = ScaffoldScope::of(ctx);
        let messenger = ScaffoldMessengerScope::of(ctx);

        let card = if dismissed.get(ctx) {
            SizedBox::new(420.0, 52.0)
                .child(Center::new().child(Text::new("Card dismissed. Press Restore card.")))
                .boxed()
        } else {
            Dismissible::new(
                SizedBox::new(420.0, 52.0).child(
                    ColoredBox::new(BLUE)
                        .child(Center::new().child(Text::new("Swipe this card sideways"))),
                ),
            )
            .background(ColoredBox::new(ORANGE))
            .on_dismissed(move |cx, _| dismissed.set(cx, true))
            .boxed()
        };

        Padding::all(24.0).child(Column::new(column![
            Text::new(format!(
                "Effective motion: {:?}",
                MediaQuery::motion_of(ctx)
            )),
            SizedBox::height(12.0),
            Text::new("Change width, then height. Reverse either before it finishes."),
            SizedBox::height(8.0),
            Row::new(row![
                FilledButton::new(Text::new("Change width"))
                    .on_pressed(move |cx| wide.update(cx, |value| *value = !*value)),
                SizedBox::width(12.0),
                FilledButton::new(Text::new("Change height"))
                    .on_pressed(move |cx| tall.update(cx, |value| *value = !*value)),
            ]),
            SizedBox::height(8.0),
            SizedBox::new(420.0, 150.0).child(
                ColoredBox::new(LANE).child(
                    Center::new().child(
                        AnimatedContainer::new(SizedBox::shrink())
                            .width(if wide.get(ctx) { 360.0 } else { 90.0 })
                            .height(if tall.get(ctx) { 120.0 } else { 40.0 })
                            .color(BLUE)
                            .duration(TRANSITION),
                    ),
                )
            ),
            SizedBox::height(16.0),
            Row::new(row![
                FilledButton::new(Text::new("Fade"))
                    .on_pressed(move |cx| faded.update(cx, |value| *value = !*value)),
                SizedBox::width(12.0),
                FilledButton::new(Text::new("Rotate"))
                    .on_pressed(move |cx| turned.update(cx, |value| *value = !*value)),
                SizedBox::width(24.0),
                SizedBox::new(100.0, 100.0).child(
                    Center::new().child(
                        AnimatedOpacity::new(
                            if faded.get(ctx) { 0.15 } else { 1.0 },
                            AnimatedRotation::new(
                                Angle::from_degrees(if turned.get(ctx) { 270.0 } else { 0.0 }),
                                SizedBox::new(64.0, 32.0).child(ColoredBox::new(ORANGE)),
                            )
                            .duration(TRANSITION),
                        )
                        .duration(TRANSITION),
                    )
                ),
            ]),
            SizedBox::height(12.0),
            Column::new(column![
                card,
                SizedBox::height(8.0),
                Row::new(row![
                    TextButton::new(Text::new("Restore card"))
                        .on_pressed(move |cx| dismissed.set(cx, false)),
                    SizedBox::width(12.0),
                    FilledButton::new(Text::new("Open drawer"))
                        .on_pressed(move |_| drawer.open_drawer()),
                ]),
                SizedBox::height(16.0),
                Row::new(row![
                    SizedBox::new(36.0, 36.0).child(ActivityIndicator::new()),
                    SizedBox::width(16.0),
                    Text::new("Activity keeps running under Reduce"),
                ]),
                SizedBox::height(12.0),
                FilledButton::new(Text::new("Show message for 3 seconds")).on_pressed(move |_| {
                    let _ = messenger.show_snack_bar(
                        SnackBar::new(Text::new("This timer keeps its duration under Reduce"))
                            .duration(Duration::from_secs(3)),
                    );
                }),
            ]),
        ]))
    }
}

fn main() {
    let preference = match std::env::args().nth(1).as_deref() {
        None | Some("--system") => MotionPreference::FollowSystem,
        Some("--full") => MotionPreference::Full,
        Some("--reduce") => MotionPreference::Reduce,
        Some(_) => {
            eprintln!("Usage: motion_lab [--system|--full|--reduce]");
            std::process::exit(2);
        }
    };
    run_app_with_config(
        MotionLab,
        AppConfig::default()
            .with_title("FLUI motion lab")
            .with_size(720, 760)
            .with_min_size(640, 720)
            .with_motion_preference(preference),
    );
}
