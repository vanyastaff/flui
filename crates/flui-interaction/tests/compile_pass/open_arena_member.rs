use std::rc::Rc;

use flui_interaction::{GestureArena, GestureArenaMember, PointerId};

struct ExternalMember;

impl GestureArenaMember for ExternalMember {
    fn accept_gesture(&self, _: PointerId) {}

    fn reject_gesture(&self, _: PointerId) {}
}

fn accepts_member(_member: &dyn GestureArenaMember) {}

fn main() {
    accepts_member(&ExternalMember);
    let arena = GestureArena::new();
    let concrete = Rc::new(ExternalMember);
    let member: Rc<dyn GestureArenaMember> = concrete.clone();
    let _entry = arena.add(PointerId::new(std::num::NonZeroU64::MIN), &concrete);
    arena.accept(PointerId::new(std::num::NonZeroU64::MIN), &member);
    arena.resolve(PointerId::new(std::num::NonZeroU64::MIN), Some(&member));
    // Primitive operations borrow the caller's owner rather than consume it.
    accepts_member(member.as_ref());
}
