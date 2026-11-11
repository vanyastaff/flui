//! A value an element publishes to its subtree behaves as an inherited view
//! does (ADR-0322 §5): each case runs over an `InheritedView`, a stateless
//! view that publishes and a state that publishes.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

use flui_foundation::{ElementId, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BoxedView, BuildContext, BuildContextExt, BuildOwner, ElementTree, GlobalKey, InheritedView,
    IntoView, LifecycleContext, LifecycleHook, Provided, ProvidedValue, RebuildReason, RenderView,
    StatefulView, StatelessView, View, ViewExt, ViewState,
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
}

impl Published {
    fn palette(tone: u32) -> Self {
        Self {
            palette: Some(tone),
            ..Self::default()
        }
    }

    fn write(&self, values: &mut Provided<'_>) {
        if let Some(fail) = &self.fail {
            assert!(!fail.get(), "authored provide failure");
        }
        if let Some(tone) = self.palette {
            values.value(Palette(tone));
        }
        if let Some(density) = self.density {
            values.value(Density(density));
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
    fn create_element(&self) -> flui_view::element::ElementKind {
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

    fn provide(&self, _ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        self.published.write(values);
    }
}

impl View for Plain {
    fn create_element(&self) -> flui_view::element::ElementKind {
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

    fn create_state(&self) -> KeptState {
        KeptState
    }
}

impl ViewState<Kept> for KeptState {
    fn build(&self, view: &Kept, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }

    fn provide(&self, view: &Kept, _ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        view.published.write(values);
    }
}

impl View for Kept {
    fn create_element(&self) -> flui_view::element::ElementKind {
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

    fn create_state(&self) -> ReaderState {
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
    fn create_element(&self) -> flui_view::element::ElementKind {
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
    fn create_element(&self) -> flui_view::element::ElementKind {
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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// Carries its child to wherever its key is built next.
#[derive(Clone)]
struct Keyed {
    key: GlobalKey<KeyedState>,
    child: BoxedView,
}

struct KeyedState;

impl StatefulView for Keyed {
    type State = KeyedState;

    fn create_state(&self) -> KeyedState {
        KeyedState
    }
}

impl ViewState<Keyed> for KeyedState {
    fn build(&self, view: &Keyed, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}

impl View for Keyed {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
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

impl Frames {
    fn mount(view: &dyn View) -> Self {
        let tree = Rc::new(RefCell::new(ElementTree::new()));
        let owner = Rc::new(RefCell::new(BuildOwner::new()));
        flui_view::test_only_set_global_key_registry(&tree, &owner);
        let root = tree
            .borrow_mut()
            .mount_root(view, &mut owner.borrow_mut().element_owner_mut());
        let frames = Self { tree, owner, root };
        frames.build(RebuildReason::InitialMount);
        frames
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
                    fail: None,
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
            fail: None,
        }));
        assert_eq!(palette.seen.get(), Some(1), "{name}: falls to the outer");
        assert_eq!(density.seen.get(), Some(9), "{name}: finds the inner");
        assert_eq!(palette.changes.get(), 1, "{name}: the palette reader woke");
        assert_eq!(density.changes.get(), 1, "{name}: the density reader woke");

        // A change of one published type wakes the readers of that type only.
        frames.update(&view(Published {
            palette: None,
            density: Some(10),
            fail: None,
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
