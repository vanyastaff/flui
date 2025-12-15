//! BoxProtocol - 2D cartesian layout protocol.
//!
//! The box protocol is the primary layout protocol for most UI widgets.
//! It uses rectangular constraints and produces size as output.

use flui_types::Size;

use super::Protocol;
use crate::constraints::BoxConstraints;
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// 2D cartesian layout protocol with rectangular constraints.
///
/// This is the primary layout protocol for most UI widgets. It uses
/// `BoxConstraints` to specify min/max dimensions and returns a `Size`.
///
/// # Layout Model
///
/// 1. Parent passes `BoxConstraints` specifying allowed size range
/// 2. Child computes its size within those constraints
/// 3. Child returns `Size` (width, height) as layout result
/// 4. Parent positions child using `Offset` stored in parent data
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's box layout protocol used by `RenderBox`.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::protocol::BoxProtocol;
/// use flui_rendering::containers::Children;
///
/// // Create a container for box protocol children
/// let children: Children<BoxProtocol> = Children::new();
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Object = dyn RenderBox;
    type Constraints = BoxConstraints;
    type ParentData = BoxParentData;
    type Geometry = Size;

    fn default_geometry() -> Size {
        Size::ZERO
    }

    fn name() -> &'static str {
        "box"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_protocol_name() {
        assert_eq!(BoxProtocol::name(), "box");
    }

    #[test]
    fn test_box_protocol_default_geometry() {
        assert_eq!(BoxProtocol::default_geometry(), Size::ZERO);
    }

    #[test]
    fn test_box_protocol_is_copy() {
        let p1 = BoxProtocol;
        let _p2 = p1; // Copy - compiles because BoxProtocol is Copy
        assert_eq!(BoxProtocol::name(), "box");
    }
}
