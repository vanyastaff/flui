//! A frame is read in process: a scenario over any connection, which a device runs too, cannot
//! reach one.

use flui_protocol::client::Connection;
use flui_testing::scenario::Tester;

fn reads_a_frame<C: Connection>(t: &mut Tester<C>) {
    let _frame = t.frame();
}

fn main() {}
