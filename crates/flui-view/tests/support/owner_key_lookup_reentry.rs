//! Public custom keys may query or mutate another owner sharing their scope.
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
    cell::{Cell, RefCell},
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

thread_local! {
    static SCOPE: RefCell<Option<GlobalKeyScope>> = const { RefCell::new(None) };
    static PEERS: RefCell<Vec<(BuildOwner, Arc<Key>)>> = const { RefCell::new(Vec::new()) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

const READ: usize = 1;
const TOGGLE: usize = 2;
const INSERT_EQUAL: usize = 3;
const REPLACE: usize = 4;
const DROP_AND_PANIC: usize = 5;
const DROP_ONLY: usize = 6;
const FAIL_EQ: usize = 7;
const REPLACE_NEIGHBOR: usize = 8;

type Events = Arc<Mutex<Vec<(u64, usize)>>>;
struct Control {
    action: AtomicUsize,
    limit: AtomicUsize,
    actions: AtomicUsize,
    clones: AtomicUsize,
    reads: AtomicUsize,
    hash_read: AtomicBool,
    fail_hash: AtomicBool,
    fail_scope: AtomicBool,
    aggregate_scope: AtomicBool,
    forbidden_callbacks: AtomicBool,
    forbidden_scoped: AtomicBool,
    fail_local_admission: AtomicBool,
    clone_compete: AtomicBool,
    events: Events,
}
struct Part {
    scoped: bool,
    control: Arc<Control>,
}
impl Drop for Part {
    fn drop(&mut self) {
        assert!(
            !self.scoped || !self.control.aggregate_scope.load(Ordering::SeqCst),
            "later scoped aggregate destroyed"
        );
    }
}
struct Key {
    id: u64,
    hash: u64,
    ordinal: usize,
    control: Arc<Control>,
    _parts: (Part, Part),
}
impl Key {
    fn new(id: u64, hash: u64, events: &Events) -> Arc<Self> {
        let control = Arc::new(Control {
            action: AtomicUsize::new(0),
            limit: AtomicUsize::new(1),
            actions: AtomicUsize::new(0),
            clones: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
            hash_read: AtomicBool::new(false),
            fail_hash: AtomicBool::new(false),
            fail_scope: AtomicBool::new(false),
            aggregate_scope: AtomicBool::new(false),
            forbidden_callbacks: AtomicBool::new(false),
            forbidden_scoped: AtomicBool::new(false),
            fail_local_admission: AtomicBool::new(false),
            clone_compete: AtomicBool::new(false),
            events: Arc::clone(events),
        });
        Arc::new(Self::owned(id, hash, 0, control))
    }
    fn owned(id: u64, hash: u64, ordinal: usize, control: Arc<Control>) -> Self {
        Self {
            id,
            hash,
            ordinal,
            _parts: (
                Part {
                    scoped: ordinal == 2,
                    control: Arc::clone(&control),
                },
                Part {
                    scoped: ordinal == 2,
                    control: Arc::clone(&control),
                },
            ),
            control,
        }
    }
    fn arm(&self, action: usize, limit: usize) {
        self.control.action.store(action, Ordering::SeqCst);
        self.control.limit.store(limit, Ordering::SeqCst);
        self.control.actions.store(0, Ordering::SeqCst);
    }
    fn disarm(&self) {
        self.control.action.store(0, Ordering::SeqCst);
        self.control.hash_read.store(false, Ordering::SeqCst);
        self.control.fail_hash.store(false, Ordering::SeqCst);
        self.control.fail_scope.store(false, Ordering::SeqCst);
        self.control.aggregate_scope.store(false, Ordering::SeqCst);
        self.control
            .forbidden_callbacks
            .store(false, Ordering::SeqCst);
        self.control.forbidden_scoped.store(false, Ordering::SeqCst);
        self.control
            .fail_local_admission
            .store(false, Ordering::SeqCst);
        self.control.clone_compete.store(false, Ordering::SeqCst);
    }
}
fn scope() -> GlobalKeyScope {
    SCOPE.with(|slot| slot.borrow().as_ref().expect("installed scope").clone())
}
fn element(slot: u32) -> ElementId {
    ElementId::new_gen(slot, std::num::NonZeroU32::MIN)
}
fn owner(shared: &GlobalKeyScope) -> BuildOwner {
    let mut result = BuildOwner::new();
    result.set_global_key_scope(shared.clone());
    result
}
fn rig() -> (GlobalKeyScope, Events) {
    let shared = GlobalKeyScope::new();
    SCOPE.with(|slot| *slot.borrow_mut() = Some(shared.clone()));
    PEERS.with(|slot| assert!(slot.borrow().is_empty()));
    (shared, Arc::new(Mutex::new(Vec::new())))
}
fn clear() {
    let peers = PEERS.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    drop(peers);
    let previous = SCOPE.with(|slot| slot.borrow_mut().take());
    drop(previous);
}
fn action(key: &Key, other: &dyn ViewKey) {
    let mode = key.control.action.load(Ordering::SeqCst);
    if mode == 0 || BUSY.with(|busy| busy.replace(true)) {
        return;
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BUSY.with(|busy| busy.set(false));
        }
    }
    let _reset = Reset;
    if mode == READ {
        let _ = scope().claim_count();
        key.control.reads.fetch_add(1, Ordering::SeqCst);
        return;
    }
    if key.control.actions.load(Ordering::SeqCst) >= key.control.limit.load(Ordering::SeqCst) {
        return;
    }
    key.control.actions.fetch_add(1, Ordering::SeqCst);
    match mode {
        TOGGLE => PEERS.with(|slot| {
            let mut peers = slot.borrow_mut();
            let (peer, declared) = peers.first_mut().expect("neighbor owner");
            if peer.element_for_global_key(declared.as_ref()).is_some() {
                peer.unregister_global_key(declared.as_ref());
            } else {
                peer.register_global_key(declared.as_ref(), element(91));
            }
        }),
        INSERT_EQUAL => PEERS.with(|slot| {
            slot.borrow_mut()
                .first_mut()
                .expect("competitor")
                .0
                .register_global_key(other, element(92));
        }),
        REPLACE_NEIGHBOR => PEERS.with(|slot| {
            let mut peers = slot.borrow_mut();
            let (peer, declared) = peers.first_mut().expect("neighbor owner");
            peer.unregister_global_key(declared.as_ref());
            peer.register_global_key(other, element(94));
        }),
        REPLACE | DROP_AND_PANIC | DROP_ONLY => {
            let peers = PEERS.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
            drop(peers);
            if mode == REPLACE {
                let mut replacement = owner(&scope());
                replacement.register_global_key(other, element(93));
                let declared = other.as_any().downcast_ref::<Key>().expect("custom key");
                PEERS.with(|slot| {
                    slot.borrow_mut().push((
                        replacement,
                        Arc::new(Key::owned(
                            declared.id,
                            declared.hash,
                            0,
                            Arc::clone(&declared.control),
                        )),
                    ));
                });
            } else if mode == DROP_AND_PANIC {
                panic!("first equality failure");
            }
        }
        FAIL_EQ => panic!("first equality failure"),
        _ => panic!("unknown equality action"),
    }
}
impl ViewKey for Key {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn key_hash(&self) -> u64 {
        assert!(
            !self.control.forbidden_callbacks.load(Ordering::SeqCst),
            "forbidden rollback hash"
        );
        assert!(
            !(self.ordinal == 2 && self.control.forbidden_scoped.load(Ordering::SeqCst)),
            "scoped release hash invoked"
        );
        assert!(
            !self.control.fail_hash.load(Ordering::SeqCst),
            "first hash failure"
        );
        if self.control.hash_read.load(Ordering::SeqCst) {
            let _ = scope().claim_count();
            self.control.reads.fetch_add(1, Ordering::SeqCst);
        }
        self.hash
    }
    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        assert!(
            !self.control.forbidden_callbacks.load(Ordering::SeqCst),
            "forbidden rollback equality"
        );
        assert!(
            !(self.ordinal == 2 && self.control.forbidden_scoped.load(Ordering::SeqCst)),
            "scoped release equality invoked"
        );
        if self.ordinal == 1
            && self.control.fail_local_admission.load(Ordering::SeqCst)
            && scope().claim_count() == 2
        {
            let incoming = other.as_any().downcast_ref::<Key>().expect("incoming key");
            incoming
                .control
                .forbidden_callbacks
                .store(true, Ordering::SeqCst);
            self.control
                .forbidden_callbacks
                .store(true, Ordering::SeqCst);
            panic!("first local admission failure");
        }
        action(self, other);
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.id == other.id)
    }
    fn clone_key(&self) -> Box<dyn ViewKey> {
        let ordinal = self.control.clones.fetch_add(1, Ordering::SeqCst) + 1;
        if ordinal == 2 && self.control.clone_compete.swap(false, Ordering::SeqCst) {
            action(self, self);
        }
        Box::new(Self::owned(
            self.id,
            self.hash,
            ordinal,
            Arc::clone(&self.control),
        ))
    }
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "custom-key-{}", self.id)
    }
    fn is_global_key(&self) -> bool {
        true
    }
}
impl Drop for Key {
    fn drop(&mut self) {
        if self.ordinal == 1 || self.ordinal == 2 {
            self.control
                .events
                .lock()
                .expect("events")
                .push((self.id, self.ordinal));
        }
        assert!(
            !(self.ordinal == 2 && self.control.fail_scope.load(Ordering::SeqCst)),
            "first scoped retirement failure"
        );
    }
}
fn text(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .expect("string failure")
}
fn next(shared: &GlobalKeyScope, events: &Events) {
    let key = Key::new(99, 199, events);
    let mut healthy = owner(shared);
    healthy.register_global_key(key.as_ref(), element(99));
    assert_eq!(
        healthy.element_for_global_key(key.as_ref()),
        Some(element(99))
    );
    healthy.unregister_global_key(key.as_ref());
}

fn reads(kind: &str) {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let b = Key::new(2, 7, &events);
    let mut first = owner(&shared);
    first.register_global_key(a.as_ref(), element(1));
    if kind == "hash" {
        a.control.hash_read.store(true, Ordering::SeqCst);
        first.unregister_global_key(a.as_ref());
        assert!(a.control.reads.load(Ordering::SeqCst) > 0);
        assert_eq!(first.element_for_global_key(a.as_ref()), None);
        assert_eq!(shared.claim_count(), 0);
    } else {
        a.arm(READ, usize::MAX);
        let mut second = owner(&shared);
        second.register_global_key(b.as_ref(), element(2));
        assert_eq!(shared.claim_count(), 2);
        assert!(a.control.reads.load(Ordering::SeqCst) > 0);
        first.unregister_global_key(a.as_ref());
        second.unregister_global_key(b.as_ref());
    }
    a.disarm();
    b.disarm();
    next(&shared, &events);
    assert_eq!(shared.claim_count(), 0);
    clear();
}

fn mutation(kind: &str) {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let b = Key::new(2, 7, &events);
    let neighbor = Key::new(3, if kind == "other-bucket" { 8 } else { 7 }, &events);
    let mut first = owner(&shared);
    first.register_global_key(a.as_ref(), element(1));
    let mut peer = owner(&shared);
    if matches!(kind, "once" | "twice" | "other-bucket" | "same-length") {
        peer.register_global_key(neighbor.as_ref(), element(3));
    }
    PEERS.with(|slot| slot.borrow_mut().push((peer, Arc::clone(&neighbor))));
    let mut anchor = Some(first);
    if kind == "replacement" {
        PEERS.with(|slot| {
            slot.borrow_mut()
                .push((anchor.take().expect("anchor"), Arc::clone(&a)));
        });
        a.arm(REPLACE, 1);
    } else {
        a.arm(
            if kind == "equivalent" {
                INSERT_EQUAL
            } else if kind == "same-length" {
                REPLACE_NEIGHBOR
            } else {
                TOGGLE
            },
            if kind == "twice" { 2 } else { 1 },
        );
    }
    let mut incoming = owner(&shared);
    let result = catch_unwind(AssertUnwindSafe(|| {
        incoming.register_global_key(b.as_ref(), element(2));
    }));
    a.disarm();
    b.disarm();
    neighbor.disarm();
    if matches!(kind, "once" | "other-bucket") {
        assert!(result.is_ok());
        assert_eq!(
            incoming.element_for_global_key(b.as_ref()),
            Some(element(2))
        );
        incoming.unregister_global_key(b.as_ref());
    } else {
        let payload = result.expect_err("registration must refuse stale verdict");
        if kind == "twice" {
            assert_eq!(
                text(payload.as_ref()),
                "GlobalKey comparison changed scope repeatedly"
            );
        } else {
            assert!(text(payload.as_ref()).contains("already claimed"));
        }
        assert_eq!(incoming.element_for_global_key(b.as_ref()), None);
        PEERS.with(|slot| {
            if kind != "twice" {
                assert!(
                    slot.borrow()
                        .iter()
                        .any(|(peer, _)| peer.element_for_global_key(b.as_ref()).is_some())
                );
            }
        });
    }
    drop(incoming);
    // All survivors, including a replacement with the same hash, retire normally.
    clear();
    assert_eq!(shared.claim_count(), usize::from(kind != "replacement"));
    if let Some(anchor) = &anchor {
        assert_eq!(anchor.element_for_global_key(a.as_ref()), Some(element(1)));
    }
    drop(anchor);
    assert_eq!(shared.claim_count(), 0);
    next(&shared, &events);
}

fn retirement(kind: &str) {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let b = Key::new(2, 7, &events);
    for key in [&a, &b] {
        let mut peer = owner(&shared);
        peer.register_global_key(key.as_ref(), element(key.id as u32));
        PEERS.with(|slot| slot.borrow_mut().push((peer, Arc::clone(key))));
    }
    events.lock().expect("events").clear();
    if kind != "healthy" {
        a.control
            .fail_scope
            .store(kind != "second", Ordering::SeqCst);
        b.control
            .aggregate_scope
            .store(kind == "caught", Ordering::SeqCst);
        b.control
            .fail_scope
            .store(matches!(kind, "eq" | "second"), Ordering::SeqCst);
    }
    a.arm(
        if kind == "eq" {
            DROP_AND_PANIC
        } else {
            DROP_ONLY
        },
        1,
    );
    let c = Key::new(3, 7, &events);
    let mut incoming = owner(&shared);
    let result = catch_unwind(AssertUnwindSafe(|| {
        incoming.register_global_key(c.as_ref(), element(3));
    }));
    a.disarm();
    b.disarm();
    c.disarm();
    if kind == "healthy" {
        assert!(result.is_ok());
        assert_eq!(shared.claim_count(), 1);
        incoming.unregister_global_key(c.as_ref());
        assert_eq!(
            &*events.lock().expect("events"),
            &[(1, 1), (2, 1), (1, 2), (2, 2), (3, 1), (3, 2)]
        );
    } else {
        assert_eq!(
            text(result.expect_err("retirement must fail").as_ref()),
            if kind == "eq" {
                "first equality failure"
            } else {
                "first scoped retirement failure"
            }
        );
        assert_eq!(incoming.element_for_global_key(c.as_ref()), None);
        assert_eq!(shared.claim_count(), 0);
        let observed = events.lock().expect("events");
        assert!(observed.contains(&(1, 1)) && observed.contains(&(2, 1)));
        assert_eq!(
            observed.iter().filter(|(_, role)| *role == 2).count(),
            if kind == "second" {
                2
            } else {
                usize::from(kind != "eq")
            }
        );
    }
    next(&shared, &events);
    clear();
}

fn failures(kind: &str) {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let b = Key::new(2, 7, &events);
    let mut existing = owner(&shared);
    existing.register_global_key(a.as_ref(), element(1));
    let mut incoming = owner(&shared);
    if kind == "hash" {
        b.control.fail_hash.store(true, Ordering::SeqCst);
    } else {
        a.arm(FAIL_EQ, 1);
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        incoming.register_global_key(b.as_ref(), element(2));
    }));
    assert_eq!(
        text(result.expect_err("callback failure").as_ref()),
        if kind == "hash" {
            "first hash failure"
        } else {
            "first equality failure"
        }
    );
    a.disarm();
    b.disarm();
    assert_eq!(shared.claim_count(), 1);
    assert_eq!(
        existing.element_for_global_key(a.as_ref()),
        Some(element(1))
    );
    assert_eq!(incoming.element_for_global_key(b.as_ref()), None);
    drop(incoming);
    existing.unregister_global_key(a.as_ref());
    next(&shared, &events);
    clear();
}

fn identity() {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let mut original = owner(&shared);
    original.register_global_key(a.as_ref(), element(1));
    let clones = a.control.clones.load(Ordering::SeqCst);
    original.register_global_key(a.as_ref(), element(2));
    assert_eq!(a.control.clones.load(Ordering::SeqCst), clones);
    assert_eq!(
        original.take_global_key_for_reparent(a.as_ref()),
        Some(element(2))
    );
    assert_eq!(shared.claim_count(), 1);
    original.register_global_key(a.as_ref(), element(3));
    assert_eq!(shared.claim_count(), 1);
    // A missing-local release still withdraws this owner's surviving claim.
    assert_eq!(
        original.take_global_key_for_reparent(a.as_ref()),
        Some(element(3))
    );
    a.arm(READ, usize::MAX);
    original.unregister_global_key(a.as_ref());
    assert_eq!(shared.claim_count(), 0);
    a.disarm();
    let mut replacement = owner(&shared);
    replacement.register_global_key(a.as_ref(), element(4));
    original.unregister_global_key(a.as_ref());
    assert_eq!(
        replacement.element_for_global_key(a.as_ref()),
        Some(element(4))
    );
    assert_eq!(shared.claim_count(), 1);
    replacement.unregister_global_key(a.as_ref());
    next(&shared, &events);
    clear();
}

fn rollback() {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let b = Key::new(2, 7, &events);
    let mut original = owner(&shared);
    original.register_global_key(a.as_ref(), element(1));
    a.control.fail_local_admission.store(true, Ordering::SeqCst);
    let result = catch_unwind(AssertUnwindSafe(|| {
        original.register_global_key(b.as_ref(), element(2));
    }));
    assert_eq!(
        text(
            result
                .expect_err("post-admission comparison fails")
                .as_ref()
        ),
        "first local admission failure"
    );
    a.disarm();
    b.disarm();
    assert_eq!(shared.claim_count(), 1);
    assert_eq!(
        original.element_for_global_key(a.as_ref()),
        Some(element(1))
    );
    assert_eq!(original.element_for_global_key(b.as_ref()), None);
    original.unregister_global_key(a.as_ref());
    next(&shared, &events);
    clear();
}

fn release_identity() {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let mut original = owner(&shared);
    original.register_global_key(a.as_ref(), element(1));
    a.control.forbidden_scoped.store(true, Ordering::SeqCst);
    original.unregister_global_key(a.as_ref());
    a.disarm();
    assert_eq!(shared.claim_count(), 0);
    assert_eq!(original.element_for_global_key(a.as_ref()), None);
    assert_eq!(&*events.lock().expect("events"), &[(1, 1), (1, 2)]);
    next(&shared, &events);
    clear();
}

fn fallback_mutation() {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let neighbor = Key::new(3, 7, &events);
    let mut original = owner(&shared);
    original.register_global_key(a.as_ref(), element(1));
    assert_eq!(
        original.take_global_key_for_reparent(a.as_ref()),
        Some(element(1))
    );
    let mut peer = owner(&shared);
    peer.register_global_key(neighbor.as_ref(), element(3));
    PEERS.with(|slot| slot.borrow_mut().push((peer, Arc::clone(&neighbor))));
    a.arm(TOGGLE, 1);
    original.unregister_global_key(a.as_ref());
    a.disarm();
    neighbor.disarm();
    assert_eq!(shared.claim_count(), 0);
    assert_eq!(original.element_for_global_key(a.as_ref()), None);
    PEERS.with(|slot| {
        assert_eq!(
            slot.borrow()[0].0.element_for_global_key(neighbor.as_ref()),
            None
        );
    });
    next(&shared, &events);
    clear();
}

fn losing_clone() {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let b = Key::new(2, 7, &events);
    let mut anchor = owner(&shared);
    anchor.register_global_key(a.as_ref(), element(1));
    PEERS.with(|slot| slot.borrow_mut().push((owner(&shared), Arc::clone(&b))));
    b.arm(INSERT_EQUAL, 1);
    b.control.clone_compete.store(true, Ordering::SeqCst);
    b.control.fail_scope.store(true, Ordering::SeqCst);
    let mut incoming = owner(&shared);
    let result = catch_unwind(AssertUnwindSafe(|| {
        incoming.register_global_key(b.as_ref(), element(2));
    }));
    assert_eq!(
        text(result.expect_err("losing clone retirement fails").as_ref()),
        "first scoped retirement failure"
    );
    a.disarm();
    b.disarm();
    assert_eq!(incoming.element_for_global_key(b.as_ref()), None);
    assert_eq!(anchor.element_for_global_key(a.as_ref()), Some(element(1)));
    PEERS.with(|slot| {
        assert_eq!(
            slot.borrow()[0].0.element_for_global_key(b.as_ref()),
            Some(element(92))
        );
    });
    assert_eq!(shared.claim_count(), 2);
    drop(incoming);
    clear();
    anchor.unregister_global_key(a.as_ref());
    assert_eq!(shared.claim_count(), 0);
    next(&shared, &events);
}

fn incoming_release() {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let mut original = owner(&shared);
    original.register_global_key(a.as_ref(), element(1));
    a.control.hash_read.store(true, Ordering::SeqCst);
    a.control.fail_scope.store(true, Ordering::SeqCst);
    events.lock().expect("events").clear();
    struct Release<'a>(&'a mut BuildOwner, &'a Key);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            self.0.unregister_global_key(self.1);
        }
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _release = Release(&mut original, a.as_ref());
        panic!("incoming lookup failure");
    }));
    assert_eq!(
        text(result.expect_err("incoming unwind").as_ref()),
        "incoming lookup failure"
    );
    a.disarm();
    assert_eq!(original.element_for_global_key(a.as_ref()), None);
    assert_eq!(shared.claim_count(), 0);
    assert!(events.lock().expect("events").is_empty());
    next(&shared, &events);
    clear();
}

#[derive(Clone)]
struct Leaf(Arc<Key>);
impl View for Leaf {
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(self.0.as_ref())
    }
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}
impl RenderView for Leaf {
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
fn mounted() {
    let (shared, events) = rig();
    let a = Key::new(1, 7, &events);
    let b = Key::new(2, 7, &events);
    a.arm(READ, usize::MAX);
    let mut presentation = owner(&shared);
    let mut tree = ElementTree::new();
    let row = super::dense_reconcile_containment::DenseRow {
        children: vec![Leaf(Arc::clone(&a)).boxed(), Leaf(Arc::clone(&b)).boxed()],
    };
    let root = tree.mount_root_with_pipeline_owner(
        &row,
        Some(PipelineCell::new(PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ))),
        &mut presentation.element_owner_mut(),
    );
    presentation.schedule_build_for(root, 0, RebuildReason::InitialMount);
    presentation.build_scope(&mut tree);
    let children = tree.get(root).expect("root").child_ids().to_vec();
    assert_eq!(children.len(), 2);
    assert_ne!(children[0], children[1]);
    assert_eq!(
        presentation.element_for_global_key(a.as_ref()),
        Some(children[0])
    );
    assert_eq!(
        presentation.element_for_global_key(b.as_ref()),
        Some(children[1])
    );
    assert_eq!(shared.claim_count(), 2);
    a.control.hash_read.store(true, Ordering::SeqCst);
    b.control.hash_read.store(true, Ordering::SeqCst);
    let empty = super::dense_reconcile_containment::DenseRow {
        children: Vec::new(),
    };
    tree.update(root, &empty, &mut presentation.element_owner_mut());
    presentation.schedule_build_for(root, 0, RebuildReason::ParentUpdate);
    presentation.build_scope(&mut tree);
    presentation.finalize_tree(&mut tree);
    assert_eq!(tree.get(root).expect("root").child_ids(), []);
    assert_eq!(shared.claim_count(), 0);
    assert_eq!(presentation.element_for_global_key(a.as_ref()), None);
    assert_eq!(presentation.element_for_global_key(b.as_ref()), None);
    a.disarm();
    b.disarm();
    next(&shared, &events);
    clear();
}
fn bounded(kind: &str) {
    super::child_payload_recovery::run_child("FLUI_OWNER_KEY_LOOKUP_REENTRY_CHILD", kind);
}
pub(crate) fn dispatch_child(kind: &str) {
    match kind {
        "hash-read" => reads("hash"),
        "eq-read" => reads("eq"),
        "one-mutation" => mutation("once"),
        "two-mutations" => mutation("twice"),
        "other-bucket" => mutation("other-bucket"),
        "insert-equivalent" => mutation("equivalent"),
        "replace-owner" => mutation("replacement"),
        "same-length-replacement" => mutation("same-length"),
        "eq-drop-competition" => retirement("eq"),
        "caught-drop-competition" => retirement("caught"),
        "first-snapshot-drop" => retirement("first"),
        "second-snapshot-drop" => retirement("second"),
        "healthy-drop-order" => retirement("healthy"),
        "hash-failure" => failures("hash"),
        "eq-failure" => failures("eq"),
        "identity" => identity(),
        "rollback" => rollback(),
        "release-identity" => release_identity(),
        "fallback-mutation" => fallback_mutation(),
        "losing-clone" => losing_clone(),
        "incoming-release" => incoming_release(),
        "mounted" => mounted(),
        _ => panic!("unknown lookup selector"),
    }
}
pub(crate) fn key_hash_can_read_its_released_scope() {
    bounded("hash-read");
}
pub(crate) fn scoped_key_equality_can_read_claim_count() {
    bounded("eq-read");
}
pub(crate) fn key_equality_owner_release_revalidates_once() {
    bounded("one-mutation");
}
pub(crate) fn repeated_key_comparison_mutation_refuses_admission() {
    bounded("two-mutations");
}
pub(crate) fn unrelated_key_bucket_mutation_does_not_refuse_admission() {
    bounded("other-bucket");
}
pub(crate) fn inserted_equivalent_claim_invalidates_surviving_snapshot() {
    bounded("insert-equivalent");
}
pub(crate) fn replaced_owner_claim_invalidates_snapshot_identity() {
    bounded("replace-owner");
}
pub(crate) fn equal_length_replacement_invalidates_surviving_snapshot_marker() {
    bounded("same-length-replacement");
}
pub(crate) fn equality_failure_retains_two_retired_snapshot_keys() {
    bounded("eq-drop-competition");
}
pub(crate) fn caught_snapshot_retirement_retains_later_aggregate() {
    bounded("caught-drop-competition");
}
pub(crate) fn first_snapshot_retirement_failure_retains_independent_tail() {
    bounded("first-snapshot-drop");
}
pub(crate) fn second_snapshot_retirement_failure_preserves_completed_first() {
    bounded("second-snapshot-drop");
}
pub(crate) fn snapshot_retirement_preserves_healthy_key_order() {
    bounded("healthy-drop-order");
}
pub(crate) fn key_hash_failure_leaves_existing_claim_authoritative() {
    bounded("hash-failure");
}
pub(crate) fn key_equality_failure_leaves_existing_claim_authoritative() {
    bounded("eq-failure");
}
pub(crate) fn passive_claim_identity_preserves_retake_and_stale_release() {
    bounded("identity");
}
pub(crate) fn lookup_admission_rollback_invokes_no_key_callbacks() {
    bounded("rollback");
}
pub(crate) fn admitted_claim_release_invokes_no_scoped_key_callbacks() {
    bounded("release-identity");
}
pub(crate) fn missing_local_release_revalidates_scope_callback_mutation() {
    bounded("fallback-mutation");
}
pub(crate) fn losing_clone_retirement_preserves_competing_claim_authority() {
    bounded("losing-clone");
}
pub(crate) fn incoming_release_withdraws_authority_before_retaining_key_owners() {
    bounded("incoming-release");
}
pub(crate) fn mounted_colliding_keys_support_scope_callback_reentry() {
    bounded("mounted");
}
