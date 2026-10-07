use flui_interaction::{GestureArenaMember, PointerId};

struct ExternalMember;

impl GestureArenaMember for ExternalMember {
    fn accept_gesture(&self, _: PointerId) {}

    fn reject_gesture(&self, _: PointerId) {}
}

fn accepts_member(_member: &dyn GestureArenaMember) {}

fn main() {
    accepts_member(&ExternalMember);
}
