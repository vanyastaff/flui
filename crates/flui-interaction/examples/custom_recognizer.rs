//! A custom recognizer attached alongside a peer through `RecognizerSet`.
//!
//! The owner retains each recognizer; the set and arena borrow it through
//! weak handles. `PrimaryContact` supplies admission and arena cleanup.
//!
//! Run with `cargo run -p flui-interaction --example custom_recognizer`.

use std::{cell::Cell, rc::Rc};

use flui_interaction::{
    ArenaMembership, CancelOutcome, GestureArenaMember, GestureRecognizer, GestureSettings, Offset,
    PointerDispatch, PointerEventExt, PointerId, PrimaryContact, RecognizerSet,
    arena::GestureArena,
    events::{PointerEvent, PointerKind, make_down_event_for_id, make_up_event_for_id},
};

struct LoggingRecognizer {
    name: &'static str,
    claims_release: bool,
    won: Cell<bool>,
    contact: PrimaryContact,
}

impl LoggingRecognizer {
    fn new(arena: GestureArena, name: &'static str, claims_release: bool) -> Rc<Self> {
        Rc::new_cyclic(|this: &std::rc::Weak<Self>| Self {
            name,
            claims_release,
            won: Cell::new(false),
            contact: PrimaryContact::new(ArenaMembership::new(arena, this.clone())),
        })
    }
}

impl GestureArenaMember for LoggingRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        self.won.set(true);
        println!("[{}] accepted for pointer {pointer:?}", self.name);
    }

    fn reject_gesture(&self, pointer: PointerId) {
        self.contact.withdraw();
        println!("[{}] rejected for pointer {pointer:?}", self.name);
    }
}

impl GestureRecognizer for LoggingRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        let _ = self.contact.begin(down, &GestureSettings::default());
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        if !dispatch
            .local
            .pointer_id()
            .is_some_and(|pointer| self.contact.tracks(pointer))
        {
            return;
        }
        match dispatch.local {
            PointerEvent::Up(_) if self.claims_release => {
                self.contact.accept();
                self.contact.finish();
            }
            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                self.contact.withdraw();
            }
            _ => {}
        }
    }

    fn cancel(&self) -> CancelOutcome {
        if self.contact.withdraw().is_some() {
            CancelOutcome::Cancelled
        } else {
            CancelOutcome::Idle
        }
    }
}

fn main() {
    let arena = GestureArena::new();
    let winner = LoggingRecognizer::new(arena.clone(), "winner", true);
    let loser = LoggingRecognizer::new(arena.clone(), "loser", false);
    let mut recognizers = RecognizerSet::default();
    recognizers.attach(&winner);
    recognizers.attach(&loser);

    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    let down = make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    let up = make_up_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    recognizers.dispatch(PointerDispatch::at_root(&down));
    arena.close(pointer);
    recognizers.dispatch(PointerDispatch::at_root(&up));

    assert!(winner.won.get(), "the release claimant wins");
    assert!(!loser.won.get(), "the other recognizer loses");
    assert!(arena.is_empty(), "the completed contact releases its arena");
    println!("custom recognizer attachment demo OK");
}
