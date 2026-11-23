//! What whole application scenarios cost on the UI thread, timed and counted.
//!
//! The perf targets of `flui-widgets` and this crate hold counts for one frame
//! of a fixed tree or one control; this bench drives Material applications the
//! way a user meets them: the start and first frame of a list screen, a steady
//! scroll and a page-per-frame jump through 10 000 list tiles, a screen of
//! wrapped paragraphs mounted, scrolled and edited, a page pushed and popped
//! through the Router, a form toggled, and an idle frame. Each scenario runs on
//! a `HeadlessHost`, so the numbers are the UI thread's: build, layout, paint
//! recording, semantics and the scene's publication. Rasterization is the
//! engine's and is measured by its own benches.
//!
//! `cargo bench -p flui-material --bench app_scenarios` prints, per scenario,
//! the median and the 90th percentile of the wall time of one measured step,
//! and what the step asked of the test thread's heap. `FLUI_SCENARIO=<name>`
//! runs one scenario; under `valgrind --tool=callgrind
//! --toggle-collect=*scenario_measured*` only the measured steps are counted,
//! which is how a phase's share of a step is read.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use flui_material::{
    AppBar, Card, Checkbox, ElevatedButton, InputDecoration, ListTile, MaterialApp, Scaffold,
    Switch, TextField,
};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    Column, Icon, IconData, ListView, Padding, PageHandle, Routable, Router, RouterHandle,
    ScrollController, ScrollView, SingleChildScrollView, Text, TextEditingController,
};
use flui_testing::heap::{CountingAllocator, HeapCounts};
use flui_testing::host::{HeadlessHost, HeadlessWindow};
use flui_testing::widgets::SharedSignal;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// One 60 Hz frame of virtual time.
const FRAME: Duration = Duration::from_nanos(16_666_667);
const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const TILES: usize = 10_000;
/// A two-line `ListTile`'s height, the lazy list's first estimate.
const TILE_HEIGHT: f64 = 72.0;

/// What one measured step cost.
#[derive(Clone, Copy)]
struct Sample {
    nanos: u64,
    heap: HeapCounts,
}

/// Runs one measured step. Kept out of line under its name: callgrind's
/// `--toggle-collect=*scenario_measured*` counts only inside it.
#[inline(never)]
fn scenario_measured<R>(step: impl FnOnce() -> R) -> (R, Sample) {
    let before = HeapCounts::now();
    let start = Instant::now();
    let result = step();
    let nanos = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let heap = HeapCounts::now().since(before);
    (result, Sample { nanos, heap })
}

fn host() -> HeadlessHost {
    HeadlessHost::new(HeadlessWindow::new(WIDTH, HEIGHT))
}

fn pump(host: &mut HeadlessHost) {
    let _outcome = host.pump(FRAME);
}

/// Pumps until the application asks for no frame, at most `limit` frames.
fn settle(host: &mut HeadlessHost, limit: usize) {
    host.pump_until_settled(FRAME, limit)
        .expect("the scenario settles");
}

// --- The list screen -------------------------------------------------------

/// A Material list screen: an app bar over 10 000 two-line tiles, each with a
/// leading icon, a title, a subtitle and a trailing switch.
#[derive(Clone, StatelessView)]
struct ListScreen {
    controller: ScrollController,
}

impl StatelessView for ListScreen {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let list = ListView::builder(TILES, TILE_HEIGHT, |index| {
            (index < TILES).then(|| {
                ListTile::new()
                    .leading(Icon::new(IconData::new(0xE7FD)))
                    .title(Text::new(format!("Contact {index}")))
                    .subtitle(Text::new(format!("contact{index}@example.com")))
                    .trailing(Switch::new(index % 3 == 0))
                    .on_tap(|_ctx| {})
                    .boxed()
            })
        })
        .controller(self.controller.clone());
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new("Contacts")))
            .body(list)
    }
}

fn list_app(controller: &ScrollController) -> MaterialApp {
    MaterialApp::new(ListScreen {
        controller: controller.clone(),
    })
}

/// Start: the host made, the application attached, and the frames that show
/// its first screen (a lazy band lands on the frame after the first).
fn startup_list() -> Vec<Sample> {
    (0..12)
        .map(|_| {
            let controller = ScrollController::new();
            let (host, sample) = scenario_measured(|| {
                let mut host = host();
                host.attach(&list_app(&controller)).expect("a fresh host");
                pump(&mut host);
                pump(&mut host);
                host
            });
            drop(host);
            sample
        })
        .collect()
}

/// A mounted list screen, settled.
fn mounted_list() -> (HeadlessHost, ScrollController) {
    let controller = ScrollController::new();
    let mut host = host();
    host.attach(&list_app(&controller)).expect("a fresh host");
    settle(&mut host, 60);
    (host, controller)
}

/// A steady drag: 12 logical pixels a frame, one new tile every six frames.
fn list_scroll_steady() -> Vec<Sample> {
    let (mut host, controller) = mounted_list();
    let mut offset = 0.0;
    // Warm: the first screens build every tile kind once.
    for _ in 0..120 {
        offset += 12.0;
        controller.jump_to(offset).expect("outside a build");
        pump(&mut host);
    }
    (0..240)
        .map(|_| {
            offset += 12.0;
            controller.jump_to(offset).expect("outside a build");
            scenario_measured(|| pump(&mut host)).1
        })
        .collect()
}

/// A fling at its fastest: one screen a frame, every visible tile new.
fn list_scroll_page() -> Vec<Sample> {
    let (mut host, controller) = mounted_list();
    let mut offset = 0.0;
    for _ in 0..8 {
        offset += f64::from(HEIGHT);
        controller.jump_to(offset).expect("outside a build");
        pump(&mut host);
    }
    (0..60)
        .map(|_| {
            offset += f64::from(HEIGHT);
            controller.jump_to(offset).expect("outside a build");
            scenario_measured(|| pump(&mut host)).1
        })
        .collect()
}

/// A frame with nothing to do on the settled list screen.
fn idle_list() -> Vec<Sample> {
    let (mut host, _controller) = mounted_list();
    (0..240)
        .map(|_| {
            host.request_frame();
            scenario_measured(|| pump(&mut host)).1
        })
        .collect()
}

// --- The text screen ------------------------------------------------------

const PARAGRAPHS: usize = 40;
const LOREM: &str = "Declarative interfaces describe what the screen shows for a \
    given state and let the framework work out what changed. A paragraph like this \
    one wraps over several lines at the width of a phone or a narrow window, so its \
    layout shapes and breaks every run of text it holds before anything is painted.";

/// A long article: cards of a heading and a wrapped paragraph in a scroll
/// view, one paragraph editable from the test.
#[derive(Clone, StatefulView)]
struct Article {
    edited: SharedSignal<String>,
    controller: ScrollController,
}

struct ArticleState {
    edited: SharedSignal<String>,
}

impl StatefulView for Article {
    type State = ArticleState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ArticleState {
            edited: self.edited.clone(),
        }
    }
}

impl ViewState<Article> for ArticleState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.edited.bind(ctx);
    }
    fn build(&self, view: &Article, ctx: &dyn BuildContext) -> impl IntoView {
        let edited = self.edited.get(ctx);
        let cards: Vec<BoxedView> = (0..PARAGRAPHS)
            .map(|index| {
                let body = if index == 3 {
                    edited.clone()
                } else {
                    LOREM.to_owned()
                };
                Card::new()
                    .child(Padding::all(16.0).child(Column::new((
                        Text::new(format!("Section {index}")),
                        Text::new(body),
                    ))))
                    .boxed()
            })
            .collect();
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new("Article")))
            .body(
                SingleChildScrollView::new()
                    .controller(view.controller.clone())
                    .child(Column::new(cards)),
            )
    }
}

fn article(edited: &SharedSignal<String>, controller: &ScrollController) -> MaterialApp {
    MaterialApp::new(Article {
        edited: edited.clone(),
        controller: controller.clone(),
    })
}

fn startup_text() -> Vec<Sample> {
    (0..12)
        .map(|_| {
            let edited = SharedSignal::new(LOREM.to_owned());
            let controller = ScrollController::new();
            let (host, sample) = scenario_measured(|| {
                let mut host = host();
                host.attach(&article(&edited, &controller))
                    .expect("a fresh host");
                pump(&mut host);
                host
            });
            drop(host);
            sample
        })
        .collect()
}

fn mounted_article() -> (HeadlessHost, SharedSignal<String>, ScrollController) {
    let edited = SharedSignal::new(LOREM.to_owned());
    let controller = ScrollController::new();
    let mut host = host();
    host.attach(&article(&edited, &controller))
        .expect("a fresh host");
    settle(&mut host, 60);
    (host, edited, controller)
}

fn text_scroll() -> Vec<Sample> {
    let (mut host, _edited, controller) = mounted_article();
    let mut offset = 0.0;
    for _ in 0..30 {
        offset += 12.0;
        controller.jump_to(offset).expect("outside a build");
        pump(&mut host);
    }
    (0..120)
        .map(|_| {
            offset += 12.0;
            controller.jump_to(offset).expect("outside a build");
            scenario_measured(|| pump(&mut host)).1
        })
        .collect()
}

/// One paragraph's text changes: the screen's state rebuilds, as a
/// `setState` on a screen does.
fn text_edit() -> Vec<Sample> {
    let (mut host, edited, _controller) = mounted_article();
    for round in 0..10 {
        edited.set(format!("{LOREM} Edit {round}."));
        pump(&mut host);
    }
    (0..60)
        .map(|round| {
            edited.set(format!("{LOREM} Change {round}."));
            scenario_measured(|| pump(&mut host)).1
        })
        .collect()
}

// --- Navigation -----------------------------------------------------------

#[derive(Routable, Clone, Debug, PartialEq)]
enum Mail {
    #[route("/")]
    Inbox,
    #[route("/message/:id")]
    Message { id: u32 },
}

type RouterSlot = Rc<RefCell<Option<RouterHandle<Mail>>>>;

/// The inbox: a list of messages; the test pushes a message through the
/// Router the inbox hands out.
#[derive(Clone, StatelessView)]
struct Inbox {
    router: RouterSlot,
}

impl StatelessView for Inbox {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let router = Router::<Mail>::of(ctx).expect("the inbox is a page of the Router");
        *self.router.borrow_mut() = Some(router);
        let list = ListView::builder(500, TILE_HEIGHT, |index| {
            (index < 500).then(|| {
                ListTile::new()
                    .leading(Icon::new(IconData::new(0xE0BE)))
                    .title(Text::new(format!("Message {index}")))
                    .subtitle(Text::new("A preview of what the message says"))
                    .on_tap(|_ctx| {})
                    .boxed()
            })
        });
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new("Inbox")))
            .body(list)
    }
}

#[derive(Clone, StatelessView)]
struct Message {
    id: u32,
}

impl StatelessView for Message {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let _page = PageHandle::of(ctx).expect("a message is a page of the Router");
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new(format!("Message {}", self.id))))
            .body(Padding::all(16.0).child(Column::new((
                Text::new(LOREM),
                Text::new(LOREM),
                ElevatedButton::new(Text::new("Archive")).on_pressed(|_ctx| {}),
            ))))
    }
}

/// A push and its transition to the end, then the pop and its transition: one
/// sample a frame of either.
fn navigation() -> Vec<Sample> {
    let slot: RouterSlot = Rc::default();
    let inbox_slot = Rc::clone(&slot);
    let router = Router::new(Mail::Inbox, move |route: &Mail, _ctx| match route {
        Mail::Inbox => Inbox {
            router: Rc::clone(&inbox_slot),
        }
        .boxed(),
        Mail::Message { id } => Message { id: *id }.boxed(),
    });
    let mut host = host();
    host.attach(&MaterialApp::router(router))
        .expect("a fresh host");
    settle(&mut host, 60);
    let router = slot.borrow().clone().expect("the inbox built");
    let mut samples = Vec::new();
    for id in 0..12 {
        router.push(Mail::Message { id }).expect("a push");
        for _ in 0..40 {
            samples.push(scenario_measured(|| pump(&mut host)).1);
        }
        router.pop().expect("a pop");
        for _ in 0..40 {
            samples.push(scenario_measured(|| pump(&mut host)).1);
        }
    }
    samples
}

// --- A form ---------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct Form {
    accepted: SharedSignal<bool>,
}

struct FormState {
    accepted: SharedSignal<bool>,
    fields: Vec<TextEditingController>,
}

impl StatefulView for Form {
    type State = FormState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        FormState {
            accepted: self.accepted.clone(),
            fields: (0..8)
                .map(|index| TextEditingController::with_text(format!("Value {index}")))
                .collect(),
        }
    }
}

impl ViewState<Form> for FormState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.accepted.bind(ctx);
    }
    fn build(&self, _view: &Form, ctx: &dyn BuildContext) -> impl IntoView {
        let accepted = self.accepted.get(ctx);
        let mut rows: Vec<BoxedView> = Vec::new();
        for (index, field) in self.fields.iter().enumerate() {
            rows.push(
                TextField::new(field.clone())
                    .decoration(InputDecoration {
                        label_text: Some(format!("Field {index}")),
                        ..InputDecoration::default()
                    })
                    .boxed(),
            );
        }
        for index in 0..8 {
            rows.push(
                ListTile::new()
                    .title(Text::new(format!("Option {index}")))
                    .trailing(Checkbox::new(accepted && index % 2 == 0))
                    .boxed(),
            );
            rows.push(
                ListTile::new()
                    .title(Text::new(format!("Setting {index}")))
                    .trailing(Switch::new(accepted))
                    .boxed(),
            );
        }
        rows.push(
            ElevatedButton::new(Text::new("Save"))
                .on_pressed(|_ctx| {})
                .boxed(),
        );
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new("Settings")))
            .body(SingleChildScrollView::new().child(Column::new(rows)))
    }
}

fn startup_form() -> Vec<Sample> {
    (0..12)
        .map(|_| {
            let accepted = SharedSignal::new(false);
            let (host, sample) = scenario_measured(|| {
                let mut host = host();
                host.attach(&MaterialApp::new(Form {
                    accepted: accepted.clone(),
                }))
                .expect("a fresh host");
                pump(&mut host);
                host
            });
            drop(host);
            sample
        })
        .collect()
}

/// The form's state flips: the screen rebuilds and every checkbox and switch
/// starts its transition; one sample a frame until the form settles.
fn form_toggle() -> Vec<Sample> {
    let accepted = SharedSignal::new(false);
    let mut host = host();
    host.attach(&MaterialApp::new(Form {
        accepted: accepted.clone(),
    }))
    .expect("a fresh host");
    settle(&mut host, 120);
    let mut samples = Vec::new();
    for round in 0..8 {
        accepted.set(round % 2 == 0);
        for _ in 0..30 {
            samples.push(scenario_measured(|| pump(&mut host)).1);
        }
    }
    samples
}

// --- Report ---------------------------------------------------------------

fn percentile(sorted: &[u64], fraction: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn report(name: &str, samples: &[Sample]) {
    let mut nanos: Vec<u64> = samples.iter().map(|sample| sample.nanos).collect();
    nanos.sort_unstable();
    let mut allocations: Vec<u64> = samples
        .iter()
        .map(|sample| sample.heap.allocations)
        .collect();
    allocations.sort_unstable();
    let mut bytes: Vec<u64> = samples
        .iter()
        .map(|sample| sample.heap.allocated_bytes)
        .collect();
    bytes.sort_unstable();
    let count = samples.len() as u64;
    let total_allocations: u64 = allocations.iter().sum();
    let total_bytes: u64 = bytes.iter().sum();
    println!(
        "{name:<20} n={count:<4} median={:>9.1}µs p90={:>9.1}µs max={:>9.1}µs \
         allocs/step med={:<6} mean={:<8} bytes/step med={:<9} mean={}",
        percentile(&nanos, 0.5) as f64 / 1e3,
        percentile(&nanos, 0.9) as f64 / 1e3,
        percentile(&nanos, 1.0) as f64 / 1e3,
        percentile(&allocations, 0.5),
        total_allocations / count.max(1),
        percentile(&bytes, 0.5),
        total_bytes / count.max(1),
    );
}

type Scenario = (&'static str, fn() -> Vec<Sample>);

const SCENARIOS: [Scenario; 10] = [
    ("startup_list", startup_list),
    ("list_scroll_steady", list_scroll_steady),
    ("list_scroll_page", list_scroll_page),
    ("idle_list", idle_list),
    ("startup_text", startup_text),
    ("text_scroll", text_scroll),
    ("text_edit", text_edit),
    ("navigation", navigation),
    ("startup_form", startup_form),
    ("form_toggle", form_toggle),
];

fn main() {
    let only = std::env::var("FLUI_SCENARIO").ok();
    for (name, scenario) in SCENARIOS {
        if only.as_deref().is_some_and(|only| only != name) {
            continue;
        }
        let samples = scenario();
        if std::env::var_os("FLUI_SCENARIO_SAMPLES").is_some() {
            for (step, sample) in samples.iter().enumerate() {
                println!(
                    "{name} step={step} µs={:.1} allocs={} bytes={}",
                    sample.nanos as f64 / 1e3,
                    sample.heap.allocations,
                    sample.heap.allocated_bytes
                );
            }
        }
        report(name, &samples);
    }
}
