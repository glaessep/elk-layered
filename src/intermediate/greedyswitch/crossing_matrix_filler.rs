//! Port of ELK's `CrossingMatrixFiller`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/CrossingMatrixFiller.java

use std::collections::HashMap;

use crate::graph::{LGraph, NodeId};
use crate::intermediate::greedyswitch::between_layer_edge_two_node_crossings_counter::BetweenLayerEdgeTwoNodeCrossingsCounter;
use crate::phase::ordering::layer_sweep_crossing_minimizer::CrossMinType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrossingCountSide {
    West,
    East,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrossingMatrixFiller {
    is_crossing_matrix_filled: Vec<Vec<bool>>,
    crossing_matrix: Vec<Vec<usize>>,
    in_between_layer_crossing_counter: BetweenLayerEdgeTwoNodeCrossingsCounter,
    pub(crate) direction: CrossingCountSide,
    one_sided: bool,
}

impl CrossingMatrixFiller {
    pub(crate) fn new(
        free_layer_len: usize,
        greedy_switch_type: CrossMinType,
        direction: CrossingCountSide,
    ) -> Self {
        Self {
            is_crossing_matrix_filled: vec![vec![false; free_layer_len]; free_layer_len],
            crossing_matrix: vec![vec![0; free_layer_len]; free_layer_len],
            in_between_layer_crossing_counter: BetweenLayerEdgeTwoNodeCrossingsCounter::new(),
            direction,
            one_sided: greedy_switch_type == CrossMinType::OneSidedGreedySwitch,
        }
    }

    pub(crate) fn get_crossing_matrix_entry(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        free_layer_index: usize,
        layer_positions: &HashMap<NodeId, usize>,
        upper_node: NodeId,
        lower_node: NodeId,
    ) -> usize {
        let Some(upper_position) = layer_positions.get(&upper_node).copied() else {
            return 0;
        };
        let Some(lower_position) = layer_positions.get(&lower_node).copied() else {
            return 0;
        };

        if !self.is_crossing_matrix_filled[upper_position][lower_position] {
            self.fill_crossing_matrix(
                graph,
                current_node_order,
                free_layer_index,
                upper_position,
                lower_position,
                upper_node,
                lower_node,
            );
            self.is_crossing_matrix_filled[upper_position][lower_position] = true;
            self.is_crossing_matrix_filled[lower_position][upper_position] = true;
        }
        self.crossing_matrix[upper_position][lower_position]
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_crossing_matrix(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        free_layer_index: usize,
        upper_position: usize,
        lower_position: usize,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        if self.one_sided {
            match self.direction {
                CrossingCountSide::East => self
                    .in_between_layer_crossing_counter
                    .count_eastern_edge_crossings(
                        graph,
                        current_node_order,
                        free_layer_index,
                        upper_node,
                        lower_node,
                    ),
                CrossingCountSide::West => self
                    .in_between_layer_crossing_counter
                    .count_western_edge_crossings(
                        graph,
                        current_node_order,
                        free_layer_index,
                        upper_node,
                        lower_node,
                    ),
            }
        } else {
            self.in_between_layer_crossing_counter
                .count_both_side_crossings(
                    graph,
                    current_node_order,
                    free_layer_index,
                    upper_node,
                    lower_node,
                );
        }

        self.crossing_matrix[upper_position][lower_position] =
            self.in_between_layer_crossing_counter.upper_lower_crossings;
        self.crossing_matrix[lower_position][upper_position] =
            self.in_between_layer_crossing_counter.lower_upper_crossings;
    }
}
