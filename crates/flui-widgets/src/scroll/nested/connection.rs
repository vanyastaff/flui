//! Strong/weak participant ownership without exposing engine state to authors.

use std::rc::{Rc, Weak};

use flui_foundation::geometry::Axis;

use super::{
    link::ParticipantKind,
    types::{NestedScrollConnection, OverscrollBehavior},
};
use crate::scroll::scrollable::FlingEndpoint;

#[derive(Clone)]
pub(super) enum Connection {
    Authored(Rc<dyn NestedScrollConnection>),
    Owner {
        connection: Rc<FlingEndpoint>,
        axis: Axis,
        overscroll: OverscrollBehavior,
    },
}

#[derive(Clone)]
pub(super) enum WeakConnection {
    Authored(Weak<dyn NestedScrollConnection>),
    Owner {
        connection: Weak<FlingEndpoint>,
        axis: Axis,
        overscroll: OverscrollBehavior,
    },
}

impl Connection {
    pub(super) fn callbacks(&self) -> &dyn NestedScrollConnection {
        match self {
            Self::Authored(connection) => connection.as_ref(),
            Self::Owner { connection, .. } => connection.as_ref(),
        }
    }

    pub(super) fn downgrade(&self) -> WeakConnection {
        match self {
            Self::Authored(connection) => WeakConnection::Authored(Rc::downgrade(connection)),
            Self::Owner {
                connection,
                axis,
                overscroll,
            } => WeakConnection::Owner {
                connection: Rc::downgrade(connection),
                axis: *axis,
                overscroll: *overscroll,
            },
        }
    }

    pub(super) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Authored(left), Self::Authored(right)) => Rc::ptr_eq(left, right),
            (
                Self::Owner {
                    connection: left,
                    axis,
                    overscroll,
                },
                Self::Owner {
                    connection: right,
                    axis: other_axis,
                    overscroll: other_overscroll,
                },
            ) => Rc::ptr_eq(left, right) && axis == other_axis && overscroll == other_overscroll,
            _ => false,
        }
    }
}

impl WeakConnection {
    pub(super) fn upgrade(&self) -> Option<Connection> {
        match self {
            Self::Authored(connection) => connection.upgrade().map(Connection::Authored),
            Self::Owner {
                connection,
                axis,
                overscroll,
            } => connection.upgrade().map(|connection| Connection::Owner {
                connection,
                axis: *axis,
                overscroll: *overscroll,
            }),
        }
    }

    pub(super) fn kind(&self) -> ParticipantKind {
        match self {
            Self::Authored(_) => ParticipantKind::Connection,
            Self::Owner {
                axis, overscroll, ..
            } => ParticipantKind::Owner {
                axis: *axis,
                overscroll: *overscroll,
            },
        }
    }
}
