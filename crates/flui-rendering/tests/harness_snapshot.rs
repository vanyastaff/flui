//! Structural paint-snapshot dogfood for the render harness (sub-project A).

#[test]
fn insta_tooling_smoke() {
    insta::assert_snapshot!("smoke", "line one\nline two");
}
