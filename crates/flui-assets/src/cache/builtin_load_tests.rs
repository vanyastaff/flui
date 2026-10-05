//! Controlled failure boundaries for the private built-in singleflight path.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{AssetCache, CacheCapacity};
use crate::Asset;

#[path = "../../tests/cases.rs"]
mod cases;

struct Probe;

#[derive(Debug, thiserror::Error)]
#[error("load attempt {0}")]
struct LoadError(usize);

impl Asset for Probe {
    type Data = usize;
    type Key = String;
    type Error = LoadError;

    fn key(&self) -> String {
        "builtin".into()
    }
    async fn load(&self) -> Result<usize, LoadError> {
        Ok(1)
    }
}

#[test]
fn builtin_initialization_shares_work_and_recovers() {
    cases::run_cases(&[
        ("success shares allocation", success),
        ("failure shares error and retries", failure),
        ("cancellation releases waiter", cancellation),
        ("panic releases waiter", panic),
    ]);
}

fn success() {
    run_case(0);
}
fn failure() {
    run_case(1);
}
fn cancellation() {
    run_case(2);
}
fn panic() {
    run_case(3);
}

fn run_case(mode: u8) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("singleflight runtime starts")
        .block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let cache = AssetCache::<Probe>::new(CacheCapacity::default());
                let key = "builtin".to_owned();
                let loads = AtomicUsize::new(0);
                let gate = tokio::sync::Semaphore::new(0);
                let mut first = Box::pin(cache.initialize_builtin(key.clone(), || async {
                    let attempt = loads.fetch_add(1, Ordering::Relaxed) + 1;
                    gate.acquire().await.expect("gate remains open").forget();
                    assert!(mode != 3, "initializer panic");
                    if mode == 1 {
                        Err(LoadError(attempt))
                    } else {
                        Ok(attempt)
                    }
                }));
                let mut second = Box::pin(cache.initialize_builtin(key.clone(), || async {
                    let attempt = loads.fetch_add(1, Ordering::Relaxed) + 1;
                    gate.acquire().await.expect("gate remains open").forget();
                    Ok(attempt)
                }));
                poll_pending(first.as_mut()).await;
                poll_pending(second.as_mut()).await;
                assert_eq!(loads.load(Ordering::Relaxed), 1);
                if mode == 3 {
                    gate.add_permits(1);
                    std::future::poll_fn(|cx| {
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                first.as_mut().poll(cx)
                            }));
                        match outcome {
                            Err(payload) => {
                                assert_eq!(
                                    payload.downcast_ref::<&str>(),
                                    Some(&"initializer panic")
                                );
                                std::task::Poll::Ready(())
                            }
                            Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
                            Ok(std::task::Poll::Ready(_)) => panic!("initializer must panic"),
                        }
                    })
                    .await;
                }
                if mode >= 2 {
                    drop(first);
                    poll_pending(second.as_mut()).await;
                    assert_eq!(loads.load(Ordering::Relaxed), 2);
                    gate.add_permits(1);
                    let result = second.await.expect("remaining waiter restarts");
                    assert_eq!(*result, 2);
                    assert!(
                        cache
                            .get(&key)
                            .await
                            .expect("retry publishes")
                            .ptr_eq(&result)
                    );
                } else {
                    gate.add_permits(2);
                    let first = first.await;
                    let second = second.await;
                    if mode == 1 {
                        let first = first.expect_err("elected load fails");
                        let second = second.expect_err("waiter observes elected error");
                        assert!(Arc::ptr_eq(&first, &second));
                        assert_eq!(first.0, 1);
                        assert!(!cache.contains(&key));
                        let retry = cache
                            .initialize_builtin(key.clone(), || async { Ok(3) })
                            .await
                            .expect("later request retries");
                        assert_eq!(*retry, 3);
                    } else {
                        assert!(
                            first
                                .expect("elected load succeeds")
                                .ptr_eq(&second.expect("waiter shares success"))
                        );
                    }
                }
                assert_eq!(cache.stats().insertions, 1);
                let cached = cache
                    .initialize_builtin(key, || async { Err(LoadError(99)) })
                    .await
                    .expect("healthy next hit does not invoke initializer");
                assert_eq!(
                    *cached,
                    if mode >= 2 {
                        2
                    } else if mode == 1 {
                        3
                    } else {
                        1
                    }
                );
            })
            .await
            .expect("singleflight recovery finishes within five seconds");
        });
}

async fn poll_pending<F: Future>(mut future: std::pin::Pin<&mut F>) {
    std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending(), "load remains gated");
        std::task::Poll::Ready(())
    })
    .await;
}
