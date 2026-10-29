//! Phase 5: edge routing, one file per ELK class.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges

pub mod orthogonal;
pub(crate) mod orthogonal_edge_router;
#[cfg(test)]
mod tests;

pub use orthogonal_edge_router::route_edges_orthogonal;
