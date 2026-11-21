//! A value an element publishes to its subtree behaves as an inherited view
//! does (ADR-0322 §5): each case runs over an `InheritedView`, a stateless
//! view that publishes and a state that publishes.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

use flui_foundation::ElementId;
use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::__runtime::{BuildOwner, ElementTree, LifecycleHook};
use flui_view::{
    BoxedView, BuildContext, BuildContextExt, GlobalKey, InheritedView, IntoView, LifecycleContext,
    Provided, ProvidedValue, RebuildReason, RenderView, Signal, SignalWriteExt, StatefulView,
    StatelessView, View, ViewExt, ViewState,
};

// ============================================================================
// Values
// ============================================================================

/// What a reader takes from the provider it resolved.
trait Tone: 'static {
    fn tone(&self) -> u32;
}

#[derive(Clone, Debug, PartialEq)]
struct Palette(u32);

impl ProvidedValue for Palette {
    fn update_should_notify(&self, old: &Self) -> bool {
        self != old
    }
}

impl Tone for Palette {
    fn tone(&self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Density(u32);

impl ProvidedValue for Density {
    fn update_should_notify(&self, old: &Self) -> bool {
        self != old
    }
}

impl Tone for Density {
    fn tone(&self) -> u32 {
        self.0
    }
}

/// A value published only so that its element is one more provider layer.
#[derive(Clone, Debug, PartialEq)]
struct Filler(u32);

impl ProvidedValue for Filler {
    fn update_should_notify(&self, old: &Self) -> bool {
        self != old
    }
}

/// A published value whose `Drop` is authored to fail once.
struct Fragile(u32, Rc<Cell<bool>>);

impl ProvidedValue for Fragile {
    fn update_should_notify(&self, old: &Self) -> bool {
        self.0 != old.0
    }
}

impl Tone for Fragile {
    fn tone(&self) -> u32 {
        self.0
    }
}

impl Drop for Fragile {
    fn drop(&mut self) {
        assert!(!self.1.replace(false), "authored drop failure");
    }
}

// ============================================================================
// Providers
// ============================================================================

/// What a publishing view publishes this build.
#[derive(Clone, Default)]
struct Published {
    palette: Option<u32>,
    density: Option<u32>,
    /// Panic in `provide` while set.
    fail: Option<Rc<Cell<bool>>>,
    /// A value whose `Drop` panics once while the flag is set.
    fragile: Option<(u32, Rc<Cell<bool>>)>,
    /// A palette whose tone is read from a signal inside `provide`.
    signal: Option<Signal<u32>>,
    /// A [`Filler`], which nobody reads.
    filler: Option<u32>,
}

impl Published {
    fn palette(tone: u32) -> Self {
        Self {
            palette: Some(tone),
            ..Self::default()
        }
    }

    fn write(&self, ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        if let Some(fail) = &self.fail {
            assert!(!fail.get(), "authored provide failure");
        }
        if let Some(tone) = self.palette {
            values.value(Palette(tone));
        }
        if let Some((tone, armed)) = &self.fragile {
            values.value(Fragile(*tone, Rc::clone(armed)));
        }
        if let Some(signal) = self.signal {
            values.value(Palette(signal.get(ctx)));
        }
        if let Some(density) = self.density {
            values.value(Density(density));
        }
        if let Some(filler) = self.filler {
            values.value(Filler(filler));
        }
    }
}

#[derive(Clone)]
struct Inherit {
    tone: u32,
    child: BoxedView,
}

impl InheritedView for Inherit {
    type Data = u32;

    fn data(&self) -> &u32 {
        &self.tone
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.tone != old.tone
    }
}

impl View for Inherit {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::inherited(self)
    }
}

impl Tone for Inherit {
    fn tone(&self) -> u32 {
        self.tone
    }
}

#[derive(Clone)]
struct Plain {
    published: Published,
    child: BoxedView,
}

impl StatelessView for Plain {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }

    fn provide(&self, ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        self.published.write(ctx, values);
    }
}

impl View for Plain {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct Kept {
    published: Published,
    child: BoxedView,
}

struct KeptState;

impl StatefulView for Kept {
    type State = KeptState;

    fn create_state(&self, _ctx: &dyn flui_view::LifecycleContext) -> KeptState {
        KeptState
    }
}

impl ViewState<Kept> for KeptState {
    fn build(&self, view: &Kept, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }

    fn provide(&self, view: &Kept, ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        view.published.write(ctx, values);
    }
}

impl View for Kept {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

// ============================================================================
// Readers and scaffolding
// ============================================================================

/// What one reader observed.
#[derive(Clone, Default)]
struct Seen {
    seen: Rc<Cell<Option<u32>>>,
    changes: Rc<Cell<usize>>,
}

/// Reads the nearest `K` in every build and counts its dependency changes.
struct Reader<K> {
    probe: Seen,
    key: PhantomData<fn() -> K>,
}

impl<K> Clone for Reader<K> {
    fn clone(&self) -> Self {
        Self {
            probe: self.probe.clone(),
            key: PhantomData,
        }
    }
}

struct ReaderState(Seen);

impl<K: Tone> StatefulView for Reader<K> {
    type State = ReaderState;

    fn create_state(&self, _ctx: &dyn flui_view::LifecycleContext) -> ReaderState {
        ReaderState(self.probe.clone())
    }
}

impl<K: Tone> ViewState<Reader<K>> for ReaderState {
    fn did_change_dependencies(&mut self, _ctx: &dyn LifecycleContext) {
        self.0.changes.set(self.0.changes.get() + 1);
    }

    fn build(&self, _view: &Reader<K>, ctx: &dyn BuildContext) -> impl IntoView {
        self.0.seen.set(ctx.depend_on::<K, _>(Tone::tone));
        Leaf
    }
}

impl<K: Tone> View for Reader<K> {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

fn reader<K: Tone>(probe: &Seen) -> BoxedView {
    Reader::<K> {
        probe: probe.clone(),
        key: PhantomData,
    }
    .boxed()
}

#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// Several children under one element.
#[derive(Clone)]
struct Group(Vec<BoxedView>);

impl RenderView for Group {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        for child in &self.0 {
            visitor(child);
        }
    }
}

impl View for Group {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// Carries its child to wherever its key is built next.
#[derive(Clone, StatefulView)]
struct Keyed {
    #[view(key)]
    key: GlobalKey<KeyedState>,
    child: BoxedView,
}

struct KeyedState;

impl StatefulView for Keyed {
    type State = KeyedState;

    fn create_state(&self, _ctx: &dyn flui_view::LifecycleContext) -> KeyedState {
        KeyedState
    }
}

impl ViewState<Keyed> for KeyedState {
    fn build(&self, view: &Keyed, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}

/// One way to publish a tone, and the reader that finds it.
struct Kind {
    name: &'static str,
    provider: fn(u32, BoxedView) -> BoxedView,
    reader: fn(&Seen) -> BoxedView,
}

const KINDS: [Kind; 3] = [
    Kind {
        name: "inherited view",
        provider: |tone, child| Inherit { tone, child }.boxed(),
        reader: reader::<Inherit>,
    },
    Kind {
        name: "stateless view",
        provider: |tone, child| {
            Plain {
                published: Published::palette(tone),
                child,
            }
            .boxed()
        },
        reader: reader::<Palette>,
    },
    Kind {
        name: "state",
        provider: |tone, child| {
            Kept {
                published: Published::palette(tone),
                child,
            }
            .boxed()
        },
        reader: reader::<Palette>,
    },
];

/// A tree and its owner, driven one frame at a time.
struct Frames {
    tree: Rc<RefCell<ElementTree>>,
    owner: Rc<RefCell<BuildOwner>>,
    root: ElementId,
}

struct Unmounted {
    tree: Rc<RefCell<ElementTree>>,
    owner: Rc<RefCell<BuildOwner>>,
}

impl Unmounted {
    fn mounted(self, view: &dyn View) -> Frames {
        let Self { tree, owner } = self;
        let root = tree
            .borrow_mut()
            .mount_root(view, &mut owner.borrow_mut().element_owner_mut());
        let frames = Frames { tree, owner, root };
        frames.build(RebuildReason::InitialMount);
        frames
    }
}

impl Frames {
    fn mount(view: &dyn View) -> Self {
        Self::empty().mounted(view)
    }

    /// A tree and its owner before the root mounts, for a case that needs
    /// the owner's signal graph to build its view.
    fn empty() -> Unmounted {
        let tree = Rc::new(RefCell::new(ElementTree::new()));
        let owner = Rc::new(RefCell::new(BuildOwner::new()));
        flui_view::test_only_set_global_key_registry(&tree, &owner);
        Unmounted { tree, owner }
    }

    /// Build what is scheduled, without scheduling the root.
    fn drain(&self) {
        self.owner
            .borrow_mut()
            .build_scope(&mut self.tree.borrow_mut());
    }

    fn update(&self, view: &dyn View) {
        self.tree.borrow_mut().update(
            self.root,
            view,
            &mut self.owner.borrow_mut().element_owner_mut(),
        );
        self.build(RebuildReason::ParentUpdate);
    }

    fn build(&self, reason: RebuildReason) {
        self.owner
            .borrow_mut()
            .schedule_build_for(self.root, 0, reason);
        self.owner
            .borrow_mut()
            .build_scope(&mut self.tree.borrow_mut());
    }

    /// How many elements the provider at `id` counts as its dependents.
    fn dependents_of(&self, id: ElementId) -> Option<usize> {
        let tree = self.tree.borrow();
        let provider = tree.get(id)?.element().as_inherited()?;
        Some(provider.dependent_count())
    }
}

// ============================================================================
// Cases
// ============================================================================

pub(crate) fn provided_value_change_notifies_like_inherited_element() {
    for kind in &KINDS {
        let probe = Seen::default();
        let view = |tone| (kind.provider)(tone, (kind.reader)(&probe));
        let frames = Frames::mount(&view(1));
        assert_eq!(probe.seen.get(), Some(1), "{}: first read", kind.name);
        assert_eq!(probe.changes.get(), 0, "{}: nothing changed yet", kind.name);

        frames.update(&view(1));
        assert_eq!(
            probe.changes.get(),
            0,
            "{}: an equal value notifies nobody",
            kind.name
        );

        frames.update(&view(2));
        assert_eq!(probe.seen.get(), Some(2), "{}: the new value", kind.name);
        assert_eq!(
            probe.changes.get(),
            1,
            "{}: one change, one did_change_dependencies",
            kind.name
        );
    }
}

#[serial_test::serial(global_key_registry)]
pub(crate) fn provided_value_follows_a_moved_subtree() {
    for kind in &KINDS {
        let probe = Seen::default();
        let key = GlobalKey::<KeyedState>::new();
        let carried = || {
            Keyed {
                key: key.clone(),
                child: (kind.reader)(&probe),
            }
            .boxed()
        };
        let view = |under_first: bool| {
            let (first, second) = if under_first {
                (carried(), Leaf.boxed())
            } else {
                (Leaf.boxed(), carried())
            };
            Group(vec![(kind.provider)(1, first), (kind.provider)(2, second)])
        };
        let frames = Frames::mount(&view(true));
        assert_eq!(probe.seen.get(), Some(1), "{}: under the first", kind.name);

        frames.update(&view(false));
        assert_eq!(
            probe.seen.get(),
            Some(2),
            "{}: the moved reader resolves its new provider",
            kind.name
        );
        assert_eq!(
            probe.changes.get(),
            1,
            "{}: the move is one dependency change",
            kind.name
        );
    }
}

pub(crate) fn provided_value_unmount_releases_dependents() {
    for kind in &KINDS {
        let probe = Seen::default();
        let frames = Frames::mount(&(kind.provider)(1, (kind.reader)(&probe)));
        assert_eq!(
            frames.dependents_of(frames.root),
            Some(1),
            "{}: the reader is a dependent",
            kind.name
        );

        // The reader leaves; the provider stays.
        frames.update(&(kind.provider)(1, Leaf.boxed()));
        assert_eq!(
            frames.dependents_of(frames.root),
            Some(0),
            "{}: an unmounted reader is released at once",
            kind.name
        );
        frames.update(&(kind.provider)(2, Leaf.boxed()));
        assert_eq!(
            probe.changes.get(),
            0,
            "{}: a released reader is never notified",
            kind.name
        );

        // The provider leaves with a reader under it, and another mounts.
        let probe = Seen::default();
        let frames = Frames::mount(&Group(vec![(kind.provider)(1, (kind.reader)(&probe))]));
        frames.update(&Group(vec![Leaf.boxed()]));
        let late = Seen::default();
        frames.update(&Group(vec![(kind.provider)(3, (kind.reader)(&late))]));
        assert_eq!(late.seen.get(), Some(3), "{}: a fresh provider", kind.name);
        assert_eq!(
            probe.changes.get(),
            0,
            "{}: the reader that left with its provider is not notified",
            kind.name
        );
        assert!(
            frames.owner.borrow_mut().take_recovered_panics().is_empty(),
            "{}: teardown is clean",
            kind.name
        );
    }
}

/// The two publishing kinds, taking what to publish.
type Publisher = fn(Published, BoxedView) -> BoxedView;

const PUBLISHERS: [(&str, Publisher); 2] = [
    ("stateless view", |published, child| {
        Plain { published, child }.boxed()
    }),
    ("state", |published, child| {
        Kept { published, child }.boxed()
    }),
];

pub(crate) fn provided_set_change_rescopes_the_subtree() {
    for (name, publisher) in PUBLISHERS {
        let palette = Seen::default();
        let density = Seen::default();
        let view = |inner: Published| {
            let readers = Group(vec![
                reader::<Palette>(&palette),
                reader::<Density>(&density),
            ]);
            publisher(
                Published {
                    palette: Some(1),
                    density: Some(7),
                    ..Published::default()
                },
                publisher(inner, readers.boxed()),
            )
        };

        let frames = Frames::mount(&view(Published::palette(5)));
        assert_eq!(palette.seen.get(), Some(5), "{name}: the inner palette");
        assert_eq!(density.seen.get(), Some(7), "{name}: the outer density");

        // The inner provider drops one type and adds the other while both
        // readers are mounted.
        frames.update(&view(Published {
            palette: None,
            density: Some(9),
            ..Published::default()
        }));
        assert_eq!(palette.seen.get(), Some(1), "{name}: falls to the outer");
        assert_eq!(density.seen.get(), Some(9), "{name}: finds the inner");
        assert_eq!(palette.changes.get(), 1, "{name}: the palette reader woke");
        assert_eq!(density.changes.get(), 1, "{name}: the density reader woke");

        // A change of one published type wakes the readers of that type only.
        frames.update(&view(Published {
            density: Some(10),
            ..Published::default()
        }));
        assert_eq!(density.seen.get(), Some(10), "{name}: the new density");
        assert_eq!(density.changes.get(), 2, "{name}: its reader woke");
        assert_eq!(
            palette.changes.get(),
            1,
            "{name}: the palette reader did not"
        );
    }
}

/// What one element of a [`provider_chain`] publishes beside its `Filler`,
/// or that it publishes nothing and is no provider at all.
#[derive(Clone, Copy, Default)]
struct Step {
    palette: Option<u32>,
    density: Option<u32>,
    silent: bool,
}

impl Step {
    const FILLER: Self = Self {
        palette: None,
        density: None,
        silent: false,
    };
    const SILENT: Self = Self {
        palette: None,
        density: None,
        silent: true,
    };

    const fn palette(tone: u32) -> Self {
        Self {
            palette: Some(tone),
            ..Self::FILLER
        }
    }

    const fn density(tone: u32) -> Self {
        Self {
            density: Some(tone),
            ..Self::FILLER
        }
    }
}

/// `steps.len()` elements nested in each other, the outermost first, each
/// over a reader of each type (`probes` by position) beside the next
/// element; the innermost holds `tail` there instead.
fn provider_chain(
    publisher: Publisher,
    steps: &[Step],
    probes: &[(Seen, Seen)],
    tail: BoxedView,
) -> BoxedView {
    let Some((step, below)) = steps.split_first() else {
        return tail;
    };
    let (palette, density) = &probes[0];
    let children = vec![
        reader::<Palette>(palette),
        reader::<Density>(density),
        provider_chain(publisher, below, &probes[1..], tail),
    ];
    let published = if step.silent {
        Published::default()
    } else {
        Published {
            palette: step.palette,
            density: step.density,
            filler: Some(0),
            ..Published::default()
        }
    };
    publisher(published, Group(children).boxed())
}

/// Holds each reader of `probes` to the nearest value its chain publishes:
/// `above` (the ancestors outside the chain) and then `steps`, the
/// outermost first.
fn expect_chain(above: &[Step], steps: &[Step], probes: &[(Seen, Seen)], phase: &str) {
    let mut palette_wanted = None;
    let mut density_wanted = None;
    for step in above.iter().filter(|step| !step.silent) {
        palette_wanted = step.palette.or(palette_wanted);
        density_wanted = step.density.or(density_wanted);
    }
    for (position, (step, (palette, density))) in steps.iter().zip(probes).enumerate() {
        if !step.silent {
            palette_wanted = step.palette.or(palette_wanted);
            density_wanted = step.density.or(density_wanted);
        }
        let depth = position + 1;
        assert_eq!(
            palette.seen.get(),
            palette_wanted,
            "{phase}: the palette at chain position {depth}"
        );
        assert_eq!(
            density.seen.get(),
            density_wanted,
            "{phase}: the density at chain position {depth}"
        );
    }
}

/// How many elements a chain of these cases nests: enough that a lookup at
/// the bottom resolves through several tables folded above it.
const CHAIN_DEPTH: usize = 40;

fn probes(count: usize) -> Vec<(Seen, Seen)> {
    (0..count)
        .map(|_| (Seen::default(), Seen::default()))
        .collect()
}

/// A chain of [`CHAIN_DEPTH`] providers: `Palette(1)` and `Density(7)` at the
/// top, nearer values at `nearer` (position, step), a `Filler` elsewhere.
fn chain_steps(nearer: &[(usize, Step)]) -> Vec<Step> {
    let mut steps = vec![Step::FILLER; CHAIN_DEPTH];
    steps[0] = Step {
        palette: Some(1),
        density: Some(7),
        silent: false,
    };
    for (position, step) in nearer {
        steps[*position] = *step;
    }
    steps
}

pub(crate) fn a_provider_any_number_of_providers_up_is_found_and_the_nearest_shadows_it() {
    // Each case mounts its first chain, then updates to each next one in
    // place, element for element, and checks every reader after each.
    let holders = [
        (3, Step::density(8)),
        (4, Step::palette(2)),
        (7, Step::density(9)),
        (8, Step::palette(3)),
        (12, Step::density(10)),
    ];
    let without = |position: usize| {
        let mut steps = chain_steps(&holders);
        steps.remove(position);
        steps
    };
    let silenced = |position: usize| {
        let mut steps = chain_steps(&holders);
        steps[position] = Step::SILENT;
        steps
    };
    let cases: [(&str, Vec<Vec<Step>>); 4] = [
        (
            "a nearer palette two below the first comes and goes",
            vec![
                chain_steps(&[(2, Step::palette(2)), (12, Step::density(8))]),
                chain_steps(&[(12, Step::density(8))]),
                chain_steps(&[(2, Step::palette(5)), (12, Step::density(8))]),
            ],
        ),
        (
            // The providers at depths 4 and 8 move to 3 and 7, and every
            // one below them one level up.
            "a provider above the tables leaves",
            vec![chain_steps(&holders), without(1), chain_steps(&holders)],
        ),
        (
            "the provider at depth 4 stops publishing",
            vec![chain_steps(&holders), silenced(3), chain_steps(&holders)],
        ),
        (
            "the provider at depth 8 stops publishing",
            vec![chain_steps(&holders), silenced(7), chain_steps(&holders)],
        ),
    ];
    for (name, publisher) in PUBLISHERS {
        for (case, phases) in &cases {
            let probes = probes(CHAIN_DEPTH);
            let view = |steps: &[Step]| provider_chain(publisher, steps, &probes, Leaf.boxed());
            let frames = Frames::mount(&view(&phases[0]));
            for (phase, steps) in phases.iter().enumerate() {
                if phase > 0 {
                    frames.update(&view(steps));
                }
                expect_chain(
                    &[],
                    steps,
                    &probes,
                    &format!("{name}, {case}, phase {phase}"),
                );
            }
        }
    }
}

#[serial_test::serial(global_key_registry)]
pub(crate) fn a_subtree_moved_to_another_depth_reads_its_new_ancestry() {
    // Two branches of different depths under one root; a keyed chain of its
    // own providers moves from the end of the shallow one to the end of the
    // deep one and back.
    let shallow = [
        Step {
            palette: Some(1),
            density: Some(7),
            silent: false,
        },
        Step::FILLER,
    ];
    let deep = {
        let mut steps = vec![Step::FILLER; 11];
        steps[0] = Step {
            palette: Some(2),
            density: Some(8),
            silent: false,
        };
        steps[6] = Step::density(9);
        steps
    };
    let carried = {
        let mut steps = vec![Step::FILLER; 14];
        steps[5] = Step::palette(3);
        steps
    };
    for (name, publisher) in PUBLISHERS {
        let key = GlobalKey::<KeyedState>::new();
        let carried_probes = probes(carried.len());
        let shallow_probes = probes(shallow.len());
        let deep_probes = probes(deep.len());
        let keyed = || {
            Keyed {
                key: key.clone(),
                child: provider_chain(publisher, &carried, &carried_probes, Leaf.boxed()),
            }
            .boxed()
        };
        let view = |under_deep: bool| {
            let (shallow_tail, deep_tail) = if under_deep {
                (Leaf.boxed(), keyed())
            } else {
                (keyed(), Leaf.boxed())
            };
            Group(vec![
                provider_chain(publisher, &shallow, &shallow_probes, shallow_tail),
                provider_chain(publisher, &deep, &deep_probes, deep_tail),
            ])
        };

        let frames = Frames::mount(&view(false));
        expect_chain(
            &shallow,
            &carried,
            &carried_probes,
            &format!("{name}: mounted"),
        );

        frames.update(&view(true));
        expect_chain(
            &deep,
            &carried,
            &carried_probes,
            &format!("{name}: moved deeper"),
        );

        frames.update(&view(false));
        expect_chain(
            &shallow,
            &carried,
            &carried_probes,
            &format!("{name}: moved back"),
        );
        expect_chain(
            &[],
            &deep,
            &deep_probes,
            &format!("{name}: the deep branch"),
        );
    }
}

pub(crate) fn a_provide_panic_is_recorded_and_keeps_the_frame() {
    for (name, publisher) in PUBLISHERS {
        let probe = Seen::default();
        let fail = Rc::new(Cell::new(false));
        let view = |tone| {
            Group(vec![publisher(
                Published {
                    palette: Some(tone),
                    density: None,
                    fail: Some(Rc::clone(&fail)),
                    ..Published::default()
                },
                reader::<Palette>(&probe),
            )])
        };
        let frames = Frames::mount(&view(1));
        assert_eq!(probe.seen.get(), Some(1), "{name}: published");

        fail.set(true);
        frames.update(&view(2));
        let panics = frames.owner.borrow_mut().take_recovered_panics();
        assert_eq!(panics.len(), 1, "{name}: one recorded failure");
        assert_eq!(panics[0].hook, LifecycleHook::Provide, "{name}: its hook");

        // The next frame builds: the failure replaced one element, not the tree.
        fail.set(false);
        let late = Seen::default();
        frames.update(&Group(vec![publisher(
            Published::palette(4),
            reader::<Palette>(&late),
        )]));
        assert_eq!(late.seen.get(), Some(4), "{name}: the frame after");
    }
}

/// A replaced value's `Drop` runs after the change is on record: the retry
/// publishes equal values and still reaches the reader.
pub(crate) fn a_replaced_value_that_panics_in_drop_keeps_its_notification() {
    for (name, publisher) in PUBLISHERS {
        let probe = Seen::default();
        let armed = Rc::new(Cell::new(false));
        let view = |tone| {
            publisher(
                Published {
                    fragile: Some((tone, Rc::clone(&armed))),
                    ..Published::default()
                },
                reader::<Fragile>(&probe),
            )
        };
        let frames = Frames::mount(&view(1));
        assert_eq!(probe.seen.get(), Some(1), "{name}: published");

        armed.set(true);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            frames.update(&view(2));
        }));
        assert!(unwound.is_err(), "{name}: the root's failure unwinds");
        let _ = frames.owner.borrow_mut().take_recovered_panics();
        frames.update(&view(2));
        assert_eq!(probe.seen.get(), Some(2), "{name}: the replacement is read");
        assert_eq!(probe.changes.get(), 1, "{name}: its reader was notified");
    }
}

/// `provide` is part of the element's build: a signal it reads rebuilds the
/// element, and the value derived from it reaches the readers.
pub(crate) fn a_signal_read_in_provide_republishes_on_write() {
    for (name, publisher) in PUBLISHERS {
        let probe = Seen::default();
        let frames = Frames::empty();
        let tone = frames.owner.borrow().signal_graph().signal(1u32);
        let writer = frames.owner.borrow().writer_source();
        let frames = frames.mounted(&publisher(
            Published {
                signal: Some(tone),
                ..Published::default()
            },
            reader::<Palette>(&probe),
        ));
        assert_eq!(probe.seen.get(), Some(1), "{name}: published");

        for next in [2, 3] {
            writer.write(|ctx| tone.set(ctx, next)).expect("live");
            frames.drain();
            assert_eq!(probe.seen.get(), Some(next), "{name}: after the write");
        }
    }
}

/// A view whose `build` panics once after its `provide` ran.
#[derive(Clone)]
struct Brittle {
    armed: Rc<Cell<bool>>,
}

impl StatelessView for Brittle {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        assert!(!self.armed.replace(false), "authored build failure");
        Leaf
    }

    fn provide(&self, _ctx: &dyn BuildContext, _values: &mut Provided<'_>) {
        self.armed.set(true);
    }
}

impl View for Brittle {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// Whatever of the author's fails between `provide` and the end of `build`,
/// the build phase ends with the build: a signal is writable afterwards.
pub(crate) fn a_failure_after_provide_ends_the_build_phase() {
    let frames = Frames::empty();
    let signal = frames.owner.borrow().signal_graph().signal(0u32);
    let writer = frames.owner.borrow().writer_source();
    let armed = Rc::new(Cell::new(false));
    let mounted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        frames.mounted(&Group(vec![
            Brittle {
                armed: Rc::clone(&armed),
            }
            .boxed(),
        ]))
    }));
    assert!(!armed.get(), "the authored failure fired");
    drop(mounted);
    writer
        .write(|ctx| signal.set(ctx, 1))
        .expect("no build is in progress");
}
