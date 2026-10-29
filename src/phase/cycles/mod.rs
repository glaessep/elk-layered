//! Phase 1: cycle breaking, one file per ELK class.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles
//!
//! `reverse_feedback_edges` reverses edges the way `LEdge.reverse` does:
//! https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LEdge.java

pub(crate) mod depth_first_cycle_breaker;
pub(crate) mod greedy_cycle_breaker;
pub(crate) mod greedy_model_order_cycle_breaker;
pub(crate) mod interactive_cycle_breaker;
pub(crate) mod model_order_cycle_breaker;
#[cfg(test)]
mod tests;

pub use depth_first_cycle_breaker::break_cycles_depth_first;
pub use greedy_cycle_breaker::break_cycles_greedy;
pub use greedy_model_order_cycle_breaker::break_cycles_greedy_model_order;
pub use interactive_cycle_breaker::break_cycles_interactive;
pub use model_order_cycle_breaker::break_cycles_model_order;

use crate::graph::{EdgeId, LGraph, reverse_edge};

pub(crate) fn reverse_feedback_edges(graph: &mut LGraph, edges: Vec<EdgeId>, mark_cyclic: bool) {
    for edge_index in edges {
        if reverse_edge(graph, edge_index, true) && mark_cyclic {
            graph.cyclic = true;
        }
    }
}
