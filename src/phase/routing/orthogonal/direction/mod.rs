//! The routing directions of the orthogonal router.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/direction

pub(crate) mod base_routing_direction_strategy;
pub(crate) mod routing_direction;

pub use routing_direction::RoutingDirection;
