//! Window-independent owner wake probe; run with tools/device-checks/check-owner-wake.py.
#[cfg(target_os = "macos")]
mod native {
    use flui_platform::{OwnerPlatform, PlatformProxy, ProxySendError, WindowOptions};
    use std::{
        cell::RefCell,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    thread_local! {
        static OWNER: RefCell<Option<OwnerPlatform>> = const { RefCell::new(None) };
    }
    struct Capture(Arc<AtomicUsize>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    pub(super) fn run() {
        let case = std::env::args().nth(1).unwrap_or_else(|| "worker".into());
        assert!(matches!(
            case.as_str(),
            "worker" | "live-worker" | "reentrant" | "quit-priority" | "bootstrap-error"
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let denied = Arc::new(AtomicUsize::new(0));
        let wrong_thread = Arc::new(AtomicUsize::new(0));
        let depth = Arc::new(AtomicUsize::new(0));
        let max_depth = Arc::new(AtomicUsize::new(0));
        let callback_thread = Arc::clone(&wrong_thread);
        let callback_depth = Arc::clone(&depth);
        let callback_max_depth = Arc::clone(&max_depth);
        let saved = Arc::new(std::sync::Mutex::new(None::<PlatformProxy>));
        let cb_calls = Arc::clone(&calls);
        let cb_denied = Arc::clone(&denied);
        let capture = Capture(Arc::clone(&drops));
        let saved_proxy = Arc::clone(&saved);
        let selected = case.clone();
        let (turn_tx, turn_rx) = std::sync::mpsc::channel();
        let (proxy_tx, proxy_rx) = std::sync::mpsc::channel::<PlatformProxy>();
        let worker = (case == "live-worker").then(|| {
            std::thread::spawn(move || {
                let proxy = proxy_rx.recv().expect("proxy");
                turn_rx.recv().expect("first live owner turn");
                std::thread::sleep(std::time::Duration::from_millis(40));
                proxy.wake().expect("wake idle windowless loop");
            })
        });
        let result = flui_platform::current_platform()
            .expect("platform")
            .run(Box::new(move |owner| {
                let proxy = owner.proxy();
                *saved_proxy.lock().expect("proxy slot") = Some(proxy.clone());
                let callback_proxy = proxy.clone();
                let callback_case = selected.clone();
                let owner_thread = std::thread::current().id();
                owner.on_wake(Box::new(move || {
                    let _ = &capture;
                    callback_thread.fetch_add(
                        usize::from(std::thread::current().id() != owner_thread),
                        Ordering::SeqCst,
                    );
                    let entered = callback_depth.fetch_add(1, Ordering::SeqCst) + 1;
                    callback_max_depth.fetch_max(entered, Ordering::SeqCst);
                    let count = cb_calls.fetch_add(1, Ordering::SeqCst) + 1;
                    if callback_case == "live-worker" && count == 1 {
                        turn_tx.send(()).expect("worker ready");
                    } else if callback_case == "reentrant" && count == 1 {
                        callback_proxy.wake().expect("next turn");
                        objc2_foundation::NSRunLoop::currentRunLoop().runUntilDate(
                            &objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.03),
                        );
                    } else {
                        callback_proxy.request_quit().expect("quit signal");
                        if callback_case == "quit-priority" {
                            let wake_denied = matches!(
                                callback_proxy.wake(),
                                Err(ProxySendError::OwnerGone { .. })
                            );
                            let open_denied = OWNER.with(|slot| {
                                slot.borrow()
                                    .as_ref()
                                    .expect("owner")
                                    .open_window(WindowOptions::default())
                                    .is_err()
                            });
                            cb_denied
                                .store(usize::from(wake_denied && open_denied), Ordering::SeqCst);
                        }
                    }
                    callback_depth.fetch_sub(1, Ordering::SeqCst);
                }))?;
                OWNER.with(|slot| *slot.borrow_mut() = Some(owner));
                if selected == "bootstrap-error" {
                    return Err(std::io::Error::other("intentional bootstrap failure").into());
                }
                if selected == "live-worker" {
                    proxy_tx.send(proxy.clone()).expect("worker");
                }
                std::thread::spawn(move || {
                    for _ in 0..32 {
                        proxy.wake().expect("worker wake");
                    }
                })
                .join()
                .expect("worker");
                Ok(())
            }));
        if case == "bootstrap-error" {
            assert!(result.is_err());
        } else {
            result.expect("normal return");
        }
        if let Some(worker) = worker {
            worker.join().expect("live worker");
        }
        println!("OWNER_WAKE_RETURNED");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            if matches!(case.as_str(), "reentrant" | "live-worker") {
                2
            } else {
                usize::from(case != "bootstrap-error")
            }
        );
        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "callback released at shutdown"
        );
        assert_eq!(wrong_thread.load(Ordering::SeqCst), 0);
        assert_eq!(depth.load(Ordering::SeqCst), 0);
        assert_eq!(
            max_depth.load(Ordering::SeqCst),
            usize::from(case != "bootstrap-error")
        );
        if case == "quit-priority" {
            assert_eq!(denied.load(Ordering::SeqCst), 1);
        }
        let old = saved.lock().expect("saved").take().expect("proxy");
        assert!(matches!(old.wake(), Err(ProxySendError::OwnerGone { .. })));
        OWNER.with(|slot| drop(slot.borrow_mut().take()));
        println!("OWNER_WAKE_CASE={case}");
        println!("OWNER_WAKE_PASS");
    }
}
fn main() {
    #[cfg(target_os = "macos")]
    native::run();
    #[cfg(not(target_os = "macos"))]
    panic!("owner_wake_probe currently requires the native macOS backend");
}
