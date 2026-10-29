//! Port of ELK's `SelfLoopType`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/SelfLoopType.java

use crate::graph::{PortSide, SelfLoopType};

pub(crate) fn self_loop_type_from_sides(sides: &[PortSide]) -> Option<SelfLoopType> {
    match sides.len() {
        1 => Some(SelfLoopType::OneSide),
        2 => {
            let opposing = (sides.contains(&PortSide::East) && sides.contains(&PortSide::West))
                || (sides.contains(&PortSide::North) && sides.contains(&PortSide::South));
            if opposing {
                Some(SelfLoopType::TwoSidesOpposing)
            } else {
                Some(SelfLoopType::TwoSidesCorner)
            }
        }
        3 => Some(SelfLoopType::ThreeSides),
        4 => Some(SelfLoopType::FourSides),
        _ => None,
    }
}
