//! A behavioural widget's gesture performs an `Intent`: a callback in its
//! place is refused with the reason, not a bare trait error.
use flui_sdk::widgets::GestureCommand;

fn main() {
    let _ = GestureCommand::new("Archive", |_count: u32| {});
}
