//! Port of ELK's `RoutingDirection` and of the parts of `WestToEastRoutingStrategy`,
//! `NorthToSouthRoutingStrategy` and `SouthToNorthRoutingStrategy` that differ by direction:
//! the port sides, a port's position on the hypernode, and the place of a routing slot. One enum
//! holds the three strategies, so each method matches on the direction.
//!
//! Sources:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/direction/RoutingDirection.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/direction/WestToEastRoutingStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/direction/NorthToSouthRoutingStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/direction/SouthToNorthRoutingStrategy.java

use crate::graph::{LGraph, PortRef, PortSide};
use crate::phase::routing::orthogonal::direction::base_routing_direction_strategy::absolute_anchor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDirection {
    WestToEast,
    NorthToSouth,
    SouthToNorth,
}

impl RoutingDirection {
    pub(crate) fn source_port_side(self) -> PortSide {
        match self {
            Self::WestToEast => PortSide::East,
            Self::NorthToSouth => PortSide::South,
            Self::SouthToNorth => PortSide::North,
        }
    }

    pub(crate) fn target_port_side(self) -> PortSide {
        match self {
            Self::WestToEast => PortSide::West,
            Self::NorthToSouth => PortSide::North,
            Self::SouthToNorth => PortSide::South,
        }
    }

    pub(crate) fn port_position_on_hypernode(self, graph: &LGraph, port_ref: PortRef) -> f64 {
        let anchor = absolute_anchor(graph, port_ref);
        match self {
            Self::WestToEast => anchor.y,
            Self::NorthToSouth | Self::SouthToNorth => anchor.x,
        }
    }

    pub(crate) fn routing_slot_position(
        self,
        start_pos: f64,
        routing_slot: i32,
        edge_spacing: f64,
    ) -> f64 {
        match self {
            Self::WestToEast | Self::NorthToSouth => {
                start_pos + f64::from(routing_slot) * edge_spacing
            }
            Self::SouthToNorth => start_pos - f64::from(routing_slot) * edge_spacing,
        }
    }
}
