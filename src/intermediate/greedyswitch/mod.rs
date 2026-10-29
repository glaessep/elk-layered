//! The greedy switch heuristic and its crossing counters, one file per ELK class.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch

pub(crate) mod between_layer_edge_two_node_crossings_counter;
pub(crate) mod crossing_matrix_filler;
pub(crate) mod greedy_switch_heuristic;
pub(crate) mod north_south_edge_neighbouring_node_crossings_counter;
pub(crate) mod switch_decider;
