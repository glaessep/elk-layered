//! Port of ELK's `GreedySwitchHeuristic`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/GreedySwitchHeuristic.java

use crate::graph::{LGraph, NodeId};
use crate::intermediate::greedyswitch::crossing_matrix_filler::{
    CrossingCountSide, CrossingMatrixFiller,
};
use crate::intermediate::greedyswitch::switch_decider::SwitchDecider;
use crate::phase::ordering::first_index;
use crate::phase::ordering::layer_sweep_crossing_minimizer::CrossMinType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GreedySwitchHeuristic {
    greedy_switch_type: CrossMinType,
}

impl GreedySwitchHeuristic {
    pub(crate) fn new(greedy_switch_type: CrossMinType) -> Self {
        Self { greedy_switch_type }
    }

    pub(crate) fn minimize_crossings(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        free_layer_index: usize,
        forward: bool,
    ) -> bool {
        self.set_up_and_switch(graph, order, free_layer_index, forward, true)
    }

    pub(crate) fn set_first_layer_order(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        forward: bool,
    ) -> bool {
        let start_index = first_index(forward, order.len());
        self.set_up_and_switch(graph, order, start_index, forward, false)
    }

    fn set_up_and_switch(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        free_layer_index: usize,
        forward: bool,
        repeat_until_stable: bool,
    ) -> bool {
        let side = if forward {
            CrossingCountSide::West
        } else {
            CrossingCountSide::East
        };
        let filler =
            CrossingMatrixFiller::new(order[free_layer_index].len(), self.greedy_switch_type, side);
        let mut switch_decider = SwitchDecider::new(
            graph,
            order,
            free_layer_index,
            filler,
            self.greedy_switch_type == CrossMinType::OneSidedGreedySwitch,
        );

        if repeat_until_stable {
            self.continue_switching_until_no_improvement_in_layer(
                graph,
                order,
                free_layer_index,
                &mut switch_decider,
            )
        } else {
            self.sweep_downward_in_layer(graph, order, free_layer_index, &mut switch_decider)
        }
    }

    fn continue_switching_until_no_improvement_in_layer(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        free_layer_index: usize,
        switch_decider: &mut SwitchDecider,
    ) -> bool {
        let mut improved = false;
        loop {
            let continue_switching =
                self.sweep_downward_in_layer(graph, order, free_layer_index, switch_decider);
            improved |= continue_switching;
            if !continue_switching {
                return improved;
            }
        }
    }

    fn sweep_downward_in_layer(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        layer_index: usize,
        switch_decider: &mut SwitchDecider,
    ) -> bool {
        let mut continue_switching = false;
        let length_of_free_layer = order[layer_index].len();
        for upper_node_index in 0..length_of_free_layer.saturating_sub(1) {
            let lower_node_index = upper_node_index + 1;
            if switch_decider.does_switch_reduce_crossings(
                graph,
                order,
                upper_node_index,
                lower_node_index,
            ) {
                let upper_node = order[layer_index][upper_node_index];
                let lower_node = order[layer_index][lower_node_index];
                switch_decider.notify_of_switch(graph, order, upper_node, lower_node);
                order[layer_index].swap(upper_node_index, lower_node_index);
                continue_switching = true;
            }
        }
        continue_switching
    }
}
