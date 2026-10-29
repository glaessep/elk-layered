//! ELK's intermediate processors, one file per ELK class.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate

pub(crate) mod edge_and_layer_constraint_edge_reverser;
pub(crate) mod end_label_postprocessor;
pub(crate) mod end_label_preprocessor;
pub(crate) mod end_label_sorter;
pub(crate) mod graph_transformer;
pub(crate) mod hierarchical_node_resizing_processor;
pub(crate) mod hierarchical_port_constraint_processor;
pub(crate) mod hierarchical_port_dummy_size_processor;
pub(crate) mod hierarchical_port_orthogonal_edge_router;
pub(crate) mod hierarchical_port_position_processor;
pub(crate) mod hyperedge_dummy_merger;
pub(crate) mod in_layer_constraint_processor;
pub(crate) mod innermost_node_margin_calculator;
pub(crate) mod interactive_external_port_positioner;
pub(crate) mod inverted_port_processor;
pub(crate) mod label_and_node_size_processor;
pub(crate) mod label_dummy_inserter;
pub(crate) mod label_dummy_remover;
pub(crate) mod label_dummy_switcher;
pub(crate) mod label_side_selector;
pub(crate) mod layer_constraint_postprocessor;
pub(crate) mod layer_constraint_preprocessor;
pub(crate) mod layer_size_and_graph_height_calculator;
pub(crate) mod long_edge_joiner;
pub(crate) mod long_edge_splitter;
#[cfg(test)]
mod node_size_processor_tests;
pub(crate) mod north_south_port_postprocessor;
pub(crate) mod north_south_port_preprocessor;
#[cfg(test)]
mod north_south_port_tests;
pub(crate) mod reversed_edge_restorer;
#[cfg(test)]
mod tests;

pub use edge_and_layer_constraint_edge_reverser::reverse_edges_for_edge_and_layer_constraints;
pub use end_label_postprocessor::postprocess_end_labels;
pub use end_label_preprocessor::preprocess_end_labels;
pub use end_label_sorter::sort_end_labels;
pub use hierarchical_node_resizing_processor::resize_hierarchical_node_graph;
pub use hierarchical_port_constraint_processor::process_hierarchical_port_constraints;
pub use hierarchical_port_dummy_size_processor::process_hierarchical_port_dummy_sizes;
pub use hierarchical_port_orthogonal_edge_router::process_hierarchical_port_orthogonal_edges;
pub use hierarchical_port_position_processor::process_hierarchical_port_positions;
pub use hyperedge_dummy_merger::merge_hyperedge_dummies;
pub use in_layer_constraint_processor::process_in_layer_constraints;
pub use innermost_node_margin_calculator::calculate_innermost_node_margins;
pub use interactive_external_port_positioner::position_interactive_external_ports;
pub use inverted_port_processor::process_inverted_ports;
pub use label_and_node_size_processor::calculate_label_and_node_sizes;
pub use label_dummy_inserter::insert_label_dummies;
pub use label_dummy_remover::remove_label_dummies;
pub use label_dummy_switcher::switch_label_dummies;
pub use label_side_selector::select_label_sides;
pub use layer_constraint_postprocessor::postprocess_layer_constraints;
pub use layer_constraint_preprocessor::preprocess_layer_constraints;
pub use layer_size_and_graph_height_calculator::calculate_layer_sizes_and_graph_height;
pub use long_edge_joiner::join_long_edges;
pub use long_edge_splitter::split_long_edges;
pub use north_south_port_postprocessor::postprocess_north_south_ports;
pub use north_south_port_preprocessor::preprocess_north_south_ports;
pub use reversed_edge_restorer::restore_reversed_edges;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
// Each variant names the edge that breaks a layer constraint, so all end in `Edge`.
#[allow(clippy::enum_variant_names)]
pub enum IntermediateError {
    #[error(
        "node `{node_id}` has layer constraint FIRST_SEPARATE but has incoming edge `{edge_id}`"
    )]
    FirstSeparateIncomingEdge { node_id: String, edge_id: String },
    #[error(
        "node `{node_id}` has layer constraint LAST_SEPARATE but has outgoing edge `{edge_id}`"
    )]
    LastSeparateOutgoingEdge { node_id: String, edge_id: String },
    #[error("node `{node_id}` has layer constraint FIRST but has incoming edge `{edge_id}`")]
    FirstIncomingEdge { node_id: String, edge_id: String },
    #[error("node `{node_id}` has layer constraint LAST but has outgoing edge `{edge_id}`")]
    LastOutgoingEdge { node_id: String, edge_id: String },
}

pub type IntermediateResult<T> = Result<T, IntermediateError>;
