//! ELK's orthogonal hyperedge routing, one file per ELK class.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal

pub(crate) mod direction;
pub(crate) mod hyper_edge_cycle_detector;
pub(crate) mod hyper_edge_segment;
pub(crate) mod hyper_edge_segment_dependency;
pub(crate) mod hyper_edge_segment_splitter;
pub(crate) mod orthogonal_routing_generator;
#[cfg(test)]
mod tests;

pub use direction::RoutingDirection;
pub use hyper_edge_segment::HyperEdgeSegment;
pub use hyper_edge_segment_dependency::HyperEdgeGraph;
pub use orthogonal_routing_generator::{
    break_non_critical_cycles, route_edges, route_edges_west_to_east,
};

pub const TOLERANCE: f64 = 1e-3;
