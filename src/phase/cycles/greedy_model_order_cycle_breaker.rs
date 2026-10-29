//! Port of ELK's `GreedyModelOrderCycleBreaker`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/GreedyModelOrderCycleBreaker.java

use crate::graph::LGraph;
use crate::phase::cycles::greedy_cycle_breaker::{
    GreedyNodeChoice, break_cycles_greedy_with_choice,
};

pub fn break_cycles_greedy_model_order(graph: &mut LGraph) {
    break_cycles_greedy_with_choice(graph, GreedyNodeChoice::MinimumModelOrder);
}
