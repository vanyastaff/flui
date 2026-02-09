//! Compile-fail tests for unit mixing prevention
//!
//! These tests verify that the type system prevents mixing incompatible unit types
//! at compile time, fulfilling User Story 2 (Unit Mixing Prevention) requirements.
//!
//! Uses trybuild to verify that certain code patterns fail to compile with
//! appropriate error messages.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
