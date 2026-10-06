//! Public owner and key envelopes retire without competing with a prior failure.
use flui_foundation::observe::TreeObserver;
use flui_foundation::{ElementId, ViewKey};
use flui_rendering::{
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
};
use flui_view::{
    BuildOwner, ElementTree, GlobalKeyScope, RebuildReason, RenderView, View, ViewExt,
};
use std::{
    any::Any,
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

type Events = Arc<Mutex<Vec<&'static str>>>;
struct Leaf {
    name: &'static str,
    fail: bool,
    events: Events,
}
impl Drop for Leaf {
    fn drop(&mut self) {
        self.events.lock().expect("events").push(self.name);
        assert!(!self.fail, "{} owner leaf failed", self.name);
    }
}
struct Observer {
    _leaf: Leaf,
}
impl TreeObserver for Observer {}

fn panic_text(payload: &dyn std::any::Any) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .expect("string failure")
}

fn physical(kind: &str) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut owner = BuildOwner::new();
    let _signal = owner.reactive().signal(Leaf {
        name: "graph",
        fail: matches!(kind, "graph" | "all"),
        events: events.clone(),
    });
    owner.set_tree_observer(Arc::new(Observer {
        _leaf: Leaf {
            name: "observer",
            fail: matches!(kind, "observer" | "pair" | "all"),
            events: events.clone(),
        },
    }));
    let callback = Leaf {
        name: "callback",
        fail: matches!(kind, "callback" | "pair" | "all"),
        events: events.clone(),
    };
    owner.set_on_build_scheduled(move || {
        let _ = &callback;
    });
    let result = catch_unwind(AssertUnwindSafe(|| {
        if kind == "incoming" {
            let _owner = owner;
            panic!("incoming owner failure");
        }
        drop(owner);
    }));
    let expected: &[&str] = match kind {
        "healthy" | "callback" => &["graph", "observer", "callback"],
        "graph" | "all" => &["graph"],
        "observer" | "pair" => &["graph", "observer"],
        "incoming" => &[],
        _ => panic!("unknown physical row"),
    };
    assert_eq!(&*events.lock().expect("events"), expected);
    if kind == "healthy" {
        assert!(result.is_ok());
    } else {
        let payload = result.expect_err("first owner failure");
        let first = if kind == "incoming" {
            "incoming owner failure"
        } else if matches!(kind, "graph" | "all") {
            "graph owner leaf failed"
        } else if matches!(kind, "observer" | "pair") {
            "observer owner leaf failed"
        } else {
            "callback owner leaf failed"
        };
        assert_eq!(panic_text(payload.as_ref()), first);
    }
    let next = BuildOwner::new();
    let _next_signal = next.reactive().signal(Leaf {
        name: "next",
        fail: false,
        events: events.clone(),
    });
    drop(next);
    assert_eq!(events.lock().expect("events").last(), Some(&"next"));
}

pub(crate) fn dispatch_child(kind: &str) {
    if kind == "physical-key" {
        physical_key(false);
    } else if kind == "physical-key-pair" {
        physical_key(true);
    } else if let Some(kind) = kind.strip_prefix("physical:") {
        physical(kind);
    } else if let Some(kind) = kind.strip_prefix("release:") {
        release(kind);
    } else if let Some(kind) = kind.strip_prefix("clone:") {
        cloning(kind);
    } else if let Some(kind) = kind.strip_prefix("frame:") {
        frame(kind);
    } else if kind == "rollback" {
        rollback();
    } else if kind == "identity" {
        identity();
    } else {
        panic!("unknown owner/key selector");
    }
}
fn bounded(kind: &str) {
    super::child_payload_recovery::run_child("FLUI_OWNER_KEY_RETIREMENT_CHILD", kind);
}
pub(crate) fn physical_healthy_order() {
    bounded("physical:healthy");
}
pub(crate) fn physical_graph_failure() {
    bounded("physical:graph");
}
pub(crate) fn physical_observer_failure() {
    bounded("physical:observer");
}
pub(crate) fn physical_callback_failure() {
    bounded("physical:callback");
}
pub(crate) fn physical_observer_callback_competition() {
    bounded("physical:pair");
}
pub(crate) fn physical_graph_observer_callback_competition() {
    bounded("physical:all");
}
pub(crate) fn physical_incoming_failure() {
    bounded("physical:incoming");
}

thread_local! {
    static SCOPE: RefCell<Option<GlobalKeyScope>> = const { RefCell::new(None) };
    static COMPETITOR: RefCell<Option<BuildOwner>> = const { RefCell::new(None) };
}
struct KeyControl {
    clones: AtomicUsize,
    fail_clone: AtomicUsize,
    fail_local: AtomicBool,
    fail_scope: AtomicBool,
    read_clone: AtomicUsize,
    compete_clone: AtomicUsize,
    reenter_drop: AtomicBool,
    reentries: AtomicUsize,
    fail_local_admission: AtomicBool,
    forbidden_hash: AtomicBool,
    forbidden_eq: AtomicBool,
    frame: AtomicBool,
    verification_phase: AtomicBool,
    fail_storage: AtomicBool,
    fail_storage_at: AtomicUsize,
    fail_verification: AtomicBool,
    storage_drops: AtomicUsize,
    verification_drops: AtomicUsize,
    events: Events,
}
impl KeyControl {
    fn new(events: &Events) -> Arc<Self> {
        Arc::new(Self {
            clones: AtomicUsize::new(0),
            fail_clone: AtomicUsize::new(0),
            fail_local: AtomicBool::new(false),
            fail_scope: AtomicBool::new(false),
            read_clone: AtomicUsize::new(0),
            compete_clone: AtomicUsize::new(0),
            reenter_drop: AtomicBool::new(false),
            reentries: AtomicUsize::new(0),
            fail_local_admission: AtomicBool::new(false),
            forbidden_hash: AtomicBool::new(false),
            forbidden_eq: AtomicBool::new(false),
            frame: AtomicBool::new(false),
            verification_phase: AtomicBool::new(false),
            fail_storage: AtomicBool::new(false),
            fail_storage_at: AtomicUsize::new(0),
            fail_verification: AtomicBool::new(false),
            storage_drops: AtomicUsize::new(0),
            verification_drops: AtomicUsize::new(0),
            events: events.clone(),
        })
    }
}
#[derive(Clone, Copy)]
enum KeyRole {
    Original,
    LocalRegistration,
    ScopedClaim,
    LaterClone,
}
struct TestKey {
    id: u64,
    ordinal: usize,
    role: KeyRole,
    verification: bool,
    control: Arc<KeyControl>,
}
impl TestKey {
    fn new(id: u64, events: &Events) -> Self {
        Self {
            id,
            ordinal: 0,
            role: KeyRole::Original,
            verification: false,
            control: KeyControl::new(events),
        }
    }
}
fn element(slot: u32) -> ElementId {
    ElementId::new_gen(slot, std::num::NonZeroU32::MIN)
}
fn with_scope(f: impl FnOnce(&GlobalKeyScope)) {
    SCOPE.with(|scope| f(scope.borrow().as_ref().expect("installed scope")));
}
fn competitor_claim(id: u64, events: &Events) {
    let key = TestKey::new(id, events);
    COMPETITOR.with(|owner| {
        owner
            .borrow_mut()
            .as_mut()
            .expect("competitor")
            .register_global_key(&key, element(8));
    });
}
impl ViewKey for TestKey {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn key_hash(&self) -> u64 {
        assert!(
            !self.control.forbidden_hash.load(Ordering::SeqCst),
            "rollback key hash called"
        );
        17
    }
    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        assert!(
            !self.control.forbidden_eq.load(Ordering::SeqCst),
            "rollback key equality called"
        );
        let other = other
            .as_any()
            .downcast_ref::<Self>()
            .expect("same key fixture");
        // Only this fixture's local registration clone may query the scope.
        // Scoped comparison receivers never reenter its live scope borrow.
        let mut admitted = false;
        if matches!(self.role, KeyRole::LocalRegistration)
            && self.control.fail_local_admission.load(Ordering::SeqCst)
        {
            with_scope(|scope| admitted = scope.claim_count() == 2);
        }
        if admitted {
            self.control.forbidden_eq.store(true, Ordering::SeqCst);
            other.control.forbidden_hash.store(true, Ordering::SeqCst);
            other.control.forbidden_eq.store(true, Ordering::SeqCst);
            panic!("local admission comparison failed");
        }
        self.id == other.id
    }
    fn clone_key(&self) -> Box<dyn ViewKey> {
        let ordinal = self.control.clones.fetch_add(1, Ordering::SeqCst) + 1;
        assert_ne!(
            self.control.fail_clone.load(Ordering::SeqCst),
            ordinal,
            "key clone {ordinal} failed"
        );
        if self.control.read_clone.load(Ordering::SeqCst) == ordinal {
            with_scope(|scope| assert_eq!(scope.claim_count(), 0));
            self.control.reentries.fetch_add(1, Ordering::SeqCst);
        }
        if self.control.compete_clone.load(Ordering::SeqCst) == ordinal {
            competitor_claim(self.id, &self.control.events);
            self.control.reentries.fetch_add(1, Ordering::SeqCst);
        }
        Box::new(Self {
            id: self.id,
            ordinal,
            role: match ordinal {
                1 => KeyRole::LocalRegistration,
                2 => KeyRole::ScopedClaim,
                _ => KeyRole::LaterClone,
            },
            verification: self.control.verification_phase.load(Ordering::SeqCst),
            control: self.control.clone(),
        })
    }
    fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "test-key {}", self.id)
    }
    fn is_global_key(&self) -> bool {
        true
    }
}
impl Drop for TestKey {
    fn drop(&mut self) {
        if self.ordinal == 0 {
            return;
        }
        if self.control.frame.load(Ordering::SeqCst) {
            let (label, fail) = if self.verification {
                self.control
                    .verification_drops
                    .fetch_add(1, Ordering::SeqCst);
                (
                    "verification",
                    self.control.fail_verification.load(Ordering::SeqCst),
                )
            } else {
                let count = self.control.storage_drops.fetch_add(1, Ordering::SeqCst) + 1;
                (
                    "storage",
                    self.control.fail_storage.load(Ordering::SeqCst)
                        || self.control.fail_storage_at.load(Ordering::SeqCst) == count,
                )
            };
            self.control.events.lock().expect("events").push(label);
            assert!(!fail, "{label} frame key retirement failed");
            return;
        }
        let (label, fail) = if self.ordinal == 1 {
            ("local", self.control.fail_local.load(Ordering::SeqCst))
        } else if self.ordinal == 2 {
            ("scoped", self.control.fail_scope.load(Ordering::SeqCst))
        } else {
            ("next-key", false)
        };
        self.control.events.lock().expect("events").push(label);
        if self.ordinal == 2 && self.control.reenter_drop.load(Ordering::SeqCst) {
            with_scope(|scope| assert_eq!(scope.claim_count(), 0));
            competitor_claim(self.id, &self.control.events);
            self.control.reentries.fetch_add(1, Ordering::SeqCst);
        }
        assert!(!fail, "{label} key retirement failed");
    }
}
fn rig() -> (BuildOwner, GlobalKeyScope, Events) {
    let scope = GlobalKeyScope::new();
    let mut owner = BuildOwner::new();
    owner.set_global_key_scope(scope.clone());
    SCOPE.with(|slot| *slot.borrow_mut() = Some(scope.clone()));
    let mut competitor = BuildOwner::new();
    competitor.set_global_key_scope(scope.clone());
    COMPETITOR.with(|slot| *slot.borrow_mut() = Some(competitor));
    (owner, scope, Arc::new(Mutex::new(Vec::new())))
}
fn finish_rig() {
    COMPETITOR.with(|slot| drop(slot.borrow_mut().take()));
    SCOPE.with(|slot| drop(slot.borrow_mut().take()));
}
fn next_claim(key: &TestKey, scope: &GlobalKeyScope) {
    let mut next = BuildOwner::new();
    next.set_global_key_scope(scope.clone());
    next.register_global_key(key, element(6));
    assert_eq!(next.element_for_global_key(key), Some(element(6)));
    next.unregister_global_key(key);
    assert_eq!(scope.claim_count(), 0);
}
fn release(kind: &str) {
    let (mut owner, scope, events) = rig();
    let key = TestKey::new(1, &events);
    owner.register_global_key(&key, element(1));
    key.control.fail_local.store(
        matches!(kind, "local" | "pair" | "incoming"),
        Ordering::SeqCst,
    );
    key.control.fail_scope.store(
        matches!(kind, "scoped" | "pair" | "incoming"),
        Ordering::SeqCst,
    );
    key.control
        .reenter_drop
        .store(kind == "reentry", Ordering::SeqCst);
    struct Unregister<'a>(&'a mut BuildOwner, &'a TestKey);
    impl Drop for Unregister<'_> {
        fn drop(&mut self) {
            self.0.unregister_global_key(self.1);
        }
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        if kind == "incoming" {
            let _release = Unregister(&mut owner, &key);
            panic!("incoming key failure");
        }
        owner.unregister_global_key(&key);
    }));
    assert_eq!(owner.element_for_global_key(&key), None);
    let expected: &[&str] = match kind {
        "local" | "pair" => &["local"],
        "incoming" => &[],
        "healthy" | "scoped" | "reentry" => &["local", "scoped"],
        _ => panic!("unknown release row"),
    };
    assert_eq!(&*events.lock().expect("events"), expected);
    if matches!(kind, "healthy" | "reentry") {
        assert!(result.is_ok());
    } else {
        let payload = result.expect_err("key failure");
        assert_eq!(
            panic_text(payload.as_ref()),
            match kind {
                "incoming" => "incoming key failure",
                "scoped" => "scoped key retirement failed",
                _ => "local key retirement failed",
            }
        );
    }
    if kind == "reentry" {
        assert_eq!(key.control.reentries.load(Ordering::SeqCst), 1);
        assert_eq!(scope.claim_count(), 1);
        COMPETITOR.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("competitor")
                .unregister_global_key(&key);
        });
    }
    assert_eq!(scope.claim_count(), 0);
    key.control.fail_local.store(false, Ordering::SeqCst);
    key.control.fail_scope.store(false, Ordering::SeqCst);
    next_claim(&key, &scope);
    finish_rig();
}
fn cloning(kind: &str) {
    let (mut owner, scope, events) = rig();
    let key = TestKey::new(1, &events);
    match kind {
        "local" => key.control.fail_clone.store(1, Ordering::SeqCst),
        "scoped" => {
            key.control.fail_clone.store(2, Ordering::SeqCst);
            key.control.fail_local.store(true, Ordering::SeqCst);
        }
        "read" => key.control.read_clone.store(2, Ordering::SeqCst),
        "compete" => key.control.compete_clone.store(2, Ordering::SeqCst),
        "local-compete" => key.control.compete_clone.store(1, Ordering::SeqCst),
        "compete-drop" => {
            key.control.compete_clone.store(2, Ordering::SeqCst);
            key.control.fail_local.store(true, Ordering::SeqCst);
            key.control.fail_scope.store(true, Ordering::SeqCst);
        }
        _ => panic!("unknown clone row"),
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        owner.register_global_key(&key, element(1));
    }));
    if kind == "read" {
        assert!(result.is_ok());
        assert_eq!(key.control.reentries.load(Ordering::SeqCst), 1);
        owner.unregister_global_key(&key);
    } else {
        let payload = result.expect_err("rejected registration");
        if matches!(kind, "local" | "scoped") {
            assert!(panic_text(payload.as_ref()).contains(if kind == "local" {
                "key clone 1 failed"
            } else {
                "key clone 2 failed"
            }));
            assert_eq!(scope.claim_count(), 0);
            assert!(
                events.lock().expect("events").is_empty(),
                "prepared key preserved during clone unwind"
            );
        } else {
            if kind == "compete-drop" {
                assert_eq!(panic_text(payload.as_ref()), "scoped key retirement failed");
                assert_eq!(&*events.lock().expect("events"), &["scoped"]);
            } else {
                assert!(panic_text(payload.as_ref()).contains("already claimed"));
            }
            assert_eq!(scope.claim_count(), 1);
            assert_eq!(key.control.reentries.load(Ordering::SeqCst), 1);
            owner.unregister_global_key(&key); // stale owner cannot release competitor.
            assert_eq!(scope.claim_count(), 1);
            COMPETITOR.with(|slot| {
                slot.borrow_mut()
                    .as_mut()
                    .expect("competitor")
                    .unregister_global_key(&key);
            });
        }
        assert_eq!(owner.element_for_global_key(&key), None);
    }
    key.control.fail_clone.store(0, Ordering::SeqCst);
    key.control.fail_local.store(false, Ordering::SeqCst);
    key.control.fail_scope.store(false, Ordering::SeqCst);
    key.control.compete_clone.store(0, Ordering::SeqCst);
    key.control.read_clone.store(0, Ordering::SeqCst);
    next_claim(&key, &scope);
    finish_rig();
}
fn rollback() {
    let (mut owner, scope, events) = rig();
    let existing = TestKey::new(1, &events);
    owner.register_global_key(&existing, element(1));
    // Fail only the local comparison after public scope authority contains
    // both the existing and incoming claims, independently of lookup count.
    existing
        .control
        .fail_local_admission
        .store(true, Ordering::SeqCst);
    let incoming = TestKey::new(2, &events);
    incoming.control.fail_local.store(true, Ordering::SeqCst);
    incoming.control.fail_scope.store(true, Ordering::SeqCst);
    let result = catch_unwind(AssertUnwindSafe(|| {
        owner.register_global_key(&incoming, element(2));
    }));
    let payload = result.expect_err("post-admission comparison fails");
    assert_eq!(
        panic_text(payload.as_ref()),
        "local admission comparison failed"
    );
    assert!(
        events.lock().expect("events").is_empty(),
        "incoming failure retains both prepared key envelopes"
    );
    existing.control.forbidden_eq.store(false, Ordering::SeqCst);
    existing
        .control
        .fail_local_admission
        .store(false, Ordering::SeqCst);
    incoming
        .control
        .forbidden_hash
        .store(false, Ordering::SeqCst);
    incoming.control.forbidden_eq.store(false, Ordering::SeqCst);
    incoming.control.fail_local.store(false, Ordering::SeqCst);
    incoming.control.fail_scope.store(false, Ordering::SeqCst);
    assert_eq!(scope.claim_count(), 1);
    assert_eq!(owner.element_for_global_key(&incoming), None);
    assert_eq!(owner.element_for_global_key(&existing), Some(element(1)));
    owner.unregister_global_key(&existing);
    next_claim(&incoming, &scope);
    finish_rig();
}
fn identity() {
    let (mut owner, scope, events) = rig();
    let first = TestKey::new(1, &events);
    let second = TestKey::new(2, &events);
    owner.register_global_key(&first, element(1));
    let admitted_clones = first.control.clones.load(Ordering::SeqCst);
    owner.register_global_key(&first, element(2));
    assert_eq!(
        first.control.clones.load(Ordering::SeqCst),
        admitted_clones,
        "same-owner admission clones neither key"
    );
    owner.register_global_key(&second, element(3));
    assert_eq!(owner.element_for_global_key(&first), Some(element(2)));
    assert_eq!(owner.element_for_global_key(&second), Some(element(3)));
    assert_eq!(scope.claim_count(), 2);
    owner.unregister_global_key(&first);
    competitor_claim(first.id, &events);
    owner.unregister_global_key(&first);
    assert_eq!(
        scope.claim_count(),
        2,
        "stale release preserves replacement and collision"
    );
    COMPETITOR.with(|slot| {
        let slot = slot.borrow();
        let competitor = slot.as_ref().expect("competitor");
        assert_eq!(competitor.element_for_global_key(&first), Some(element(8)));
    });
    owner.unregister_global_key(&second);
    finish_rig();
    assert_eq!(scope.claim_count(), 0);
    next_claim(&first, &scope);
}
fn physical_key(pair: bool) {
    let (mut owner, scope, events) = rig();
    let first = TestKey::new(1, &events);
    let second = TestKey::new(2, &events);
    owner.register_global_key(&first, element(1));
    if pair {
        owner.register_global_key(&second, element(2));
    }
    first.control.fail_local.store(true, Ordering::SeqCst);
    second.control.fail_local.store(true, Ordering::SeqCst);
    owner.set_tree_observer(Arc::new(Observer {
        _leaf: Leaf {
            name: "observer",
            fail: true,
            events: events.clone(),
        },
    }));
    let result = catch_unwind(AssertUnwindSafe(|| drop(owner)));
    assert_eq!(
        panic_text(result.expect_err("first key failure").as_ref()),
        "local key retirement failed"
    );
    let expected: &[&str] = if pair {
        &["scoped", "scoped", "local"]
    } else {
        &["scoped", "local"]
    };
    assert_eq!(&*events.lock().expect("events"), expected);
    assert_eq!(scope.claim_count(), 0);
    first.control.fail_local.store(false, Ordering::SeqCst);
    next_claim(&first, &scope);
    finish_rig();
}
pub(crate) fn physical_key_observer_competition() {
    bounded("physical-key");
}
pub(crate) fn physical_registry_key_competition() {
    bounded("physical-key-pair");
}
pub(crate) fn key_release_healthy_order() {
    bounded("release:healthy");
}
pub(crate) fn key_release_local_failure() {
    bounded("release:local");
}
pub(crate) fn key_release_scope_failure() {
    bounded("release:scoped");
}
pub(crate) fn key_release_competition() {
    bounded("release:pair");
}
pub(crate) fn key_release_incoming_failure() {
    bounded("release:incoming");
}
pub(crate) fn key_release_reentry() {
    bounded("release:reentry");
}
pub(crate) fn local_clone_failure() {
    bounded("clone:local");
}
pub(crate) fn scope_clone_failure() {
    bounded("clone:scoped");
}
pub(crate) fn scope_clone_read_reentry() {
    bounded("clone:read");
}
pub(crate) fn scope_clone_competing_owner_reentry() {
    bounded("clone:compete");
}
pub(crate) fn rollback_invokes_no_key_callbacks() {
    bounded("rollback");
}
pub(crate) fn key_collision_same_owner_and_stale_release() {
    bounded("identity");
}

#[derive(Clone)]
struct FrameLeaf(Arc<TestKey>);
impl View for FrameLeaf {
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(self.0.as_ref())
    }
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}
impl RenderView for FrameLeaf {
    type Protocol = BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;
    fn create_render_object(&self, _: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        flui_objects::RenderSizedBox::shrink()
    }
    fn update_render_object(
        &self,
        _: &flui_view::RenderObjectContext<'_>,
        _: &mut Self::RenderObject,
    ) -> flui_view::RenderUpdateImpact {
        flui_view::RenderUpdateImpact::NONE
    }
}
fn row(keys: &[Arc<TestKey>]) -> super::dense_reconcile_containment::DenseRow {
    super::dense_reconcile_containment::DenseRow {
        children: keys
            .iter()
            .map(|key| FrameLeaf(Arc::clone(key)).boxed())
            .collect(),
    }
}
fn reset_frame(key: &TestKey) {
    key.control.storage_drops.store(0, Ordering::SeqCst);
    key.control.verification_drops.store(0, Ordering::SeqCst);
    key.control.verification_phase.store(true, Ordering::SeqCst);
}
fn disarm_frame(key: &TestKey) {
    key.control.fail_storage.store(false, Ordering::SeqCst);
    key.control.fail_storage_at.store(0, Ordering::SeqCst);
    key.control.fail_verification.store(false, Ordering::SeqCst);
    key.control
        .verification_phase
        .store(false, Ordering::SeqCst);
}
fn frame(kind: &str) {
    let (mut owner, scope, events) = rig();
    let keys = [
        Arc::new(TestKey::new(1, &events)),
        Arc::new(TestKey::new(2, &events)),
    ];
    for key in &keys {
        key.control.frame.store(true, Ordering::SeqCst);
    }
    let displaced = kind.starts_with("displacement");
    let initial = if displaced {
        super::dense_reconcile_containment::DenseRow {
            children: vec![row(&keys).boxed(), row(&[]).boxed()],
        }
    } else {
        row(&keys)
    };
    let mut tree = ElementTree::new();
    let parent = tree.mount_root_with_pipeline_owner(
        &initial,
        Some(PipelineCell::new(PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ))),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    let mut rebuild = parent;
    if displaced {
        owner.finalize_tree(&mut tree);
        let parents = tree.get(parent).expect("root").child_ids().to_vec();
        rebuild = parents[1];
        tree.update(rebuild, &row(&keys), &mut owner.element_owner_mut());
        owner.schedule_build_for(rebuild, 1, RebuildReason::ParentUpdate);
        owner.build_scope(&mut tree);
    }
    let children = tree.get(rebuild).expect("parent").child_ids().to_vec();
    assert_eq!(children.len(), 2);
    events.lock().expect("events").clear();
    for key in &keys {
        reset_frame(key);
    }
    match kind {
        "storage" | "pair" | "incoming" | "physical" => {
            for key in &keys {
                key.control.fail_storage.store(true, Ordering::SeqCst);
            }
        }
        "displacement" | "displacement-pair" => {
            for key in &keys {
                key.control.fail_storage_at.store(2, Ordering::SeqCst);
            }
        }
        "healthy" | "verification" | "displacement-healthy" => {}
        _ => panic!("unknown frame row"),
    }
    if matches!(
        kind,
        "verification" | "pair" | "incoming" | "displacement-pair"
    ) {
        for key in &keys {
            key.control.fail_verification.store(true, Ordering::SeqCst);
        }
    }
    if kind == "physical" {
        let result = catch_unwind(AssertUnwindSafe(|| drop(owner)));
        assert_eq!(
            panic_text(result.expect_err("scoped key failure").as_ref()),
            "storage frame key retirement failed"
        );
        assert_eq!(&*events.lock().expect("events"), &["storage"]);
        assert_eq!(scope.claim_count(), 0);
        for key in &keys {
            disarm_frame(key);
        }
        next_claim(&keys[0], &scope);
        drop(tree);
        finish_rig();
        return;
    }
    struct Finalize<'a>(&'a mut BuildOwner, &'a mut ElementTree);
    impl Drop for Finalize<'_> {
        fn drop(&mut self) {
            self.0.finalize_tree(self.1);
        }
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        if kind == "incoming" {
            let _finalize = Finalize(&mut owner, &mut tree);
            panic!("incoming frame failure");
        }
        owner.finalize_tree(&mut tree);
    }));
    if matches!(kind, "healthy" | "displacement-healthy") {
        assert!(result.is_ok());
    } else {
        assert_eq!(
            panic_text(result.expect_err("frame retirement failure").as_ref()),
            match kind {
                "incoming" => "incoming frame failure",
                "verification" => "verification frame key retirement failed",
                _ => "storage frame key retirement failed",
            }
        );
    }
    let expected: &[&str] = match kind {
        "healthy" => &["storage", "storage", "verification", "verification"],
        "storage" | "pair" => &["storage"],
        "verification" => &["storage", "storage", "verification"],
        "incoming" => &[],
        "displacement" | "displacement-pair" => &["storage", "storage", "storage"],
        "displacement-healthy" => &[
            "storage",
            "storage",
            "storage",
            "storage",
            "verification",
            "verification",
        ],
        _ => panic!("unknown frame expectation"),
    };
    assert_eq!(&*events.lock().expect("events"), expected);
    assert_eq!(
        tree.get(rebuild).expect("parent").child_ids(),
        children.as_slice()
    );
    for (key, &child) in keys.iter().zip(&children) {
        assert!(tree.contains(child));
        assert_eq!(owner.element_for_global_key(key.as_ref()), Some(child));
        disarm_frame(key);
    }
    owner.schedule_build_for(rebuild, usize::from(displaced), RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);
    owner.finalize_tree(&mut tree);
    assert_eq!(
        tree.get(rebuild).expect("next parent").child_ids(),
        children.as_slice()
    );
    drop(owner);
    assert_eq!(scope.claim_count(), 0);
    next_claim(&keys[0], &scope);
    drop(tree);
    finish_rig();
}
pub(crate) fn reservation_verification_healthy_order() {
    bounded("frame:healthy");
}
pub(crate) fn reservation_storage_failure() {
    bounded("frame:storage");
}
pub(crate) fn verification_key_failure() {
    bounded("frame:verification");
}
pub(crate) fn reservation_verification_competition() {
    bounded("frame:pair");
}
pub(crate) fn reservation_verification_incoming_failure() {
    bounded("frame:incoming");
}
pub(crate) fn owner_scope_failure_retains_reservations() {
    bounded("frame:physical");
}
pub(crate) fn displacement_key_competition() {
    bounded("frame:displacement");
}
pub(crate) fn displacement_verification_competition() {
    bounded("frame:displacement-pair");
}
pub(crate) fn displacement_verification_healthy_order() {
    bounded("frame:displacement-healthy");
}

pub(crate) fn local_clone_competing_owner_reentry() {
    bounded("clone:local-compete");
}
pub(crate) fn competing_owner_and_rejected_key_retirement() {
    bounded("clone:compete-drop");
}
