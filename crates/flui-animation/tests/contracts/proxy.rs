//! Parent queries may reenter the proxy without holding its parent lock.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use crate::child_process;
use flui_animation::{
    Animation, AnimationStatus, ConstantAnimation, ProxyAnimation, StatusCallback,
};
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};

#[derive(Debug, Clone, Copy)]
enum Reentry {
    Value,
    Status,
}

#[derive(Debug)]
struct ReentrantParent {
    proxy: Mutex<Option<Weak<ProxyAnimation<f64>>>>,
    reentry: Reentry,
    values: ChangeNotifier,
    statuses: ChangeNotifier,
}

impl ReentrantParent {
    fn reenter(&self) {
        let proxy = self.proxy.lock().expect("parent hook lock").take();
        if let Some(proxy) = proxy.and_then(|proxy| proxy.upgrade()) {
            proxy.set_parent(Arc::new(ConstantAnimation::completed(0.75)));
        }
    }
}

impl Listenable for ReentrantParent {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.values.add_listener(callback)
    }
    fn remove_listener(&self, id: ListenerId) {
        self.values.remove_listener(id);
    }
    fn remove_all_listeners(&self) {
        self.values.remove_all_listeners();
    }
}

impl Animation<f64> for ReentrantParent {
    fn value(&self) -> f64 {
        if matches!(self.reentry, Reentry::Value) {
            self.reenter();
        }
        0.25
    }
    fn status(&self) -> AnimationStatus {
        if matches!(self.reentry, Reentry::Status) {
            self.reenter();
        }
        AnimationStatus::Forward
    }
    fn add_status_listener(&self, _callback: StatusCallback) -> ListenerId {
        self.statuses.add_listener(Arc::new(|| {}))
    }
    fn remove_status_listener(&self, id: ListenerId) {
        self.statuses.remove_listener(id);
    }
}

fn fixture(reentry: Reentry) -> (Arc<ProxyAnimation<f64>>, Arc<AtomicUsize>) {
    let parent = Arc::new(ReentrantParent {
        proxy: Mutex::new(None),
        reentry,
        values: ChangeNotifier::new(),
        statuses: ChangeNotifier::new(),
    });
    let proxy = Arc::new(ProxyAnimation::new(parent.clone()));
    *parent.proxy.lock().expect("set parent hook") = Some(Arc::downgrade(&proxy));
    let changes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&changes);
    proxy.add_listener(Arc::new(move || {
        observed.fetch_add(1, Ordering::Relaxed);
    }));
    (proxy, changes)
}

fn next_swap_still_notifies(proxy: &ProxyAnimation<f64>, changes: &AtomicUsize) {
    let before = changes.load(Ordering::Relaxed);
    proxy.set_parent(Arc::new(ConstantAnimation::dismissed(0.5)));
    assert_eq!(proxy.value(), 0.5);
    assert_eq!(proxy.status(), AnimationStatus::Dismissed);
    assert_eq!(changes.load(Ordering::Relaxed), before + 1);
}

fn parent_value_may_replace_the_proxy_parent() {
    let (proxy, changes) = fixture(Reentry::Value);
    assert_eq!(proxy.value(), 0.25, "query samples its original parent");
    assert_eq!(proxy.value(), 0.75, "next query sees the replacement");
    assert_eq!(changes.load(Ordering::Relaxed), 1);
    next_swap_still_notifies(&proxy, &changes);
}

fn parent_status_may_replace_the_proxy_parent() {
    let (proxy, changes) = fixture(Reentry::Status);
    assert_eq!(proxy.status(), AnimationStatus::Forward);
    assert_eq!(proxy.status(), AnimationStatus::Completed);
    assert_eq!(proxy.value(), 0.75);
    assert_eq!(changes.load(Ordering::Relaxed), 1);
    next_swap_still_notifies(&proxy, &changes);
}

fn old_parent_status_may_reenter_during_a_swap() {
    let (proxy, changes) = fixture(Reentry::Status);
    proxy.set_parent(Arc::new(ConstantAnimation::completed(1.0)));
    assert_eq!(
        proxy.value(),
        1.0,
        "outer swap finishes after reentrant swap"
    );
    assert_eq!(proxy.status(), AnimationStatus::Completed);
    assert_eq!(changes.load(Ordering::Relaxed), 2);
    next_swap_still_notifies(&proxy, &changes);
}

#[test]
fn proxy_parent_queries_allow_reentrant_replacement() {
    let cases: &[(&str, fn())] = &[
        ("parent value", parent_value_may_replace_the_proxy_parent),
        ("parent status", parent_status_may_replace_the_proxy_parent),
        (
            "old status during swap",
            old_parent_status_may_reenter_during_a_swap,
        ),
    ];
    if let Some(selected) = child_process::selected_case() {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known child case")
            .1();
        child_process::pass();
    }
    let names: Vec<_> = cases.iter().map(|(name, _)| *name).collect();
    child_process::run_rows(
        "proxy::proxy_parent_queries_allow_reentrant_replacement",
        &names,
    );
}
