//! The dependent-set mutations on `InheritedElementAccess` take a token only
//! `flui-view` can construct. Application code that reaches a provider through
//! the public `ElementTree` chain can read it, but cannot record, reset, prune
//! or remove a dependency.
use flui_view::ElementId;
use flui_view::element::InheritedElementAccess;

fn sneak(access: &mut dyn InheritedElementAccess, dependent: ElementId) {
    access.remove_dependent(dependent);
}

fn main() {}
