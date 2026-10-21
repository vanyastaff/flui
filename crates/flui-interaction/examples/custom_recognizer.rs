//! A custom recognizer attached alongside a peer through `RecognizerSet`.
//!
//! The owner retains each recognizer; the set and arena borrow it through
//! weak handles. `PrimaryContact` supplies admission and arena cleanup.
//!
//! Run with `cargo run -p flui-interaction --example custom_recognizer`.

use flui_interaction::gesture::GestureContest;
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

use flui_foundation::geometry::Point;
use flui_platform_api::EventTime;

use flui_interaction::{
    ArenaMembership, CancelOutcome, ContactEnd, GestureArenaMember, GestureBinding,
    GestureRecognizer, GestureSettings, HitTestEntry, HitTestResult, InteractionLane,
    LongPressGestureRecognizer, ManualClock, PointerButton, PointerButtons, PointerDispatch,
    PointerEventExt, PointerId, PointerInfo, PointerPosition, PointerPress, PointerRelease,
    PointerSample, PrimaryContact, RecognizerSet, RenderId,
    events::{PointerEvent, PointerKind},
    gesture::GestureArena,
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
    fn accept_gesture(&self, contest: GestureContest) {
        self.won.set(true);
        println!(
            "[{}] accepted for pointer {:?}",
            self.name,
            contest.pointer()
        );
    }

    fn reject_gesture(&self, contest: GestureContest) {
        if self.contact.tracks_contest(contest) {
            self.contact.withdraw();
        }
        println!(
            "[{}] rejected for pointer {:?}",
            self.name,
            contest.pointer()
        );
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
                self.contact.finish(ContactEnd::Contending);
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lane = InteractionLane::try_new()?;
    let clock = Arc::new(ManualClock::new());
    let binding = GestureBinding::with_clock(clock.clone());
    let arena = binding.arena().clone();
    let winner = LoggingRecognizer::new(arena.clone(), "winner", true);
    let loser = LoggingRecognizer::new(arena.clone(), "loser", false);
    let mut recognizers = RecognizerSet::default();
    recognizers.attach(&winner);
    recognizers.attach(&loser);

    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    let info = PointerInfo::new(pointer, PointerKind::Touch);
    let position = PointerPosition::try_new(Point::new(12.0, 20.0))?;
    let down = PointerEvent::Down(PointerPress::new(
        info,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        PointerSample::new(EventTime::from_nanos(0), position),
    ));
    let up = PointerEvent::Up(PointerRelease::new(
        info,
        PointerButton::PRIMARY,
        PointerButtons::only(PointerButton::PRIMARY),
        PointerSample::new(EventTime::from_nanos(20_000_000), position),
    ));
    lane.enter(|| -> Result<(), Box<dyn std::error::Error>> {
        let target = lane.dispatch_handle().register_pointer(move |dispatch| {
            recognizers.dispatch(dispatch);
        })?;
        let route = |_| {
            let mut path = HitTestResult::new();
            path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
            path
        };
        binding.handle_pointer_event(&down, route);
        binding.handle_pointer_event(&up, route);
        Ok(())
    })?;

    assert!(winner.won.get(), "the release claimant wins");
    assert!(!loser.won.get(), "the other recognizer loses");
    assert!(arena.is_empty(), "the completed contact releases its arena");

    // A second route shows idle progression without another pointer packet.
    // A mounted widget instead acquires GestureArenaScope::of(ctx); its binding
    // owns this progression and the widget must not create a private arena.
    let presses = Rc::new(Cell::new(0));
    let observed = presses.clone();
    let long_press = LongPressGestureRecognizer::builder(arena.clone())
        .settings(GestureSettings::default().with_long_press_timeout(Duration::from_millis(500)))
        .on_long_press(move || observed.set(observed.get() + 1))
        .build();
    let mut timed_recognizers = RecognizerSet::default();
    timed_recognizers.attach(&long_press);
    lane.enter(|| -> Result<(), Box<dyn std::error::Error>> {
        let target = lane.dispatch_handle().register_pointer(move |dispatch| {
            timed_recognizers.dispatch(dispatch);
        })?;
        let route = |_| {
            let mut path = HitTestResult::new();
            path.add(HitTestEntry::new(RenderId::new(2)).pointer_target(target));
            path
        };
        binding.handle_pointer_event(&down, route);
        assert!(binding.next_deadline().is_some());
        clock.advance(Duration::from_millis(500));
        binding.tick_deadlines();
        binding.drain_deferred_arena_resolutions();
        assert_eq!(presses.get(), 1, "stationary contact reaches its deadline");
        let held_up = PointerEvent::Up(PointerRelease::new(
            info,
            PointerButton::PRIMARY,
            PointerButtons::only(PointerButton::PRIMARY),
            PointerSample::new(EventTime::from_nanos(500_000_000), position),
        ));
        binding.handle_pointer_event(&held_up, route);
        Ok(())
    })?;
    assert!(arena.is_empty());
    println!("custom recognizer attachment demo OK");
    Ok(())
}
