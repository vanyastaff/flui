//! Parent queries may reenter the proxy without holding its parent lock.

use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

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
    const SELECTED: &str = "FLUI_PROXY_PARENT_REENTRY_CASE";
    if let Ok(selected) = std::env::var(SELECTED) {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known child case")
            .1();
        return;
    }
    let mut failures = Vec::new();
    for (name, _) in cases {
        let mut child =
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "proxy::proxy_parent_queries_allow_reentrant_replacement",
                    "--nocapture",
                ])
                .env(SELECTED, name)
                .env("RUST_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("proxy reentry child");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("child stdout");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("child stderr");
            text
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill deadlocked child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
