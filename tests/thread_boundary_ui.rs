//! Realm-bound handles stay on their owner thread: each fixture under
//! `tests/ui/thread_boundary/` hands one to `std::thread::spawn` through the
//! facade, and the compiler must refuse it, naming the public type.

#[test]
fn thread_boundary_ui() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/thread_boundary/*.rs");
}
