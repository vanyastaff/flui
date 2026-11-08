//! Public-API regression tests for dynamically typed inherited entries.

#![expect(clippy::arc_with_non_send_sync)]

use std::{
    any::TypeId,
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use flui_foundation::ViewKey;
use flui_view::{
    BoxedView, BuildContext, BuildContextExt, BuildOwner, ElementBuildContext, ElementTree,
    EntrySet, ErrorView, FieldMask, GlobalKey, InheritedData, InheritedElement, InheritedView,
    IntoView, LifecycleContext, RebuildReason, RenderView, StatefulView, StatelessView, View,
    ViewExt, ViewState,
};

struct Alpha;
struct Beta;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EntryData {
    alpha: Option<u32>,
    beta: Option<u32>,
    base: u32,
}

impl EntryData {
    const FIELD_BASE: FieldMask<Self> = FieldMask::bit(0);
}

impl InheritedData for EntryData {
    fn field_mask_diff(&self, old: &Self) -> FieldMask<Self> {
        if self.base == old.base {
            FieldMask::NONE
        } else {
            Self::FIELD_BASE
        }
    }
}

fn data(alpha: Option<u32>, beta: Option<u32>, base: u32) -> EntryData {
    EntryData { alpha, beta, base }
}

#[derive(Clone)]
struct EntryProvider {
    data: EntryData,
    child: BoxedView,
}

impl EntryProvider {
    fn new(data: EntryData, child: BoxedView) -> Self {
        Self { data, child }
    }
}

impl InheritedView for EntryProvider {
    type Data = EntryData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.data != old.data
    }

    fn changed_fields(&self, old: &Self) -> FieldMask<Self::Data> {
        self.data.field_mask_diff(&old.data)
    }

    fn changed_entries(&self, old: &Self) -> EntrySet {
        let mut changed = EntrySet::NONE;
        if self.data.alpha != old.data.alpha {
            changed |= EntrySet::of::<Alpha>();
        }
        if self.data.beta != old.data.beta {
            changed |= EntrySet::of::<Beta>();
        }
        changed
    }
}

impl View for EntryProvider {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::inherited(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadMode {
    Alpha,
    Beta,
    BaseField,
    Whole,
    None,
}

#[derive(Clone)]
struct EntryReader {
    mode: ReadMode,
    key: Option<GlobalKey<()>>,
    builds: Rc<Cell<usize>>,
    observed: Rc<RefCell<Vec<EntryData>>>,
    panic_in_callback: Rc<Cell<bool>>,
    panic_after_read: Rc<Cell<bool>>,
}

impl EntryReader {
    fn new(mode: ReadMode) -> Self {
        Self {
            mode,
            key: None,
            builds: Rc::new(Cell::new(0)),
            observed: Rc::new(RefCell::new(Vec::new())),
            panic_in_callback: Rc::new(Cell::new(false)),
            panic_after_read: Rc::new(Cell::new(false)),
        }
    }

    fn keyed(mode: ReadMode, key: GlobalKey<()>) -> Self {
        let mut reader = Self::new(mode);
        reader.key = Some(key);
        reader
    }
}

impl StatelessView for EntryReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let read = match self.mode {
            ReadMode::Alpha => {
                let panic = Rc::clone(&self.panic_in_callback);
                ctx.depend_on_entry::<EntryProvider, Alpha, _>(move |provider| {
                    let data = *provider.data();
                    assert!(
                        !panic.replace(false),
                        "induced inherited-entry callback panic"
                    );
                    data
                })
            }
            ReadMode::Beta => {
                ctx.depend_on_entry::<EntryProvider, Beta, _>(|provider| *provider.data())
            }
            ReadMode::BaseField => ctx
                .depend_on_field::<EntryProvider, _>(EntryData::FIELD_BASE, |provider| {
                    *provider.data()
                }),
            ReadMode::Whole => ctx.depend_on::<EntryProvider, _>(|provider| *provider.data()),
            ReadMode::None => None,
        };
        if let Some(data) = read {
            self.observed.borrow_mut().push(data);
        }
        assert!(
            !self.panic_after_read.replace(false),
            "induced build panic after inherited-entry read"
        );
        ErrorView::new("terminal leaf")
    }
}

impl View for EntryReader {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_ref().map(|key| key as &dyn ViewKey)
    }
}

fn mount_child(
    initial: EntryData,
    child: BoxedView,
) -> (
    ElementTree,
    BuildOwner,
    flui_view::ElementId,
    flui_view::ElementId,
) {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let provider = tree.mount_root(
        &EntryProvider::new(initial, child),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(provider, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    let reader = tree
        .get(provider)
        .expect("the provider remains mounted")
        .child_ids()[0];
    (tree, owner, provider, reader)
}

struct ReaderHarness {
    tree: ElementTree,
    owner: BuildOwner,
    provider: flui_view::ElementId,
    reader: flui_view::ElementId,
    view: EntryReader,
    child: BoxedView,
}

impl ReaderHarness {
    fn new(mode: ReadMode) -> Self {
        let view = EntryReader::new(mode);
        let child = view.clone().boxed();
        let (tree, owner, provider, reader) = mount_child(EntryData::default(), child.clone());
        Self {
            tree,
            owner,
            provider,
            reader,
            view,
            child,
        }
    }

    fn update_data(&mut self, data: EntryData, expect_rebuild: bool) {
        let before = self.view.builds.get();
        self.tree.update(
            self.provider,
            &EntryProvider::new(data, self.child.clone()),
            &mut self.owner.element_owner_mut(),
        );
        assert_eq!(
            self.owner.dirty_count(),
            usize::from(expect_rebuild),
            "unexpected scheduling for {data:?} with {:?}",
            self.view.mode,
        );
        if expect_rebuild {
            self.owner.build_scope(&mut self.tree);
        }
        assert_eq!(
            self.view.builds.get(),
            before + usize::from(expect_rebuild),
            "unexpected build count for {data:?} with {:?}",
            self.view.mode,
        );
        assert_eq!(self.owner.dirty_count(), 0);
        if expect_rebuild && self.view.mode != ReadMode::None {
            assert_eq!(self.view.observed.borrow().last(), Some(&data));
        }
    }

    fn rebuild_with_mode(&mut self, mode: ReadMode) {
        self.view.mode = mode;
        self.child = self.view.clone().boxed();
        self.tree
            .update(self.reader, &self.view, &mut self.owner.element_owner_mut());
        let depth = self
            .tree
            .get(self.reader)
            .expect("the reader remains mounted")
            .depth();
        self.owner
            .schedule_build_for(self.reader, depth, RebuildReason::ParentUpdate);
        self.owner.build_scope(&mut self.tree);
    }
}

#[test]
fn entry_set_is_a_typed_set_with_an_all_form() {
    let alpha = EntrySet::of::<Alpha>();
    let beta = EntrySet::from_type_id(TypeId::of::<Beta>());
    let both = alpha.clone().union(beta.clone());

    assert_eq!(alpha, EntrySet::from_type_id(TypeId::of::<Alpha>()));
    assert!(alpha.intersects(&alpha));
    assert!(!alpha.intersects(&beta));
    assert!(both.intersects(&alpha));
    assert!(both.intersects(&beta));
    assert!(EntrySet::ALL.intersects(&alpha));
    assert!(!EntrySet::ALL.intersects(&EntrySet::NONE));
    assert!(EntrySet::NONE.is_empty());
    assert!(!both.is_empty());
}

#[test]
fn exact_entry_reads_cover_add_remove_replace_and_equal_updates() {
    let mut harness = ReaderHarness::new(ReadMode::Alpha);

    harness.update_data(data(Some(1), None, 0), true);
    harness.update_data(data(Some(1), None, 0), false);
    harness.update_data(data(Some(2), None, 0), true);
    harness.update_data(data(None, None, 0), true);
    harness.update_data(data(None, Some(1), 0), false);
    harness.update_data(data(None, None, 7), false);

    assert_eq!(harness.view.builds.get(), 4);
    assert_eq!(
        harness.view.observed.borrow().as_slice(),
        &[
            EntryData::default(),
            data(Some(1), None, 0),
            data(Some(2), None, 0),
            data(None, None, 0),
        ]
    );
}

#[test]
fn whole_reads_include_all_entries_and_fields_but_field_reads_exclude_entries() {
    let mut whole = ReaderHarness::new(ReadMode::Whole);
    whole.update_data(data(None, Some(1), 0), true);
    whole.update_data(data(None, Some(1), 0), false);
    whole.update_data(data(None, Some(1), 3), true);
    whole.update_data(data(None, None, 3), true);

    let mut base = ReaderHarness::new(ReadMode::BaseField);
    base.update_data(data(Some(1), None, 0), false);
    base.update_data(data(Some(1), Some(1), 0), false);
    base.update_data(data(Some(1), Some(1), 4), true);
    base.update_data(data(None, None, 4), false);

    assert_eq!(whole.view.builds.get(), 4);
    assert_eq!(base.view.builds.get(), 2);
}

#[test]
fn successful_builds_replace_and_prune_entry_reads() {
    let mut harness = ReaderHarness::new(ReadMode::Alpha);

    harness.rebuild_with_mode(ReadMode::Beta);
    harness.update_data(data(Some(1), None, 0), false);
    harness.update_data(data(Some(1), Some(1), 0), true);

    harness.rebuild_with_mode(ReadMode::None);
    {
        let provider = harness
            .tree
            .get(harness.provider)
            .expect("the provider remains mounted")
            .element()
            .downcast_ref::<InheritedElement<EntryProvider>>()
            .expect("the provider keeps its inherited element type");
        assert!(!provider.dependents().contains_key(&harness.reader));
    }
    harness.update_data(data(Some(1), Some(2), 0), false);
}

#[derive(Clone)]
struct LifecycleReader {
    inits: Rc<Cell<usize>>,
    changes: Rc<Cell<usize>>,
    builds: Rc<Cell<usize>>,
    seen: Rc<Cell<Option<u32>>>,
}

struct LifecycleState {
    inits: Rc<Cell<usize>>,
    changes: Rc<Cell<usize>>,
    builds: Rc<Cell<usize>>,
    seen: Rc<Cell<Option<u32>>>,
}

impl LifecycleReader {
    fn new() -> Self {
        Self {
            inits: Rc::new(Cell::new(0)),
            changes: Rc::new(Cell::new(0)),
            builds: Rc::new(Cell::new(0)),
            seen: Rc::new(Cell::new(None)),
        }
    }
}

impl StatefulView for LifecycleReader {
    type State = LifecycleState;

    fn create_state(&self) -> Self::State {
        LifecycleState {
            inits: Rc::clone(&self.inits),
            changes: Rc::clone(&self.changes),
            builds: Rc::clone(&self.builds),
            seen: Rc::clone(&self.seen),
        }
    }
}

impl ViewState<LifecycleReader> for LifecycleState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.inits.set(self.inits.get() + 1);
        let value = ctx
            .depend_on_entry::<EntryProvider, Alpha, _>(|provider| provider.data().alpha)
            .expect("the lifecycle reader has an entry provider");
        self.seen.set(value);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.changes.set(self.changes.get() + 1);
        let value = ctx
            .depend_on_entry::<EntryProvider, Alpha, _>(|provider| provider.data().alpha)
            .expect("the lifecycle reader still has an entry provider");
        self.seen.set(value);
    }

    fn build(&self, _view: &LifecycleReader, _ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        ErrorView::new("terminal leaf")
    }
}

impl View for LifecycleReader {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

#[test]
fn lifecycle_entry_reads_survive_builds_that_do_not_reread_them() {
    let reader = LifecycleReader::new();
    let child = reader.clone().boxed();
    let (mut tree, mut owner, provider, _) = mount_child(EntryData::default(), child.clone());

    for (next, expect_rebuild, changes, builds, seen) in [
        (data(None, Some(1), 0), false, 0, 1, None),
        (data(Some(1), Some(1), 0), true, 1, 2, Some(1)),
        (data(Some(2), Some(1), 0), true, 2, 3, Some(2)),
    ] {
        tree.update(
            provider,
            &EntryProvider::new(next, child.clone()),
            &mut owner.element_owner_mut(),
        );
        assert_eq!(owner.dirty_count(), usize::from(expect_rebuild));
        if expect_rebuild {
            owner.build_scope(&mut tree);
        }
        assert_eq!(reader.changes.get(), changes);
        assert_eq!(reader.builds.get(), builds);
        assert_eq!(reader.seen.get(), seen);
    }
    assert_eq!(reader.inits.get(), 1);
}

#[test]
fn direct_context_callback_panic_keeps_the_entry_registration() {
    let tree = std::sync::Arc::new(parking_lot::RwLock::new(ElementTree::new()));
    let owner = std::sync::Arc::new(parking_lot::RwLock::new(BuildOwner::new()));
    let reader = EntryReader::new(ReadMode::None);
    let child = reader.clone().boxed();
    let provider = tree.write().mount_root(
        &EntryProvider::new(EntryData::default(), child.clone()),
        &mut owner.write().element_owner_mut(),
    );
    owner
        .write()
        .schedule_build_for(provider, 0, RebuildReason::InitialMount);
    owner.write().build_scope(&mut tree.write());
    let reader_id = tree
        .read()
        .get(provider)
        .expect("the provider remains mounted")
        .child_ids()[0];
    let context = ElementBuildContext::for_element(reader_id, tree.clone(), owner.clone())
        .expect("the direct reader remains mounted");

    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = context.depend_on_entry::<EntryProvider, Alpha, ()>(|_| {
            panic!("induced direct-context entry callback panic");
        });
    }))
    .expect_err("the direct entry callback panics");
    assert_eq!(
        panic.downcast_ref::<&str>().copied(),
        Some("induced direct-context entry callback panic")
    );

    tree.write().update(
        provider,
        &EntryProvider::new(data(Some(5), None, 0), child),
        &mut owner.write().element_owner_mut(),
    );
    assert_eq!(owner.read().dirty_count(), 1);
    owner.write().build_scope(&mut tree.write());
    assert_eq!(reader.builds.get(), 2);
    assert!(owner.write().take_recovered_panics().is_empty());
}

#[test]
fn build_panic_after_entry_read_keeps_recovery_subscription() {
    let mut harness = ReaderHarness::new(ReadMode::Alpha);
    harness.view.panic_after_read.set(true);
    harness.rebuild_with_mode(ReadMode::Alpha);

    assert_eq!(harness.owner.take_recovered_panics().len(), 1);
    assert_eq!(harness.view.builds.get(), 2);
    harness.update_data(data(Some(9), None, 0), true);
    assert_eq!(harness.view.builds.get(), 3);
    assert_eq!(
        harness.view.observed.borrow().last(),
        Some(&data(Some(9), None, 0))
    );
}

#[test]
fn entry_callback_panic_during_build_recovers_on_the_next_entry_change() {
    let mut harness = ReaderHarness::new(ReadMode::Alpha);
    harness.view.panic_in_callback.set(true);
    harness.rebuild_with_mode(ReadMode::Alpha);

    assert_eq!(harness.owner.take_recovered_panics().len(), 1);
    harness.update_data(data(Some(11), None, 0), true);
    assert_eq!(harness.view.builds.get(), 3);
    assert_eq!(
        harness.view.observed.borrow().last(),
        Some(&data(Some(11), None, 0))
    );
}

#[test]
fn missed_entry_read_does_not_invoke_the_callback() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let reader = EntryReader::new(ReadMode::None);
    let root = tree.mount_root(&reader, &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let tree = std::sync::Arc::new(parking_lot::RwLock::new(tree));
    let owner = std::sync::Arc::new(parking_lot::RwLock::new(owner));
    let context = ElementBuildContext::for_element(root, tree, owner)
        .expect("the miss reader remains mounted");
    let called = Rc::new(Cell::new(false));
    let callback_flag = Rc::clone(&called);
    let found = context.depend_on_entry::<EntryProvider, Alpha, _>(move |_| {
        callback_flag.set(true);
    });
    assert!(found.is_none());
    assert!(!called.get());
}

#[derive(Clone)]
struct EntryHost {
    children: Vec<BoxedView>,
}

impl RenderView for EntryHost {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        flui_objects::RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        for child in &self.children {
            visitor(child);
        }
    }
}

impl View for EntryHost {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[test]
fn entry_read_reroutes_after_global_key_reparent() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let reader = EntryReader::keyed(ReadMode::Alpha, GlobalKey::new());
    let initial_host = EntryHost {
        children: vec![
            EntryProvider::new(data(Some(1), None, 0), reader.clone().boxed()).boxed(),
            EntryProvider::new(
                data(Some(2), None, 0),
                ErrorView::new("terminal leaf").boxed(),
            )
            .boxed(),
        ],
    };
    let root = tree.mount_root_with_pipeline_owner(
        &flui_view::RootRenderView::new(initial_host, 800.0, 600.0),
        Some(flui_rendering::pipeline::PipelineCell::new(
            flui_rendering::pipeline::PipelineOwner::new(
                flui_rendering::TextContextHandle::standalone(),
            ),
        )),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let host = tree
        .get(root)
        .expect("the root remains mounted")
        .child_ids()[0];
    let providers = tree
        .get(host)
        .expect("the host remains mounted")
        .child_ids();
    assert_eq!(providers.len(), 2);
    let (provider_a, provider_b) = (providers[0], providers[1]);
    let reader_id = tree
        .get(provider_a)
        .expect("provider A remains mounted")
        .child_ids()[0];
    assert_eq!(reader.builds.get(), 1);
    assert_eq!(
        reader.observed.borrow().as_slice(),
        &[data(Some(1), None, 0)]
    );

    let moved_host = EntryHost {
        children: vec![
            EntryProvider::new(
                data(Some(1), None, 0),
                ErrorView::new("terminal leaf").boxed(),
            )
            .boxed(),
            EntryProvider::new(data(Some(2), None, 0), reader.clone().boxed()).boxed(),
        ],
    };
    tree.update(host, &moved_host, &mut owner.element_owner_mut());
    let host_depth = tree.get(host).expect("the host remains mounted").depth();
    owner.schedule_build_for(host, host_depth, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);

    assert_eq!(
        tree.get(provider_b)
            .expect("provider B remains mounted")
            .child_ids(),
        &[reader_id],
        "the GlobalKey reparent preserves the reader's ElementId"
    );
    assert_eq!(reader.builds.get(), 2);
    assert_eq!(
        reader.observed.borrow().last(),
        Some(&data(Some(2), None, 0)),
        "the reactivated reader resolves provider B instead of its former provider"
    );
    let provider_a_element = tree
        .get(provider_a)
        .expect("provider A remains mounted")
        .element()
        .downcast_ref::<InheritedElement<EntryProvider>>()
        .expect("provider A keeps its inherited element type");
    let provider_b_element = tree
        .get(provider_b)
        .expect("provider B remains mounted")
        .element()
        .downcast_ref::<InheritedElement<EntryProvider>>()
        .expect("provider B keeps its inherited element type");
    assert!(!provider_a_element.dependents().contains_key(&reader_id));
    assert!(provider_b_element.dependents().contains_key(&reader_id));

    tree.update(
        provider_a,
        &EntryProvider::new(
            data(Some(3), None, 0),
            ErrorView::new("terminal leaf").boxed(),
        ),
        &mut owner.element_owner_mut(),
    );
    assert_eq!(owner.dirty_count(), 0);
    assert_eq!(reader.builds.get(), 2);
    assert_eq!(
        reader.observed.borrow().last(),
        Some(&data(Some(2), None, 0)),
        "changing the former provider does not rebuild the moved reader"
    );

    tree.update(
        provider_b,
        &EntryProvider::new(data(Some(4), None, 0), reader.clone().boxed()),
        &mut owner.element_owner_mut(),
    );
    assert_eq!(owner.dirty_count(), 1);
    owner.build_scope(&mut tree);
    assert_eq!(reader.builds.get(), 3);
    assert_eq!(
        reader.observed.borrow().last(),
        Some(&data(Some(4), None, 0)),
        "changing the new entry rebuilds the moved reader exactly once"
    );
    assert_eq!(owner.dirty_count(), 0);
}
