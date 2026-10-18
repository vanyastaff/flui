//! Display probe: prints the display snapshot and, when asked, watches it.
//!
//! `cargo run -p flui-platform --example displays` prints each display (id,
//! name, bounds, scale, refresh period) and exits.
//!
//! With `FLUI_DISPLAYS_WATCH_SECS=<seconds>` it then opens a window and keeps
//! running until the window is closed or the time is up, printing a line for
//! every snapshot change and every `on_display_changed` hint. Change the
//! refresh rate or resolution in the system settings, or drag the window
//! between monitors, to see them.

use flui_platform::{DisplaySnapshot, WindowOptions, current_platform};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

const WATCH_SECS: &str = "FLUI_DISPLAYS_WATCH_SECS";

// The owner handle, reachable from the owner thread's wake hook.
thread_local! {
    static OWNER: std::cell::RefCell<Option<flui_platform::OwnerPlatform>> =
        std::cell::RefCell::default();
}

fn main() -> anyhow::Result<()> {
    let platform = current_platform()?;
    println!("platform: {}", platform.name());
    let snapshot = platform.display_snapshot()?;
    print_snapshot("snapshot", &snapshot);

    let Some(watch) = std::env::var(WATCH_SECS)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return Ok(());
    };
    println!("watching for {watch} s, close the window to stop");

    let seen = Arc::new(AtomicU64::new(snapshot.revision().get()));
    platform.run(Box::new(move |owner| {
        let window = owner.open_window(WindowOptions::default())?.try_ready()?;
        println!("window display: {:?}", window.display_id());
        let hinted = Arc::downgrade(&window);
        window.on_display_changed(Box::new(move || {
            let display = hinted.upgrade().and_then(|window| window.display_id());
            println!("window on_display_changed: now on {display:?}");
        }));
        let quit = owner.proxy();
        let sampled = Arc::clone(&seen);
        owner.on_wake(Box::new(move || {
            // Every owner wake re-reads the snapshot; a new revision is a change.
            let snapshot = OWNER.with(|owner| {
                owner
                    .borrow()
                    .as_ref()
                    .map(flui_platform::OwnerPlatform::display_snapshot)
            });
            let Some(Ok(snapshot)) = snapshot else {
                return;
            };
            if sampled.swap(snapshot.revision().get(), Ordering::AcqRel)
                != snapshot.revision().get()
            {
                print_snapshot("snapshot changed", &snapshot);
            }
        }))?;
        OWNER.with(|slot| slot.replace(Some(owner)));
        // The window lives as long as the loop.
        std::mem::forget(window);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(watch));
            let _ = quit.request_quit();
        });
        Ok(())
    }))?;
    Ok(())
}

fn print_snapshot(label: &str, snapshot: &DisplaySnapshot) {
    println!(
        "{label}: revision {}, {} display(s)",
        snapshot.revision().get(),
        snapshot.displays().len()
    );
    for display in snapshot.displays() {
        let period = display.refresh_period.map_or_else(
            || "period unknown".to_owned(),
            |period| {
                let period = period.get();
                format!(
                    "period {} us ({:.3} Hz)",
                    period.as_micros(),
                    period.as_secs_f64().recip()
                )
            },
        );
        println!(
            "  id {} {:?}{}: {}x{} at ({}, {}), scale {}, {period}",
            display.id.get(),
            display.name,
            if display.is_primary { " [primary]" } else { "" },
            display.bounds.size.width,
            display.bounds.size.height,
            display.bounds.origin.x,
            display.bounds.origin.y,
            display.scale_factor,
        );
    }
}
