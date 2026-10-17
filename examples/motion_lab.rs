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
//! while loading stays visible without motion and the three-second snack-bar
//! timer keeps its duration. Pause activity also stops the indicator while
//! leaving the other controls usable; Resume activity respects the preference.
//! Drag the card, or open the drawer and release it halfway through a drag.
//!
//! Catalog scenes pushes a page, with the theme's page transition, holding
//! the implicit widgets (scale, slide, position, cross fade), an effects chain
//! to replay, an animated list (add, remove, reverse; remove an item and undo
//! it before its exit ends), shimmering skeletons, a picker for the theme's
//! page transition with a push to try it, and an `OpenContainer` card that
//! grows into its page and shrinks back.

use std::time::Duration;

use flui::animation::MotionOverride;
use flui::foundation::geometry::Angle;
use flui::material::{
    OpenContainer, PageTransition, PageTransitionsTheme, ScaffoldScope, SharedAxis,
};
use flui::painting::styling::Color;
use flui::prelude::*;
use flui::widgets::{
    ActivityIndicator, AnimateExt, AnimatedContainer, AnimatedCrossFade, AnimatedList,
    AnimatedOpacity, AnimatedPositioned, AnimatedRotation, AnimatedScale, AnimatedSlide,
    CrossFadeState, Dismissible, Effects, NavigatorHandle, PageRoute, Shimmer,
    SingleChildScrollView, Skeleton, Stack, TickerMode, Transition, TranslationFraction, Trigger,
    column, row,
};

const BLUE: Color = Color::rgb(35, 94, 180);
const ORANGE: Color = Color::rgb(216, 102, 35);
const LANE: Color = Color::rgb(235, 240, 247);
const TRANSITION: Duration = Duration::from_millis(1600);

/// The page transitions the lab can install, by name.
const PAGE_TRANSITIONS: [&str; 6] = [
    "Platform default",
    "Fade forwards",
    "Zoom",
    "Cupertino",
    "Shared axis",
    "Fade through",
];

fn page_transition(choice: usize) -> Option<PageTransition> {
    match choice {
        1 => Some(PageTransition::FadeForwards),
        2 => Some(PageTransition::Zoom),
        3 => Some(PageTransition::Cupertino),
        4 => Some(PageTransition::SharedAxis(SharedAxis::Horizontal)),
        5 => Some(PageTransition::FadeThrough),
        _ => None,
    }
}

#[derive(Clone, StatelessView)]
pub(crate) struct MotionLab;

impl StatelessView for MotionLab {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        LabApp
    }
}

/// A `MaterialApp` whose theme carries the chosen page transition, so a push
/// anywhere below picks it up.
#[derive(Clone, StatefulView)]
struct LabApp;

#[derive(Default)]
struct LabAppState {
    page_transition: Signal<usize>,
}

impl StatefulView for LabApp {
    type State = LabAppState;

    fn create_state(&self) -> Self::State {
        LabAppState::default()
    }
}

impl ViewState<LabApp> for LabAppState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.page_transition = ctx.signal(0);
    }

    fn build(&self, _view: &LabApp, ctx: &dyn BuildContext) -> impl IntoView {
        let mut theme = ThemeData::light();
        if let Some(transition) = page_transition(self.page_transition.get(ctx)) {
            theme.page_transitions_theme =
                PageTransitionsTheme::default().with(theme.platform, transition);
        }
        MaterialApp::new(
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
                .body(LabControls {
                    page_transition: self.page_transition,
                }),
        )
        .theme(theme)
    }
}

#[derive(Clone, StatefulView)]
struct LabControls {
    page_transition: Signal<usize>,
}

#[derive(Default)]
struct LabState {
    wide: Signal<bool>,
    tall: Signal<bool>,
    faded: Signal<bool>,
    turned: Signal<bool>,
    dismissed: Signal<bool>,
    activity_paused: Signal<bool>,
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
        self.activity_paused = ctx.signal(false);
    }

    fn build(&self, view: &LabControls, ctx: &dyn BuildContext) -> impl IntoView {
        let navigator = NavigatorHandle::maybe_of(ctx);
        let page_transition = view.page_transition;
        let catalog = FilledButton::new(Text::new("Catalog scenes")).on_pressed(move |_| {
            if let Some(navigator) = &navigator {
                navigator.push(PageRoute::<()>::new(move |_ctx, _primary, _secondary| {
                    CatalogScenes { page_transition }.boxed()
                }));
            }
        });
        Padding::all(24.0).child(self.property_motion(ctx, catalog.boxed()))
    }
}

impl LabState {
    /// Independent property motion, gestures, activity and a timed snack bar.
    fn property_motion(&self, ctx: &dyn BuildContext, catalog: BoxedView) -> BoxedView {
        let wide = self.wide;
        let tall = self.tall;
        let faded = self.faded;
        let turned = self.turned;
        let dismissed = self.dismissed;
        let activity_paused = self.activity_paused;
        let paused = activity_paused.get(ctx);
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

        Column::new(column![
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
                    SizedBox::width(12.0),
                    catalog,
                ]),
                SizedBox::height(16.0),
                Row::new(row![
                    TickerMode::new(SizedBox::new(36.0, 36.0).child(ActivityIndicator::new()))
                        .enabled(!paused),
                    SizedBox::width(16.0),
                    Text::new(if paused {
                        "Activity paused"
                    } else {
                        "Activity follows motion preference"
                    }),
                    SizedBox::width(12.0),
                    FilledButton::new(Text::new(if paused {
                        "Resume activity"
                    } else {
                        "Pause activity"
                    }))
                    .on_pressed(move |cx| activity_paused.update(cx, |value| *value = !*value)),
                ]),
                SizedBox::height(12.0),
                FilledButton::new(Text::new("Show message for 3 seconds")).on_pressed(move |_| {
                    let _ = messenger.show_snack_bar(
                        SnackBar::new(Text::new("This timer keeps its duration under Reduce"))
                            .duration(Duration::from_secs(3)),
                    );
                }),
            ]),
        ])
        .boxed()
    }
}

/// The catalog's scenes on a page of their own, pushed with the theme's
/// page transition.
#[derive(Clone, StatefulView)]
struct CatalogScenes {
    page_transition: Signal<usize>,
}

#[derive(Default)]
struct Scenes {
    shifted: Signal<bool>,
    crossed: Signal<bool>,
    replays: Signal<u64>,
    items: Signal<Vec<u32>>,
    next_item: Signal<u32>,
    removed: Signal<Option<u32>>,
    loading: Signal<bool>,
}

impl StatefulView for CatalogScenes {
    type State = Scenes;

    fn create_state(&self) -> Self::State {
        Scenes::default()
    }
}

impl ViewState<CatalogScenes> for Scenes {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.shifted = ctx.signal(false);
        self.crossed = ctx.signal(false);
        self.replays = ctx.signal(0);
        self.items = ctx.signal(vec![1, 2, 3]);
        self.next_item = ctx.signal(4);
        self.removed = ctx.signal(None);
        self.loading = ctx.signal(true);
    }

    fn build(&self, view: &CatalogScenes, ctx: &dyn BuildContext) -> impl IntoView {
        let scenes = self;
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new("Catalog scenes")))
            .body(
                SingleChildScrollView::new().child(Padding::all(24.0).child(Column::new(column![
                    scenes.implicit_widgets(ctx),
                    SizedBox::height(24.0),
                    scenes.effects_chain(ctx),
                    SizedBox::height(24.0),
                    scenes.animated_list(ctx),
                    SizedBox::height(24.0),
                    scenes.skeletons(ctx),
                    SizedBox::height(24.0),
                    page_transitions(view.page_transition, ctx),
                    SizedBox::height(24.0),
                    open_container(),
                ]))),
            )
    }
}

impl Scenes {
    /// Scale, slide, position and cross fade, each on its default motion.
    fn implicit_widgets(&self, ctx: &dyn BuildContext) -> BoxedView {
        let shifted = self.shifted;
        let crossed = self.crossed;
        let on = shifted.get(ctx);
        let swatch = || SizedBox::new(48.0, 48.0).child(ColoredBox::new(ORANGE));

        Column::new(column![
            Text::new("Implicit widgets: press Shift, then press it again mid-flight."),
            SizedBox::height(8.0),
            Row::new(row![
                FilledButton::new(Text::new("Shift"))
                    .on_pressed(move |cx| shifted.update(cx, |value| *value = !*value)),
                SizedBox::width(12.0),
                FilledButton::new(Text::new("Cross fade"))
                    .on_pressed(move |cx| crossed.update(cx, |value| *value = !*value)),
            ]),
            SizedBox::height(8.0),
            Row::new(row![
                SizedBox::new(96.0, 96.0).child(
                    Center::new().child(AnimatedScale::new(if on { 1.6 } else { 1.0 }, swatch()))
                ),
                SizedBox::new(96.0, 96.0).child(Center::new().child(AnimatedSlide::new(
                    TranslationFraction::new(0.0, if on { 0.5 } else { -0.5 }),
                    swatch(),
                ))),
                SizedBox::new(140.0, 96.0).child(ColoredBox::new(LANE).child(Stack::new(row![
                    AnimatedPositioned::new(swatch())
                        .left(if on { 84.0 } else { 8.0 })
                        .top(if on { 40.0 } else { 8.0 })
                        .duration(TRANSITION),
                ]))),
                SizedBox::width(16.0),
                AnimatedCrossFade::new(
                    SizedBox::new(96.0, 48.0).child(ColoredBox::new(BLUE)),
                    SizedBox::new(48.0, 96.0).child(ColoredBox::new(ORANGE)),
                    if crossed.get(ctx) {
                        CrossFadeState::ShowSecond
                    } else {
                        CrossFadeState::ShowFirst
                    },
                ),
            ]),
        ])
        .boxed()
    }

    /// A timeline that fades and slides in, then scales: replayed on demand.
    fn effects_chain(&self, ctx: &dyn BuildContext) -> BoxedView {
        let replays = self.replays;
        Column::new(column![
            Text::new("Effects chain: fade and slide in, then scale."),
            SizedBox::height(8.0),
            Row::new(row![
                FilledButton::new(Text::new("Replay"))
                    .on_pressed(move |cx| replays.update(cx, |count| *count += 1)),
                SizedBox::width(24.0),
                SizedBox::new(160.0, 48.0)
                    .child(ColoredBox::new(BLUE).child(Center::new().child(Text::new("Hello"))))
                    .animate(
                        Effects::new()
                            .fade_in()
                            .slide_y(0.5)
                            .then()
                            .scale(0.8)
                            .duration(Duration::from_millis(400)),
                    )
                    .trigger(Trigger::OnChange(replays.get(ctx))),
            ]),
        ])
        .boxed()
    }

    /// Items enter and leave in place; the rest slide to their new places.
    fn animated_list(&self, ctx: &dyn BuildContext) -> BoxedView {
        let items_signal = self.items;
        let next_item = self.next_item;
        let removed = self.removed;
        let items = items_signal.with(ctx, Vec::clone);
        Column::new(column![
            Text::new("Animated list: remove an item, then add it back before it is gone."),
            SizedBox::height(8.0),
            Row::new(row![
                FilledButton::new(Text::new("Add")).on_pressed(move |cx| {
                    let mut id = 0;
                    let _ = next_item.update(cx, |next| {
                        id = *next;
                        *next += 1;
                    });
                    items_signal.update(cx, |list| list.insert(0, id))
                }),
                SizedBox::width(12.0),
                FilledButton::new(Text::new("Remove first")).on_pressed(move |cx| {
                    let mut first = None;
                    let _ = items_signal.update(cx, |list| {
                        if !list.is_empty() {
                            first = Some(list.remove(0));
                        }
                    });
                    removed.set(cx, first)
                }),
                SizedBox::width(12.0),
                // The same key comes back, so a row still leaving turns around.
                FilledButton::new(Text::new("Undo remove")).on_pressed(move |cx| {
                    let mut restored = None;
                    let _ = removed.update(cx, |last| restored = last.take());
                    items_signal.update(cx, |list| {
                        if let Some(id) = restored {
                            list.insert(0, id);
                        }
                    })
                }),
                SizedBox::width(12.0),
                FilledButton::new(Text::new("Reverse"))
                    .on_pressed(move |cx| items_signal.update(cx, |list| list.reverse())),
            ]),
            SizedBox::height(8.0),
            SizedBox::new(420.0, 220.0).child(
                ColoredBox::new(LANE).child(
                    AnimatedList::new(
                        items,
                        |id| *id,
                        |id| {
                            Padding::all(4.0).child(
                                SizedBox::new(400.0, 32.0).child(
                                    ColoredBox::new(BLUE).child(
                                        Center::new().child(Text::new(format!("Item {id}"))),
                                    ),
                                ),
                            )
                        },
                    )
                    .transition(
                        Transition::fade()
                            + Transition::slide_in(TranslationFraction::new(-0.3, 0.0)),
                    ),
                )
            ),
        ])
        .boxed()
    }

    /// Placeholders sharing one shimmer band, swapped for content.
    fn skeletons(&self, ctx: &dyn BuildContext) -> BoxedView {
        let loading = self.loading;
        let content = if loading.get(ctx) {
            Shimmer::new(Row::new(row![
                Skeleton::circle(48.0),
                SizedBox::width(16.0),
                SizedBox::width(300.0).child(Skeleton::text(3)),
            ]))
            .label("Loading profile")
            .boxed()
        } else {
            Row::new(row![
                SizedBox::new(48.0, 48.0).child(ColoredBox::new(ORANGE)),
                SizedBox::width(16.0),
                Text::new("Profile loaded."),
            ])
            .boxed()
        };
        Column::new(column![
            Text::new("Skeletons: one shimmer band across every placeholder."),
            SizedBox::height(8.0),
            FilledButton::new(Text::new(if loading.get(ctx) {
                "Finish loading"
            } else {
                "Load again"
            }))
            .on_pressed(move |cx| loading.update(cx, |value| *value = !*value)),
            SizedBox::height(8.0),
            content,
        ])
        .boxed()
    }
}

/// Pick the theme's page transition, then push a page that adopts it.
fn page_transitions(choice: Signal<usize>, ctx: &dyn BuildContext) -> BoxedView {
    let navigator = NavigatorHandle::maybe_of(ctx);
    let selected = choice.get(ctx);
    let choices: Vec<BoxedView> = PAGE_TRANSITIONS
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let label = Text::new(if index == selected {
                format!("[{name}]")
            } else {
                (*name).to_owned()
            });
            TextButton::new(label)
                .on_pressed(move |cx| choice.set(cx, index))
                .boxed()
        })
        .collect();
    Column::new(column![
        Text::new("Page transitions: pick one, then push a page."),
        SizedBox::height(8.0),
        Row::new(choices),
        SizedBox::height(8.0),
        FilledButton::new(Text::new("Push a page")).on_pressed(move |_| {
            if let Some(navigator) = &navigator {
                navigator.push(PageRoute::<()>::new(|_ctx, _primary, _secondary| {
                    Scaffold::new()
                        .app_bar(AppBar::new().title(Text::new("Pushed page")))
                        .body(Center::new().child(Text::new("Use the back button to return.")))
                        .boxed()
                }));
            }
        }),
    ])
    .boxed()
}

/// A card that grows into its page and shrinks back to where it is now.
fn open_container() -> BoxedView {
    Column::new(column![
        Text::new("OpenContainer: tap the card, then close the page."),
        SizedBox::height(8.0),
        SizedBox::new(240.0, 80.0).child(
            OpenContainer::<()>::new(
                |_ctx, _open| { Center::new().child(Text::new("Tap to open")) },
                |_ctx, close| {
                    let close = close.clone();
                    Scaffold::new()
                        .app_bar(AppBar::new().title(Text::new("Opened container")))
                        .body(Center::new().child(
                            FilledButton::new(Text::new("Close")).on_pressed(move |_| {
                                let _ = close.close();
                            }),
                        ))
                },
            )
            .closed_color(LANE)
            .semantic_label("Open the container"),
        ),
    ])
    .boxed()
}

fn main() {
    let preference = match std::env::args().nth(1).as_deref() {
        None | Some("--system") => MotionOverride::FollowSystem,
        Some("--full") => MotionOverride::Full,
        Some("--reduce") => MotionOverride::Reduce,
        Some(_) => {
            eprintln!("Usage: motion_lab [--system|--full|--reduce]");
            std::process::exit(2);
        }
    };
    run_app_with_config(
        MotionLab,
        AppConfig::default()
            .with_title("FLUI motion lab")
            .with_size(960, 900)
            .with_min_size(960, 900)
            .with_motion_override(preference),
    );
}
