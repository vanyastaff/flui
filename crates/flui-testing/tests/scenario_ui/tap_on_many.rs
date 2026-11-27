//! A tap is aimed at one element: a `Find<Many>` is narrowed before it can be tapped.

use flui_testing::scenario::{Tester, find};
use flui_widgets::Text;

fn main() {
    let mut t = Tester::mount(Text::new("Item"));
    t.tap(&find::text("Item").all());
}
