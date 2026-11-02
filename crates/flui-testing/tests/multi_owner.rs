//! Real runtime transactions while a sibling's post-frame callback is held.
//! The shared-owner row is the negative control; no latency threshold is a
//! correctness assertion. Only commands and receipts cross threads.

use std::sync::mpsc::{TryRecvError, sync_channel};
use std::thread;
use std::time::Duration;

use flui_testing::widgets::{lay_out, tight};
use flui_widgets::Text;

#[test]
fn independent_runtime_frames_progress_while_a_sibling_callback_is_held() {
    for separate in [false, true] {
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let (finished_tx, finished_rx) = sync_channel(1);
        let (request_tx, request_rx) = sync_channel(1);
        let (frame_tx, frame_rx) = sync_channel(1);
        let (ready_tx, ready_rx) = sync_channel(1);
        thread::scope(|scope| {
            let small = move || {
                let mut panel = lay_out(Text::new("before"), tight(200.0, 40.0));
                ready_tx.send(()).expect("coordinator receives readiness");
                move || {
                    request_rx
                        .recv_timeout(Duration::from_secs(10))
                        .expect("frame request");
                    let before = panel.painted_frame_count();
                    panel.pump_widget(Text::new("after"));
                    assert_eq!(panel.painted_frame_count(), before + 1);
                    assert_eq!(panel.find_all_text("after").len(), 1);
                    frame_tx
                        .send(thread::current().id())
                        .expect("frame receipt");
                }
            };
            let held = move || {
                let mut panel = lay_out(Text::new("held"), tight(200.0, 40.0));
                panel
                    .post_frame_handle()
                    .schedule(move |_| {
                        entered_tx
                            .send(thread::current().id())
                            .expect("entered receipt");
                        // Disconnect also releases the owner if the coordinator panics.
                        let _ = release_rx.recv_timeout(Duration::from_secs(10));
                    })
                    .expect("callback admitted");
                panel.pump_widget(Text::new("released"));
                assert_eq!(panel.find_all_text("released").len(), 1);
                finished_tx.send(()).expect("completion receipt");
            };
            if separate {
                scope.spawn(held);
                scope.spawn(move || small()());
            } else {
                scope.spawn(move || {
                    let mut small = small();
                    held();
                    small();
                });
            }
            ready_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("sibling mounted before frame request");
            let held_thread = entered_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("held owner entered its callback");
            request_tx.send(()).expect("request accepted");
            if separate {
                let sibling = frame_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("independent owner frames before release");
                assert_ne!(held_thread, sibling);
                assert_eq!(finished_rx.try_recv(), Err(TryRecvError::Empty));
            } else {
                assert_eq!(frame_rx.try_recv(), Err(TryRecvError::Empty));
            }
            release_tx.send(()).expect("release accepted");
            finished_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("held callback finishes");
            if !separate {
                assert_eq!(
                    frame_rx
                        .recv_timeout(Duration::from_secs(10))
                        .expect("shared frame after release"),
                    held_thread
                );
            }
        });
    }
}
