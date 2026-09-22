//! ADR-0074 §8 go/no-go: `setState` (variant **A**) against realm-scoped
//! signals (variant **B**) on the **same widget tree**, in one file.
//!
//! For every scenario both variants mount the identical tree shape (a `Column`
//! of `SizedBox` cells, a 20-field form with a "Save" cell, three "screens" of
//! cells reading one setting), apply the same one-value change, pump one frame,
//! and report:
//!
//! - `elements_built` and its `RebuildReason` split, from
//!   `BuildOwner::last_frame_build_report` (the drain's own telemetry);
//! - `layout_roots`, the frame's difference of `PipelineOwner::layout_roots_total`
//!   (dirty layout entries the frame's `run_layout` passes drained);
//! - the wall time of `change + pump_frame`, from criterion.
//!
//! The counts are deterministic and printed once as a markdown table (that
//! table is the evidence ADR-0074 §8 cites); the timings follow as criterion
//! groups. Run with `just bench-signals`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_view::{FrameBuildReport, Reactive, RebuildReason, Signal, StateHandle, View};
use flui_widgets::prelude::*;
use flui_widgets::testing::{LaidOut, lay_out, loose};
use flui_widgets::{Column, SizedBox};

const LIST_ROWS: usize = 10_000;
const FORM_FIELDS: usize = 20;
const SCREENS: usize = 3;
const CELLS_PER_SCREEN: usize = 200;

fn cell(v: u32) -> SizedBox {
    SizedBox::square(1.0 + (v % 7) as f32)
}

/// A: one cell, one plain value. Same element shape as [`CellB`] (a
/// stateless view over a `SizedBox`), so both variants carry two elements
/// per cell and the rebuild counts compare like for like.
#[derive(Clone, Debug, StatelessView)]
struct CellA {
    v: u32,
}

impl StatelessView for CellA {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        cell(self.v)
    }
}

// ---------------------------------------------------------------- variant A

/// A: the owning `ViewState` holds a `Vec<u32>` behind a `StateHandle`; a
/// change is `handle.update(..)` which schedules **this** element, and its
/// build re-emits every child.
#[derive(Clone, StatefulView)]
struct ListA {
    values: StateHandle<Vec<u32>>,
}

struct ListAState {
    values: StateHandle<Vec<u32>>,
}

impl StatefulView for ListA {
    type State = ListAState;
    fn create_state(&self) -> Self::State {
        ListAState {
            values: self.values.clone(),
        }
    }
}

impl ViewState<ListA> for ListAState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.values.bind(ctx);
    }
    fn build(&self, _view: &ListA, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_view::ViewExt;
        Column::new(self.values.with(|values| {
            values
                .iter()
                .map(|v| CellA { v: *v }.boxed())
                .collect::<Vec<_>>()
        }))
    }
}

/// A: 20 fields plus a "Save" cell whose size encodes the invalid count; the
/// whole form is one `ViewState`.
#[derive(Clone, StatefulView)]
struct FormA {
    fields: StateHandle<Vec<u32>>,
}

struct FormAState {
    fields: StateHandle<Vec<u32>>,
}

impl StatefulView for FormA {
    type State = FormAState;
    fn create_state(&self) -> Self::State {
        FormAState {
            fields: self.fields.clone(),
        }
    }
}

impl ViewState<FormA> for FormAState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.fields.bind(ctx);
    }
    fn build(&self, _view: &FormA, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_view::ViewExt;
        self.fields.with(|fields| {
            let mut children: Vec<_> = fields.iter().map(|v| CellA { v: *v }.boxed()).collect();
            let invalid = fields.iter().filter(|v| **v == 0).count() as u32;
            children.push(SizedBox::square(1.0 + invalid as f32).boxed());
            Column::new(children)
        })
    }
}

/// A: one setting read by every cell of three screens; the app root owns it.
#[derive(Clone, StatefulView)]
struct AppA {
    unit: StateHandle<u32>,
}

struct AppAState {
    unit: StateHandle<u32>,
}

impl StatefulView for AppA {
    type State = AppAState;
    fn create_state(&self) -> Self::State {
        AppAState {
            unit: self.unit.clone(),
        }
    }
}

impl ViewState<AppA> for AppAState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.unit.bind(ctx);
    }
    fn build(&self, _view: &AppA, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_view::ViewExt;
        let unit = self.unit.with(|u| *u);
        Column::new(
            (0..SCREENS)
                .map(|_| {
                    Column::new(
                        (0..CELLS_PER_SCREEN)
                            .map(|_| CellA { v: unit }.boxed())
                            .collect::<Vec<_>>(),
                    )
                    .boxed()
                })
                .collect::<Vec<_>>(),
        )
    }
}

// ---------------------------------------------------------------- variant B

/// B: one cell, one signal. The cell is the reader; nothing above it depends
/// on the value.
#[derive(Clone, Debug, StatelessView)]
struct CellB {
    sig: Signal<u32>,
}

impl StatelessView for CellB {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        cell(self.sig.get(ctx))
    }
}

/// B: the list reads only the row *count*; each row reads its own signal.
#[derive(Clone, Debug, StatelessView)]
struct ListB {
    count: Signal<usize>,
    rows: Rc<RefCell<Vec<Signal<u32>>>>,
}

impl StatelessView for ListB {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        use flui_view::ViewExt;
        let count = self.count.get(ctx);
        let rows = self.rows.borrow();
        Column::new(
            rows.iter()
                .take(count)
                .map(|sig| CellB { sig: *sig }.boxed())
                .collect::<Vec<_>>(),
        )
    }
}

/// B: the form root reads nothing; each field subscribes individually. The
/// "Save" cell reads every field (a derived value is ADR-0075's subject), so
/// it rebuilds on any keystroke in both variants.
#[derive(Clone, Debug, StatelessView)]
struct FormB {
    fields: Rc<Vec<Signal<u32>>>,
}

/// B: the Save cell reads all fields directly.
#[derive(Clone, Debug, StatelessView)]
struct SaveB {
    fields: Rc<Vec<Signal<u32>>>,
}

impl StatelessView for SaveB {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let invalid = self.fields.iter().filter(|f| f.get(ctx) == 0).count();
        SizedBox::square(1.0 + invalid as f32)
    }
}

impl StatelessView for FormB {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_view::ViewExt;
        let mut children: Vec<_> = self
            .fields
            .iter()
            .map(|sig| CellB { sig: *sig }.boxed())
            .collect();
        children.push(
            SaveB {
                fields: Rc::clone(&self.fields),
            }
            .boxed(),
        );
        Column::new(children)
    }
}

/// B: three screens of cells all reading the one setting signal.
#[derive(Clone, Debug, StatelessView)]
struct AppB {
    unit: Signal<u32>,
}

impl StatelessView for AppB {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_view::ViewExt;
        Column::new(
            (0..SCREENS)
                .map(|_| {
                    Column::new(
                        (0..CELLS_PER_SCREEN)
                            .map(|_| CellB { sig: self.unit }.boxed())
                            .collect::<Vec<_>>(),
                    )
                    .boxed()
                })
                .collect::<Vec<_>>(),
        )
    }
}

// ------------------------------------------------------------------ harness

/// Applies "the change" for step `i` to a mounted tree.
type Change = Box<dyn FnMut(&mut LaidOut, u64)>;

/// One mounted tree plus the closure that applies "the change" for step `i`.
struct Scenario {
    name: &'static str,
    variant: &'static str,
    laid: LaidOut,
    change: Change,
}

struct Measured {
    build: FrameBuildReport,
    layout_roots: usize,
}

impl Scenario {
    fn step(&mut self, i: u64) -> Measured {
        let before = self
            .laid
            .pipeline_owner()
            .with(flui_rendering::pipeline::PipelineOwner::layout_roots_total);
        (self.change)(&mut self.laid, i);
        self.laid.tick();
        let after = self
            .laid
            .pipeline_owner()
            .with(flui_rendering::pipeline::PipelineOwner::layout_roots_total);
        Measured {
            build: self.laid.build_owner_mut().last_frame_build_report(),
            layout_roots: (after - before) as usize,
        }
    }
}

fn reactive(laid: &mut LaidOut) -> Reactive {
    laid.build_owner_mut().reactive().clone()
}

fn mount_b<V: View>(make: impl FnOnce(&Reactive) -> V) -> LaidOut {
    // The tree's own graph hands out the signals, so mount a placeholder first.
    let mut laid = lay_out(SizedBox::square(1.0), loose(4096.0));
    let r = reactive(&mut laid);
    laid.pump_widget(make(&r));
    laid
}

fn scenarios() -> Vec<Scenario> {
    let mut out = Vec::new();

    // 1. List: one visible row changes value.
    {
        let values = StateHandle::new((0..LIST_ROWS as u32).collect::<Vec<_>>());
        let laid = lay_out(
            ListA {
                values: values.clone(),
            },
            loose(4096.0),
        );
        out.push(Scenario {
            name: "list 10k: one row changes",
            variant: "A setState",
            laid,
            change: Box::new(move |_, i| values.update(|v| v[4217] = 100 + (i as u32 % 2))),
        });
        let rows: Rc<RefCell<Vec<Signal<u32>>>> = Rc::new(RefCell::new(Vec::new()));
        let rows_for_mount = Rc::clone(&rows);
        let mut laid = mount_b(|r| {
            rows_for_mount.replace((0..LIST_ROWS as u32).map(|v| r.signal(v)).collect());
            ListB {
                count: r.signal(LIST_ROWS),
                rows: Rc::clone(&rows_for_mount),
            }
        });
        let r = reactive(&mut laid);
        let row = rows.borrow()[4217];
        out.push(Scenario {
            name: "list 10k: one row changes",
            variant: "B signals",
            laid,
            change: Box::new(move |_, i| row.set(&r, 100 + (i as u32 % 2)).expect("row alive")),
        });
    }

    // 2. List: append one row (then remove it, so the tree stays bounded).
    {
        let values = StateHandle::new((0..LIST_ROWS as u32).collect::<Vec<_>>());
        let laid = lay_out(
            ListA {
                values: values.clone(),
            },
            loose(4096.0),
        );
        out.push(Scenario {
            name: "list 10k: append one row",
            variant: "A setState",
            laid,
            change: Box::new(move |_, i| {
                values.update(|v| {
                    if i % 2 == 0 {
                        v.push(7);
                    } else {
                        v.pop();
                    }
                });
            }),
        });
        let rows: Rc<RefCell<Vec<Signal<u32>>>> = Rc::new(RefCell::new(Vec::new()));
        let rows_for_mount = Rc::clone(&rows);
        let count_cell: Rc<RefCell<Option<Signal<usize>>>> = Rc::new(RefCell::new(None));
        let count_for_mount = Rc::clone(&count_cell);
        let mut laid = mount_b(|r| {
            rows_for_mount.replace((0..=LIST_ROWS as u32).map(|v| r.signal(v)).collect());
            let count = r.signal(LIST_ROWS);
            count_for_mount.replace(Some(count));
            ListB {
                count,
                rows: Rc::clone(&rows_for_mount),
            }
        });
        let r = reactive(&mut laid);
        let count = count_cell.borrow().expect("mounted");
        out.push(Scenario {
            name: "list 10k: append one row",
            variant: "B signals",
            laid,
            change: Box::new(move |_, i| {
                count
                    .set(&r, if i % 2 == 0 { LIST_ROWS + 1 } else { LIST_ROWS })
                    .expect("count alive");
            }),
        });
    }

    // 3. Form: type into field 7 (5 -> 6 -> 5 ...). The Save cell reads
    //    every field in both variants, so it rebuilds on each keystroke; the
    //    other 19 fields are what the variants differ on.
    {
        let initial: Vec<u32> = (0..FORM_FIELDS as u32)
            .map(|i| if i % 3 == 0 { 0 } else { 5 })
            .collect();
        let fields = StateHandle::new(initial.clone());
        let laid = lay_out(
            FormA {
                fields: fields.clone(),
            },
            loose(4096.0),
        );
        out.push(Scenario {
            name: "form 20: keystroke in one field",
            variant: "A setState",
            laid,
            change: Box::new(move |_, i| fields.update(|f| f[7] = 5 + (i % 2) as u32)),
        });
        let fields_cell: Rc<RefCell<Vec<Signal<u32>>>> = Rc::new(RefCell::new(Vec::new()));
        let fields_for_mount = Rc::clone(&fields_cell);
        let mut laid = mount_b(|r| {
            let fields: Vec<Signal<u32>> = initial.iter().map(|v| r.signal(*v)).collect();
            fields_for_mount.replace(fields.clone());
            FormB {
                fields: Rc::new(fields),
            }
        });
        let r = reactive(&mut laid);
        let field7 = fields_cell.borrow()[7];
        out.push(Scenario {
            name: "form 20: keystroke in one field",
            variant: "B signals",
            laid,
            change: Box::new(move |_, i| {
                field7.set(&r, 5 + (i % 2) as u32).expect("field alive");
            }),
        });
    }

    // 5. Setting read by 600 cells across three screens.
    {
        let unit = StateHandle::new(1u32);
        let laid = lay_out(AppA { unit: unit.clone() }, loose(4096.0));
        out.push(Scenario {
            name: "setting read by 3x200 cells",
            variant: "A setState",
            laid,
            change: Box::new(move |_, i| unit.update(|u| *u = 1 + (i as u32 % 2))),
        });
        let unit_cell: Rc<RefCell<Option<Signal<u32>>>> = Rc::new(RefCell::new(None));
        let unit_for_mount = Rc::clone(&unit_cell);
        let mut laid = mount_b(|r| {
            let unit = r.signal(1u32);
            unit_for_mount.replace(Some(unit));
            AppB { unit }
        });
        let r = reactive(&mut laid);
        let unit = unit_cell.borrow().expect("mounted");
        out.push(Scenario {
            name: "setting read by 3x200 cells",
            variant: "B signals",
            laid,
            change: Box::new(move |_, i| unit.set(&r, 1 + (i as u32 % 2)).expect("unit alive")),
        });
    }

    // 6. Idle: no change at all.
    {
        let values = StateHandle::new((0..LIST_ROWS as u32).collect::<Vec<_>>());
        let laid = lay_out(ListA { values }, loose(4096.0));
        out.push(Scenario {
            name: "idle frame (list 10k mounted)",
            variant: "A setState",
            laid,
            change: Box::new(|_, _| {}),
        });
        let rows: Rc<RefCell<Vec<Signal<u32>>>> = Rc::new(RefCell::new(Vec::new()));
        let rows_for_mount = Rc::clone(&rows);
        let laid = mount_b(|r| {
            rows_for_mount.replace((0..LIST_ROWS as u32).map(|v| r.signal(v)).collect());
            ListB {
                count: r.signal(LIST_ROWS),
                rows: Rc::clone(&rows_for_mount),
            }
        });
        out.push(Scenario {
            name: "idle frame (list 10k mounted)",
            variant: "B signals",
            laid,
            change: Box::new(|_, _| {}),
        });
    }

    out
}

fn reasons(report: &FrameBuildReport) -> String {
    let mut parts: Vec<String> = report
        .by_reason
        .iter()
        .map(|(reason, count)| format!("{}={count}", reason.as_str()))
        .collect();
    if parts.is_empty() {
        parts.push("-".to_owned());
    }
    parts.join(" ")
}

fn print_counts_table(scenarios: &mut [Scenario]) {
    println!();
    println!(
        "## ADR-0074 §8 — one change, one frame (counts are deterministic; second frame after a warm-up change)"
    );
    println!();
    println!("| scenario | variant | elements built | by reason | layout roots |");
    println!("|---|---|---:|---|---:|");
    for scenario in &mut *scenarios {
        // Warm up once so the measured frame is a steady-state change, not the
        // first mutation after mount.
        let _ = scenario.step(1);
        let measured = scenario.step(2);
        println!(
            "| {} | {} | {} | {} | {} |",
            scenario.name,
            scenario.variant,
            measured.build.elements_built,
            reasons(&measured.build),
            measured.layout_roots
        );
        debug_assert!(
            measured.build.count(RebuildReason::SignalChange) == 0
                || scenario.variant.starts_with('B'),
            "only variant B may rebuild for SignalChange"
        );
    }
    println!();
}

fn bench(c: &mut Criterion) {
    let mut scenarios = scenarios();
    print_counts_table(&mut scenarios);

    let mut group = c.benchmark_group("adr_0074_change_plus_frame");
    group.measurement_time(Duration::from_secs(3));
    group.warm_up_time(Duration::from_millis(500));
    for scenario in &mut *scenarios {
        let id = format!("{} / {}", scenario.name, scenario.variant);
        let mut i = 10u64;
        group.bench_function(id, |b| {
            b.iter(|| {
                i += 1;
                scenario.step(i);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
