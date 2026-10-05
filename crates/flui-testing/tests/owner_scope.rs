//! Owner-entry regressions for the local post-frame lane.

use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_foundation::geometry::Offset;
use flui_interaction::InteractionDispatchError;
use flui_interaction::testing::input::{device_kind_from_button, pointer_down};
use flui_interaction::{GestureRecognizer, PointerId, TapGestureRecognizer};
use flui_testing::HeadlessBinding;

pub(crate) fn interaction_targets_are_isolated_between_headless_bindings() {
    let first = HeadlessBinding::new();
    let second = HeadlessBinding::new();
    let first_handle = first.interaction_dispatch_handle();
    let second_handle = second.interaction_dispatch_handle();

    let target = first.enter_owner_scope(|| {
        first_handle
            .register_pointer(|_| {})
            .expect("first binding registers its own target")
    });

    second.enter_owner_scope(|| {
        assert!(matches!(
            second_handle.replace_pointer(target, |_| {}),
            Err(InteractionDispatchError::WrongRealm)
        ));
    });
}

pub(crate) fn pointer_route_panic_still_runs_the_down_arena_lifecycle() {
    let binding = HeadlessBinding::new();
    let pointer = PointerId::PRIMARY;
    let recognizer = TapGestureRecognizer::new(binding.arena().clone());
    recognizer.add_pointer(pointer, Offset::new(4.0, 7.0), Offset::new(4.0, 7.0));
    assert!(binding.arena().is_open(pointer));

    let event = pointer_down(Offset::new(4.0, 7.0), device_kind_from_button(0));
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        binding.dispatch_pointer(&event, |_| panic!("route panic"));
    }));

    let payload = unwind.expect_err("the route panic must propagate");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"route panic"));
    assert!(
        !binding.arena().is_open(pointer),
        "Down must close the arena before the route panic resumes"
    );
}

type SemanticsCallback = std::sync::Arc<dyn Fn(bool) + Send + Sync>;
type WeakSemanticsCallback = std::sync::Weak<dyn Fn(bool) + Send + Sync>;

thread_local! {
    // Production callbacks require Send + Sync, whereas their binding is
    // owner-thread-local. A capture-free access path exercises that actual
    // public reentry contract without weakening either requirement.
    static SEMANTICS_OWNER: std::cell::RefCell<Option<std::rc::Rc<flui_runtime::renderer_binding::RenderingBinding>>> = const { std::cell::RefCell::new(None) };
    static SEMANTICS_HANDLES: std::cell::RefCell<Vec<WeakSemanticsCallback>> = const { std::cell::RefCell::new(Vec::new()) };
}

struct SemanticsScope;

impl Drop for SemanticsScope {
    fn drop(&mut self) {
        let owner = SEMANTICS_OWNER.with(|slot| slot.borrow_mut().take());
        let handles = SEMANTICS_HANDLES.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
        drop(handles);
        drop(owner);
    }
}

fn semantics_binding() -> flui_runtime::renderer_binding::RenderingBinding {
    flui_runtime::renderer_binding::RenderingBinding::new(
        flui_rendering::TextContextHandle::standalone(),
    )
}

fn remove_semantics_registrations() -> std::rc::Rc<flui_runtime::renderer_binding::RenderingBinding>
{
    use flui_rendering::binding::RendererBinding;
    let binding =
        SEMANTICS_OWNER.with(|slot| slot.borrow().as_ref().expect("entered owner").clone());
    let handles = SEMANTICS_HANDLES.with(|slot| slot.borrow().clone());
    for handle in handles {
        let listener = handle.upgrade().expect("snapshot retains the listener");
        binding.remove_semantics_enabled_listener(&listener);
    }
    binding
}

struct SemanticsCapture {
    label: &'static str,
    fail: bool,
    reenter: bool,
    drops: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Drop for SemanticsCapture {
    fn drop(&mut self) {
        use flui_rendering::binding::RendererBinding;
        self.drops.lock().expect("drop log").push(self.label);
        if self.reenter {
            let next = std::rc::Rc::new(semantics_binding());
            let calls = self.calls.clone();
            next.add_semantics_enabled_listener(std::sync::Arc::new(move |_| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }));
            SEMANTICS_OWNER.with(|slot| {
                let mut slot = slot.borrow_mut();
                assert!(
                    slot.is_none(),
                    "external slot released before capture retirement"
                );
                *slot = Some(next);
            });
        }
        if self.fail {
            std::panic::panic_any(self.label);
        }
    }
}

fn semantics_listener_retirement_child(mode: &str) {
    use flui_rendering::binding::RendererBinding;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    let _scope = SemanticsScope;
    assert!(SEMANTICS_OWNER.with(|slot| slot.borrow().is_none()));
    assert!(SEMANTICS_HANDLES.with(|slot| slot.borrow().is_empty()));
    let drops = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let binding = std::rc::Rc::new(semantics_binding());
    let snapshot = mode.starts_with("snapshot-");
    if snapshot {
        SEMANTICS_OWNER.with(|slot| *slot.borrow_mut() = Some(binding.clone()));
    }
    for label in ["semantics A", "semantics B"] {
        let fail = match mode {
            "physical-A" | "snapshot-A" => label == "semantics A",
            "physical-B" | "snapshot-B" => label == "semantics B",
            "physical-pair" | "physical-incoming" | "snapshot-callback" | "snapshot-pair" => true,
            _ => false,
        };
        let capture = SemanticsCapture {
            label,
            fail,
            reenter: mode == "physical-reentry" && label == "semantics A",
            drops: drops.clone(),
            calls: calls.clone(),
        };
        let callback_events = events.clone();
        let callback_calls = calls.clone();
        let callback_mode = mode.to_owned();
        let listener: SemanticsCallback = Arc::new(move |enabled| {
            let _capture = &capture;
            callback_events
                .lock()
                .expect("event log")
                .push((label, enabled));
            if callback_mode.starts_with("snapshot-") && label == "semantics A" {
                let binding = remove_semantics_registrations();
                if callback_mode == "snapshot-nested" {
                    let new_events = callback_events.clone();
                    let new_calls = callback_calls.clone();
                    binding.add_semantics_enabled_listener(Arc::new(move |value| {
                        new_events.lock().expect("event log").push(("new", value));
                        new_calls.fetch_add(1, Ordering::SeqCst);
                    }));
                    binding.set_semantics_enabled(false);
                }
                if callback_mode == "snapshot-callback" {
                    std::panic::panic_any("semantics callback first");
                }
            }
        });
        if snapshot {
            SEMANTICS_HANDLES.with(|slot| slot.borrow_mut().push(Arc::downgrade(&listener)));
        }
        if mode == "physical-duplicates" && label == "semantics A" {
            binding.add_semantics_enabled_listener(listener.clone());
            binding.add_semantics_enabled_listener(listener.clone());
            binding.remove_semantics_enabled_listener(&listener);
            assert!(
                drops.lock().expect("drop log").is_empty(),
                "borrowed removal retains the caller's actual Arc"
            );
            drop(listener);
            assert_eq!(*drops.lock().expect("drop log"), ["semantics A"]);
        } else {
            binding.add_semantics_enabled_listener(listener);
        }
    }
    let retained_binding = snapshot.then(|| binding.clone());
    let outcome = if snapshot {
        catch_unwind(AssertUnwindSafe(|| binding.set_semantics_enabled(true)))
    } else {
        catch_unwind(AssertUnwindSafe(|| {
            if mode == "physical-incoming" {
                let _binding = binding;
                std::panic::panic_any("incoming semantics failure");
            } else if mode == "physical-shared" {
                let alias = binding.clone();
                drop(binding);
                assert!(drops.lock().expect("drop log").is_empty());
                drop(alias);
            } else {
                if mode == "physical-duplicates" {
                    binding.set_semantics_enabled(true);
                    assert_eq!(*events.lock().expect("event log"), [("semantics B", true)]);
                }
                drop(binding);
            }
        }))
    };
    let (payload, expected_drops): (Option<&'static str>, &[&'static str]) = match mode {
        "physical-A" | "physical-pair" | "snapshot-A" | "snapshot-pair" => {
            (Some("semantics A"), &["semantics A"])
        }
        "physical-B" | "snapshot-B" => (Some("semantics B"), &["semantics A", "semantics B"]),
        "physical-incoming" => (Some("incoming semantics failure"), &[]),
        "snapshot-callback" => (Some("semantics callback first"), &[]),
        "physical-healthy"
        | "physical-shared"
        | "physical-reentry"
        | "physical-duplicates"
        | "snapshot-healthy"
        | "snapshot-nested" => (None, &["semantics A", "semantics B"]),
        _ => panic!("unknown semantics mode {mode}"),
    };
    match (outcome, payload) {
        (Ok(()), None) => {}
        (Err(payload), Some(expected)) => {
            assert_eq!(payload.downcast_ref::<&'static str>(), Some(&expected));
            flui_foundation::panic::retain_opaque_payload(payload);
        }
        _ => panic!("unexpected semantics outcome for {mode}"),
    }
    assert_eq!(*drops.lock().expect("drop log"), expected_drops);
    if snapshot {
        let binding = retained_binding.expect("snapshot retains its public owner");
        let expected_events: &[(&str, bool)] = match mode {
            "snapshot-callback" => &[("semantics A", true)],
            "snapshot-nested" => &[("semantics A", true), ("new", false), ("semantics B", true)],
            _ => &[("semantics A", true), ("semantics B", true)],
        };
        assert_eq!(
            *events.lock().expect("event log"),
            expected_events,
            "removed callbacks still belong to the admitted snapshot"
        );
        assert_eq!(
            binding.semantics_enabled(),
            mode != "snapshot-nested",
            "nested state remains authoritative"
        );
        let next_calls = calls.clone();
        binding.add_semantics_enabled_listener(Arc::new(move |_| {
            next_calls.fetch_add(1, Ordering::SeqCst);
        }));
        let before = calls.load(Ordering::SeqCst);
        binding.set_semantics_enabled(!binding.semantics_enabled());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            before + if mode == "snapshot-nested" { 2 } else { 1 },
            "same owner delivers the next operation"
        );
        drop(binding);
    } else if mode == "physical-reentry" {
        let replacement = SEMANTICS_OWNER
            .with(|slot| slot.borrow_mut().take())
            .expect("retirement installed replacement owner");
        replacement.set_semantics_enabled(true);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(replacement);
    }
    // Independent subsequent progress is observable even after final-owner failure.
    let next = semantics_binding();
    let next_calls = calls.clone();
    next.add_semantics_enabled_listener(Arc::new(move |_| {
        next_calls.fetch_add(1, Ordering::SeqCst);
    }));
    let before = calls.load(Ordering::SeqCst);
    next.set_semantics_enabled(true);
    assert_eq!(calls.load(Ordering::SeqCst), before + 1);
    drop(next);
}

/// Final-owner and removed-snapshot capture competition must stay catchable.
pub(crate) fn semantics_listener_retirement_preserves_independent_envelopes() {
    const CHILD: &str = "FLUI_SEMANTICS_LISTENER_RETIREMENT_CHILD";
    if let Ok(mode) = std::env::var(CHILD) {
        semantics_listener_retirement_child(&mode);
        return;
    }
    let mut failures = Vec::new();
    for mode in [
        "physical-healthy",
        "physical-A",
        "physical-B",
        "physical-pair",
        "physical-incoming",
        "physical-shared",
        "physical-reentry",
        "physical-duplicates",
        "snapshot-healthy",
        "snapshot-A",
        "snapshot-B",
        "snapshot-pair",
        "snapshot-callback",
        "snapshot-nested",
    ] {
        use std::io::Read;
        let mut child =
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", "containment_and_isolation_matrix", "--nocapture"])
                .env(CHILD, mode)
                .env("RUST_BACKTRACE", "0")
                .env("RUST_LIB_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("semantics child");
        let mut stdout = child.stdout.take().expect("stdout pipe");
        let mut stderr = child.stderr.take().expect("stderr pipe");
        let stdout = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).expect("stdout read");
            bytes
        });
        let stderr = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).expect("stderr read");
            bytes
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let (status, timed_out) = loop {
            if let Some(status) = child.try_wait().expect("child status") {
                break (status, false);
            }
            if std::time::Instant::now() >= deadline {
                child.kill().expect("kill timed out child");
                break (child.wait().expect("reap child"), true);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let stdout = stdout.join().expect("stdout reader");
        let stderr = stderr.join().expect("stderr reader");
        let output = String::from_utf8_lossy(&stdout);
        if timed_out
            || !status.success()
            || !output.contains("running 1 test")
            || output.contains("running 0 tests")
            || !output.contains("1 passed; 0 failed")
        {
            failures.push(format!(
                "{mode}: {status}, timeout={timed_out}\n{output}\n{}",
                String::from_utf8_lossy(&stderr)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "semantics retirement children failed:\n{}",
        failures.join("\n")
    );
}
