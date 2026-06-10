//! Custom gesture recognizer participating in the gesture arena.
//!
//! Demonstrates the [`CustomGestureRecognizer`] extension point: any type that
//! implements it automatically becomes a [`GestureArenaMember`] (via a blanket
//! impl) and can compete in the [`GestureArena`] alongside the built-in
//! recognizers. Winning the arena calls `on_arena_accept`; losing calls
//! `on_arena_reject`.
//!
//! Run with:
//! ```text
//! cargo run -p flui-interaction --example custom_recognizer
//! ```

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use flui_interaction::{PointerId, arena::GestureArena, sealed::CustomGestureRecognizer};

/// A minimal custom recognizer that records whether it won the arena and logs
/// the outcome. A real one would inspect pointer events and resolve itself.
struct LoggingRecognizer {
    name: &'static str,
    won: AtomicBool,
}

impl CustomGestureRecognizer for LoggingRecognizer {
    fn on_arena_accept(&self, pointer: PointerId) {
        self.won.store(true, Ordering::Relaxed);
        println!("[{}] accepted for pointer {pointer:?}", self.name);
    }

    fn on_arena_reject(&self, pointer: PointerId) {
        println!("[{}] rejected for pointer {pointer:?}", self.name);
    }
}

fn main() {
    let arena = GestureArena::new();
    let pointer = PointerId::PRIMARY;

    // Two custom recognizers contend for the same pointer.
    let winner = Arc::new(LoggingRecognizer {
        name: "winner",
        won: AtomicBool::new(false),
    });
    let loser = Arc::new(LoggingRecognizer {
        name: "loser",
        won: AtomicBool::new(false),
    });

    arena.add(pointer, winner.clone());
    arena.add(pointer, loser.clone());
    arena.close(pointer);

    // Resolve in favour of `winner`: it receives `on_arena_accept`, every other
    // member receives `on_arena_reject`.
    arena.resolve(pointer, Some(winner.clone()));

    assert!(
        winner.won.load(Ordering::Relaxed),
        "winner should be accepted"
    );
    assert!(
        !loser.won.load(Ordering::Relaxed),
        "loser should be rejected"
    );
    println!("custom recognizer arena demo OK");
}
