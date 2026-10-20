//! A type used where a recognizer is expected is told what a recognizer
//! implements, not only that a bound failed.
use flui_interaction::GestureRecognizer;

struct Swipe;

fn mount<R: GestureRecognizer>(_recognizer: R) {}

fn main() {
    mount(Swipe);
}
