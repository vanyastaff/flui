//! Custom gesture recognizer participating in the gesture arena.
//!
//! Implements [`GestureArenaMember`] directly to compete in the [`GestureArena`]
//! alongside the built-in recognizers. Winning calls `accept_gesture`; losing
//! calls `reject_gesture`. The owner keeps each participant alive with [`Rc`].
//!
//! Run with:
//! ```text
//! cargo run -p flui-interaction --example custom_recognizer
//! ```

use std::{cell::Cell, rc::Rc};

use flui_interaction::{GestureArenaMember, PointerId, arena::GestureArena};

/// A minimal custom recognizer that records whether it won the arena and logs
/// the outcome. A real one would inspect pointer events and resolve itself.
struct LoggingRecognizer {
    name: &'static str,
    won: Cell<bool>,
}

impl GestureArenaMember for LoggingRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        self.won.set(true);
        println!("[{}] accepted for pointer {pointer:?}", self.name);
    }

    fn reject_gesture(&self, pointer: PointerId) {
        println!("[{}] rejected for pointer {pointer:?}", self.name);
    }
}

fn main() {
    let arena = GestureArena::new();
    let pointer = PointerId::PRIMARY;

    // Two custom recognizers contend for the same pointer.
    let winner = Rc::new(LoggingRecognizer {
        name: "winner",
        won: Cell::new(false),
    });
    let loser = Rc::new(LoggingRecognizer {
        name: "loser",
        won: Cell::new(false),
    });

    arena.add(pointer, &winner);
    arena.add(pointer, &loser);
    arena.close(pointer);

    // Resolve in favour of `winner`: it receives `accept_gesture`, every other
    // member receives `reject_gesture`.
    arena.resolve(pointer, Some(winner.clone()));

    assert!(winner.won.get(), "winner should be accepted");
    assert!(!loser.won.get(), "loser should be rejected");
    println!("custom recognizer arena demo OK");
}
