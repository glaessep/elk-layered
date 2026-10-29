//! Port of ELK's `LayerSweepCrossingMinimizer`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/LayerSweepCrossingMinimizer.java

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::graph::{
    GraphArena, GraphId, GraphParent, LGraph, LNode, LNodeKind, NodeId, PortId, PortSide,
};
use crate::intermediate::greedyswitch::greedy_switch_heuristic::GreedySwitchHeuristic;
use crate::intermediate::preserveorder::model_order_node_comparator::ModelOrderNodeComparator;
use crate::intermediate::preserveorder::model_order_port_comparator::ModelOrderPortComparator;
use crate::options::{OrderingStrategy, PortConstraints};
use crate::phase::ordering::barycenter_heuristic::BarycenterHeuristic;
use crate::phase::ordering::counting::crossings_counter::ports_in_north_south_east_west_order;
use crate::phase::ordering::graph_info_holder::{
    GraphInfoHolder, initialize_crossing_minimization_port_ids,
    initialize_crossing_minimization_port_ids_hierarchy,
};
use crate::phase::ordering::layer_sweep_type_decider::LayerSweepTypeDecider;
use crate::phase::ordering::sweep_copy::SweepCopy;
use crate::phase::ordering::{first_free, first_index, is_not_end, next_index};
use crate::random::JavaRandom;

pub(crate) struct HierarchySweep {
    pub(crate) infos: Vec<GraphInfoHolder>,
    index_by_graph: HashMap<GraphId, usize>,
    pub(crate) changed: HashSet<usize>,
    random: JavaRandom,
    random_seed: i64,
    cross_min_type: CrossMinType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossMinType {
    Barycenter,
    OneSidedGreedySwitch,
    TwoSidedGreedySwitch,
}

impl CrossMinType {
    pub(crate) fn is_deterministic(self) -> bool {
        self != Self::Barycenter
    }

    fn always_improves(self) -> bool {
        self == Self::TwoSidedGreedySwitch
    }
}

impl HierarchySweep {
    pub(crate) fn new(
        graphs: &mut GraphArena,
        start: GraphId,
        include_children: bool,
        cross_min_type: CrossMinType,
    ) -> Self {
        let mut random = graphs[start].random.clone();
        let random_seed = random.next_long();
        graphs[start].random = random.clone();

        let mut sweep = Self {
            infos: Vec::new(),
            index_by_graph: HashMap::new(),
            changed: HashSet::new(),
            random,
            random_seed,
            cross_min_type,
        };
        sweep.initialize_graphs(graphs, start, include_children);
        sweep
    }

    fn initialize_graphs(&mut self, graphs: &GraphArena, start: GraphId, include_children: bool) {
        // Parents come before their children.
        let mut found = vec![start];
        let mut index = 0usize;
        while index < found.len() {
            let id = found[index];
            let graph = &graphs[id];
            let info_index = self.infos.len();
            self.index_by_graph.insert(id, info_index);
            let mut info = GraphInfoHolder::new(graph, self.cross_min_type);
            if info.parent.is_none() {
                self.random = info.random.clone();
            }
            // ELK links a nested graph to its node only when the children are laid out with the
            // parent. Here a nested graph is always linked, so a run without them leaves it out.
            let children = if include_children {
                graph
                    .layers()
                    .iter()
                    .flat_map(|layer| layer.nodes().iter().copied())
                    .filter_map(|node_index| graph.layerless_nodes[node_index].nested_graph)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            info.child_graphs = children.clone();
            self.infos.push(info);
            found.extend(children);
            index += 1;
        }

        for index in 0..self.infos.len() {
            let use_bottom_up = self.decide_use_bottom_up(graphs, index);
            self.infos[index].use_bottom_up = use_bottom_up;
        }
    }

    fn decide_use_bottom_up(&self, graphs: &GraphArena, info_index: usize) -> bool {
        let info = &self.infos[info_index];
        let Some(parent) = info.parent else {
            return true;
        };
        let parent_graph = &graphs[parent.graph];
        let parent_node_index = parent.node;
        let parent_node = &parent_graph.layerless_nodes[parent_node_index];
        let boundary = graphs[info.graph].options.hierarchical_sweepiness;

        if boundary < -1.0
            || parent_node.port_constraints.is_order_fixed()
            || fewer_than_two_in_out_parent_ports(parent_node)
        {
            return true;
        }

        if self.cross_min_type.is_deterministic() {
            return false;
        }

        let mut decider = LayerSweepTypeDecider::default();
        decider.decide(&graphs[info.graph], Some(parent_graph), boundary)
    }

    fn graphs_to_sweep_on(&self) -> Vec<usize> {
        self.infos
            .iter()
            .enumerate()
            .filter_map(|(index, info)| info.use_bottom_up.then_some(index))
            .rev()
            .collect()
    }

    fn minimize(&mut self, graphs: &mut GraphArena) -> bool {
        let graphs_to_sweep_on = self.graphs_to_sweep_on();
        for info_index in graphs_to_sweep_on {
            if self.infos[info_index].current_node_order.is_empty() {
                continue;
            }

            if self.cross_min_type.always_improves() {
                self.minimize_no_counter(graphs, info_index);
            } else if self.cross_min_type.is_deterministic() {
                self.minimize_with_counter(graphs, info_index);
            } else {
                self.compare_different_randomized_layouts(graphs, info_index);
            }

            if self.infos[info_index].parent.is_some() {
                self.set_port_order_on_parent_graph(graphs, info_index);
            }
        }

        // GraphInfoHolder and BarycenterHeuristic share each LGraph's Random in ELK.
        // Return every consumed stream even when no better node order was found, so
        // later processors continue from the same state as the source implementation.
        for (index, info) in self.infos.iter().enumerate() {
            graphs[info.graph].random = self.barycenter_random(index);
        }
        self.transfer_node_and_port_orders_to_graph(graphs)
    }

    fn minimize_no_counter(&mut self, graphs: &mut GraphArena, info_index: usize) {
        let mut is_forward_sweep = self.random.next_bool();
        loop {
            let mut heuristic = GreedySwitchHeuristic::new(self.cross_min_type);
            let mut improved = {
                let graph = &graphs[self.infos[info_index].graph];
                heuristic.set_first_layer_order(
                    graph,
                    &mut self.infos[info_index].current_node_order,
                    is_forward_sweep,
                )
            };
            improved |= self.sweep_reducing_crossings_greedy(
                graphs,
                info_index,
                &mut heuristic,
                is_forward_sweep,
            );
            if !improved {
                break;
            }
            is_forward_sweep = !is_forward_sweep;
        }
        self.set_currently_best_node_orders(graphs);
    }

    fn minimize_with_counter(&mut self, graphs: &mut GraphArena, info_index: usize) -> f64 {
        let mut is_forward_sweep = self.random.next_bool();
        // ELK also reads FIRST_TRY_WITH_INITIAL_ORDER here. `minimize_one_sided_greedy_switch`
        // says why it is safe to leave out.
        {
            let graph = &graphs[self.infos[info_index].graph];
            let mut heuristic = GreedySwitchHeuristic::new(self.cross_min_type);
            heuristic.set_first_layer_order(
                graph,
                &mut self.infos[info_index].current_node_order,
                is_forward_sweep,
            );
            self.sweep_reducing_crossings_greedy(
                graphs,
                info_index,
                &mut heuristic,
                is_forward_sweep,
            );
        }

        let mut crossings_in_graph = self.count_current_number_of_crossings(graphs, info_index);
        loop {
            self.set_currently_best_node_orders(graphs);
            if crossings_in_graph == 0.0 {
                return 0.0;
            }

            is_forward_sweep = !is_forward_sweep;
            let old_number_of_crossings = crossings_in_graph;
            let mut heuristic = GreedySwitchHeuristic::new(self.cross_min_type);
            self.sweep_reducing_crossings_greedy(
                graphs,
                info_index,
                &mut heuristic,
                is_forward_sweep,
            );
            crossings_in_graph = self.count_current_number_of_crossings(graphs, info_index);
            if !sweep_improved_crossings(old_number_of_crossings, crossings_in_graph) {
                return old_number_of_crossings;
            }
        }
    }

    fn compare_different_randomized_layouts(&mut self, graphs: &mut GraphArena, info_index: usize) {
        self.random.set_seed(self.random_seed);
        if self.infos[info_index].parent.is_none() {
            self.infos[info_index].random = self.random.clone();
        }
        self.changed.clear();

        // ELK InternalProperties uses "firstTryWithInitialOrder" for BOTH initial-order
        // flags, and Property equality is ID-based. Clearing SECOND also clears FIRST,
        // so only run zero preserves model order; run one already randomizes its first layer.
        let first_try_with_initial_order = graphs[self.infos[info_index].graph]
            .options
            .consider_model_order_strategy
            != OrderingStrategy::None;
        let thoroughness = graphs[self.infos[info_index].graph]
            .options
            .thoroughness
            .max(1);
        let use_node_port_order_counter =
            self.use_node_port_order_crossing_counter(graphs, info_index);
        let mut best_crossings = f64::MAX;
        for run_index in 0..thoroughness {
            let crossings = self.minimize_barycenter_with_counter(
                graphs,
                info_index,
                first_try_with_initial_order && run_index == 0,
                use_node_port_order_counter,
            );
            if crossings < best_crossings {
                best_crossings = crossings;
                self.save_all_node_orders_of_changed_graphs(graphs);
                if best_crossings == 0.0 {
                    break;
                }
            }
        }
    }

    fn minimize_barycenter_with_counter(
        &mut self,
        graphs: &mut GraphArena,
        info_index: usize,
        first_try_with_initial_order: bool,
        use_node_port_order_counter: bool,
    ) -> f64 {
        let mut is_forward_sweep = self.next_sweep_direction(info_index);
        let initial_crossings =
            self.count_current_crossing_score(graphs, info_index, use_node_port_order_counter);
        if initial_crossings == 0.0 && first_try_with_initial_order {
            self.set_currently_best_node_orders(graphs);
            return 0.0;
        }

        let should_set_first_layer_order = {
            let graph = &graphs[self.infos[info_index].graph];
            !first_try_with_initial_order
                || graph.options.consider_model_order_strategy == OrderingStrategy::None
        };
        if should_set_first_layer_order {
            self.with_barycenter_heuristic(graphs, info_index, |heuristic, graph, order| {
                heuristic.set_first_layer_order(graph, order, is_forward_sweep);
            });
        } else {
            is_forward_sweep = first_try_with_initial_order;
        }

        self.sweep_reducing_crossings_barycenter(
            graphs,
            info_index,
            is_forward_sweep,
            true,
            first_try_with_initial_order,
        );

        let mut crossings_in_graph =
            self.count_current_crossing_score(graphs, info_index, use_node_port_order_counter);
        loop {
            self.set_currently_best_node_orders(graphs);
            if crossings_in_graph == 0.0 {
                return 0.0;
            }

            is_forward_sweep = !is_forward_sweep;
            let old_number_of_crossings = crossings_in_graph;
            self.sweep_reducing_crossings_barycenter(
                graphs,
                info_index,
                is_forward_sweep,
                false,
                false,
            );
            crossings_in_graph =
                self.count_current_crossing_score(graphs, info_index, use_node_port_order_counter);
            if !sweep_improved_crossings(old_number_of_crossings, crossings_in_graph) {
                return old_number_of_crossings;
            }
        }
    }

    pub(crate) fn sweep_reducing_crossings_barycenter(
        &mut self,
        graphs: &mut GraphArena,
        info_index: usize,
        forward: bool,
        first_sweep: bool,
        try_with_initial_order: bool,
    ) -> bool {
        let length = self.infos[info_index].current_node_order.len();
        if length == 0 {
            return false;
        }

        self.with_barycenter_heuristic(graphs, info_index, |heuristic, graph, order| {
            heuristic.port_distributor.distribute_ports_while_sweeping(
                graph,
                order,
                first_index(forward, length),
                forward,
            );
        });

        let first_layer =
            self.infos[info_index].current_node_order[first_index(forward, length)].clone();
        let mut improved = self.sweep_in_hierarchical_nodes(
            graphs,
            info_index,
            &first_layer,
            forward,
            first_sweep,
        );
        let mut index = first_free(forward, length);
        while is_not_end(length, index, forward) {
            let free_layer_index = index as usize;
            let first_sweep_for_heuristic = first_sweep && !try_with_initial_order;
            self.with_barycenter_heuristic(graphs, info_index, |heuristic, graph, order| {
                heuristic.minimize_crossings(
                    graph,
                    order,
                    free_layer_index,
                    forward,
                    first_sweep_for_heuristic,
                );
                heuristic.port_distributor.distribute_ports_while_sweeping(
                    graph,
                    order,
                    free_layer_index,
                    forward,
                );
            });
            let layer = self.infos[info_index].current_node_order[free_layer_index].clone();
            improved |=
                self.sweep_in_hierarchical_nodes(graphs, info_index, &layer, forward, first_sweep);
            index = next_index(index, forward);
        }

        self.changed.insert(info_index);
        improved
    }

    fn sweep_reducing_crossings_greedy(
        &mut self,
        graphs: &mut GraphArena,
        info_index: usize,
        heuristic: &mut GreedySwitchHeuristic,
        forward: bool,
    ) -> bool {
        let length = self.infos[info_index].current_node_order.len();
        if length == 0 {
            return false;
        }

        let mut improved =
            self.distribute_greedy_ports(graphs, info_index, first_index(forward, length), forward);
        let first_layer =
            self.infos[info_index].current_node_order[first_index(forward, length)].clone();
        improved |=
            self.sweep_in_hierarchical_nodes(graphs, info_index, &first_layer, forward, false);
        let mut index = first_free(forward, length);
        while is_not_end(length, index, forward) {
            let free_layer_index = index as usize;
            {
                let graph = &graphs[self.infos[info_index].graph];
                improved |= heuristic.minimize_crossings(
                    graph,
                    &mut self.infos[info_index].current_node_order,
                    free_layer_index,
                    forward,
                );
            }
            improved |= self.distribute_greedy_ports(graphs, info_index, free_layer_index, forward);
            let layer = self.infos[info_index].current_node_order[free_layer_index].clone();
            improved |=
                self.sweep_in_hierarchical_nodes(graphs, info_index, &layer, forward, false);
            index = next_index(index, forward);
        }

        self.changed.insert(info_index);
        improved
    }

    /// The port distribution of `LayerSweepCrossingMinimizer.sweepReducingCrossings` for the
    /// greedy switch. The barycenter type distributes inside its heuristic, so it does nothing
    /// here.
    fn distribute_greedy_ports(
        &mut self,
        graphs: &mut GraphArena,
        info_index: usize,
        layer_index: usize,
        forward: bool,
    ) -> bool {
        let info = &mut self.infos[info_index];
        let (graph, others) = graphs.split_mut(info.graph);
        info.port_distributor.distribute_ports_while_sweeping(
            graph,
            others,
            &info.current_node_order,
            layer_index,
            forward,
        )
    }

    fn sweep_in_hierarchical_nodes(
        &mut self,
        graphs: &mut GraphArena,
        parent_info_index: usize,
        layer: &[NodeId],
        forward: bool,
        first_sweep: bool,
    ) -> bool {
        let mut improved = false;
        for node in layer {
            let Some(child_index) =
                self.child_info_for_parent_node(graphs, parent_info_index, *node)
            else {
                continue;
            };
            if self.infos[child_index].use_bottom_up {
                continue;
            }
            improved |=
                self.sweep_in_hierarchical_node(graphs, child_index, *node, forward, first_sweep);
        }
        improved
    }

    fn sweep_in_hierarchical_node(
        &mut self,
        graphs: &mut GraphArena,
        child_info_index: usize,
        parent_node_index: NodeId,
        forward: bool,
        first_sweep: bool,
    ) -> bool {
        let child_len = self.infos[child_info_index].current_node_order.len();
        if child_len == 0 {
            return false;
        }

        let start_index = first_index(forward, child_len);
        let first_node = self.infos[child_info_index]
            .current_node_order
            .get(start_index)
            .and_then(|layer| layer.first())
            .copied();

        if let Some(first_node) = first_node {
            let child_graph = &graphs[self.infos[child_info_index].graph];
            if child_graph.layerless_nodes[first_node].kind() == LNodeKind::ExternalPort {
                let parent_graph = &graphs[self.infos[child_info_index]
                    .parent
                    .expect("child graph should have parent graph")
                    .graph];
                let sorted = sort_port_dummies_by_port_positions(
                    parent_graph,
                    child_graph,
                    parent_node_index,
                    &self.infos[child_info_index].current_node_order[start_index],
                    if forward {
                        PortSide::West
                    } else {
                        PortSide::East
                    },
                );
                self.infos[child_info_index].current_node_order[start_index] = sorted;
            } else {
                let child_graph = &graphs[self.infos[child_info_index].graph];
                match self.cross_min_type {
                    CrossMinType::Barycenter => {
                        self.with_barycenter_heuristic(
                            graphs,
                            child_info_index,
                            |heuristic, graph, order| {
                                heuristic.set_first_layer_order(graph, order, forward);
                            },
                        );
                    }
                    CrossMinType::OneSidedGreedySwitch | CrossMinType::TwoSidedGreedySwitch => {
                        let mut heuristic = GreedySwitchHeuristic::new(self.cross_min_type);
                        heuristic.set_first_layer_order(
                            child_graph,
                            &mut self.infos[child_info_index].current_node_order,
                            forward,
                        );
                    }
                }
            }
        }

        let improved = match self.cross_min_type {
            CrossMinType::Barycenter => self.sweep_reducing_crossings_barycenter(
                graphs,
                child_info_index,
                forward,
                first_sweep,
                false,
            ),
            CrossMinType::OneSidedGreedySwitch | CrossMinType::TwoSidedGreedySwitch => {
                let mut heuristic = GreedySwitchHeuristic::new(self.cross_min_type);
                self.sweep_reducing_crossings_greedy(
                    graphs,
                    child_info_index,
                    &mut heuristic,
                    forward,
                )
            }
        };
        self.sort_ports_by_dummy_positions_in_last_layer(graphs, child_info_index, forward);
        improved
    }

    fn sort_ports_by_dummy_positions_in_last_layer(
        &mut self,
        graphs: &mut GraphArena,
        child_info_index: usize,
        on_right_most_layer: bool,
    ) {
        let child_len = self.infos[child_info_index].current_node_order.len();
        if child_len == 0 {
            return;
        }
        let end_index = first_index(!on_right_most_layer, child_len);
        let last_layer = self.infos[child_info_index].current_node_order[end_index].clone();
        if last_layer.is_empty() {
            return;
        }
        let first = if on_right_most_layer {
            0
        } else {
            last_layer.len() - 1
        };
        let child_graph = &graphs[self.infos[child_info_index].graph];
        if child_graph.layerless_nodes[last_layer[first]].kind() != LNodeKind::ExternalPort {
            return;
        }

        let Some(GraphParent {
            graph: parent_path,
            node: parent_node_index,
        }) = self.infos[child_info_index].parent
        else {
            return;
        };
        let parent_graph = &graphs[parent_path];
        let parent_order = parent_port_order_by_dummy_layer(
            child_graph,
            parent_graph,
            &last_layer,
            parent_node_index,
            on_right_most_layer,
        );
        let parent_graph = &mut graphs[parent_path];
        apply_parent_hierarchical_port_order_on_sweep_side(
            parent_graph,
            parent_node_index,
            on_right_most_layer,
            &parent_order,
        );
    }

    fn set_port_order_on_parent_graph(&mut self, graphs: &mut GraphArena, info_index: usize) {
        if !self.infos[info_index].has_external_ports {
            return;
        }
        let Some(best_sweep) = self.infos[info_index]
            .get_best_sweep(self.cross_min_type)
            .cloned()
        else {
            return;
        };
        for on_right_most_layer in [true, false] {
            self.sort_ports_by_sweep_copy(graphs, info_index, &best_sweep, on_right_most_layer);
        }
        if let Some(parent) = self.infos[info_index].parent {
            graphs[parent.graph].layerless_nodes[parent.node].port_constraints =
                PortConstraints::FixedOrder;
        }
    }

    fn sort_ports_by_sweep_copy(
        &mut self,
        graphs: &mut GraphArena,
        info_index: usize,
        sweep: &SweepCopy,
        on_right_most_layer: bool,
    ) {
        if sweep.node_order.is_empty() {
            return;
        }
        let end_index = first_index(!on_right_most_layer, sweep.node_order.len());
        let last_layer = sweep.node_order[end_index].clone();
        if last_layer.is_empty() {
            return;
        }
        let child_graph = &graphs[self.infos[info_index].graph];
        let first = if on_right_most_layer {
            0
        } else {
            last_layer.len() - 1
        };
        if child_graph.layerless_nodes[last_layer[first]].kind() != LNodeKind::ExternalPort {
            return;
        }
        let Some(GraphParent {
            graph: parent_path,
            node: parent_node_index,
        }) = self.infos[info_index].parent
        else {
            return;
        };
        let parent_graph = &graphs[parent_path];
        let parent_order = parent_port_order_by_dummy_layer(
            child_graph,
            parent_graph,
            &last_layer,
            parent_node_index,
            on_right_most_layer,
        );
        let parent_graph = &mut graphs[parent_path];
        apply_parent_hierarchical_port_order_on_sweep_side(
            parent_graph,
            parent_node_index,
            on_right_most_layer,
            &parent_order,
        );
    }

    fn with_barycenter_heuristic<R>(
        &mut self,
        graphs: &mut GraphArena,
        info_index: usize,
        action: impl FnOnce(&mut BarycenterHeuristic, &mut LGraph, &mut [Vec<NodeId>]) -> R,
    ) -> R {
        let path = self.infos[info_index].graph;
        let mut heuristic = self.infos[info_index]
            .barycenter_heuristic
            .take()
            .expect("barycenter graph info should own a heuristic");
        heuristic.random = self.barycenter_random(info_index);

        let result = {
            let graph = &mut graphs[path];
            let order = &mut self.infos[info_index].current_node_order;
            action(&mut heuristic, graph, order)
        };

        self.set_barycenter_random(info_index, heuristic.random.clone());
        self.infos[info_index].barycenter_heuristic = Some(heuristic);
        result
    }

    fn next_sweep_direction(&mut self, _info_index: usize) -> bool {
        self.random.next_bool()
    }

    fn barycenter_random(&self, info_index: usize) -> JavaRandom {
        if self.infos[info_index].parent.is_none() {
            self.random.clone()
        } else {
            self.infos[info_index].random.clone()
        }
    }

    fn set_barycenter_random(&mut self, info_index: usize, random: JavaRandom) {
        if self.infos[info_index].parent.is_none() {
            self.random = random.clone();
        }
        self.infos[info_index].random = random;
    }

    fn count_current_number_of_crossings(&mut self, graphs: &GraphArena, info_index: usize) -> f64 {
        let mut total = 0.0f64;
        let mut stack = vec![info_index];
        while let Some(current) = stack.pop() {
            let info = &mut self.infos[current];
            let graph = &graphs[info.graph];
            total += info
                .crossings_counter
                .count_all_crossings_in_order(graph, &info.current_node_order);
            // `infos` and `index_by_graph` are separate fields, so the ids need no copy.
            for child_graph in &self.infos[current].child_graphs {
                if let Some(child_index) = self.index_by_graph.get(child_graph).copied()
                    && !self.infos[child_index].use_bottom_up
                {
                    stack.push(child_index);
                }
            }
        }
        total
    }

    #[allow(clippy::eq_op, clippy::nonminimal_bool)]
    pub(crate) fn use_node_port_order_crossing_counter(
        &self,
        graphs: &GraphArena,
        info_index: usize,
    ) -> bool {
        let graph = &graphs[self.infos[info_index].graph];
        // layout-base compatibility: port influence alone must not enable this counter gate.
        graph
            .options
            .consider_model_order_crossing_counter_node_influence
            != 0.0
            || graph
                .options
                .consider_model_order_crossing_counter_node_influence
                != 0.0
    }

    fn count_current_crossing_score(
        &mut self,
        graphs: &GraphArena,
        info_index: usize,
        use_node_port_order_counter: bool,
    ) -> f64 {
        if use_node_port_order_counter {
            self.count_current_number_of_crossings_node_port_order(graphs, info_index)
        } else {
            self.count_current_number_of_crossings(graphs, info_index)
        }
    }

    fn count_current_number_of_crossings_node_port_order(
        &mut self,
        graphs: &GraphArena,
        info_index: usize,
    ) -> f64 {
        let info = &mut self.infos[info_index];
        let graph = &graphs[info.graph];
        let mut model_order_influence = 0.0;
        let model_order_strategy = graph.options.consider_model_order_strategy;
        if model_order_strategy != OrderingStrategy::None {
            model_order_influence += graph
                .options
                .consider_model_order_crossing_counter_node_influence
                * count_model_order_node_changes(
                    graph,
                    &info.current_node_order,
                    model_order_strategy,
                ) as f64;
            model_order_influence += graph
                .options
                .consider_model_order_crossing_counter_port_influence
                * count_model_order_port_changes(graph, &info.current_node_order) as f64;
        }

        let mut total = info
            .crossings_counter
            .count_all_crossings_in_order(graph, &info.current_node_order)
            + model_order_influence;

        // The count below borrows `self` mutably, so each id is looked up again, not copied.
        for position in 0..self.infos[info_index].child_graphs.len() {
            let child_graph = &self.infos[info_index].child_graphs[position];
            if let Some(child_index) = self.index_by_graph.get(child_graph).copied()
                && !self.infos[child_index].use_bottom_up
            {
                total += self.count_current_number_of_crossings(graphs, child_index);
            }
        }

        total
    }

    pub(crate) fn set_currently_best_node_orders(&mut self, graphs: &GraphArena) {
        let changed = self.changed.iter().copied().collect::<Vec<_>>();
        for index in changed {
            let graph = &graphs[self.infos[index].graph];
            self.infos[index].set_currently_best_node_and_port_order(graph);
        }
    }

    pub(crate) fn save_all_node_orders_of_changed_graphs(&mut self, _graphs: &GraphArena) {
        let changed = self.changed.iter().copied().collect::<Vec<_>>();
        for index in changed {
            if let Some(copy) = self.infos[index].currently_best_node_and_port_order.clone() {
                self.infos[index].set_best_node_and_port_order(copy);
            }
        }
    }

    fn transfer_node_and_port_orders_to_graph(&self, graphs: &mut GraphArena) -> bool {
        let mut changed = false;
        for index in 0..self.infos.len() {
            let Some(best_sweep) = self.infos[index]
                .get_best_sweep(self.cross_min_type)
                .cloned()
            else {
                continue;
            };
            let graph = &mut graphs[self.infos[index].graph];
            changed |= best_sweep.transfer_node_and_port_orders_to_graph(graph, true);
        }
        changed
    }

    /// The info of the graph inside a node of the graph that `parent_info_index` describes.
    fn child_info_for_parent_node(
        &self,
        graphs: &GraphArena,
        parent_info_index: usize,
        parent_node_index: NodeId,
    ) -> Option<usize> {
        let parent = self.infos[parent_info_index].graph;
        let child = graphs[parent].layerless_nodes[parent_node_index].nested_graph?;
        self.index_by_graph.get(&child).copied()
    }
}

/// Ports `LayerSweepCrossingMinimizer.process` for the graph `start`. With `include_children`,
/// the sweep also orders the graphs inside its nodes, as ELK does for `INCLUDE_CHILDREN`.
pub(crate) fn minimize_crossings_layer_sweep(
    graphs: &mut GraphArena,
    start: GraphId,
    include_children: bool,
    cross_min_type: CrossMinType,
) -> bool {
    let graph = &graphs[start];
    let empty_graph =
        graph.layers().is_empty() || graph.layers().iter().all(|layer| layer.nodes().is_empty());
    let single_node = graph.layers().len() == 1 && graph.layers()[0].nodes().len() == 1;
    if empty_graph {
        let hierarchical_graphs = include_children
            && graph
                .layerless_nodes
                .iter()
                .filter_map(|node| node.nested_graph)
                .any(|nested| {
                    let nested = &graphs[nested];
                    !nested.layers().is_empty()
                        && nested
                            .layers()
                            .iter()
                            .any(|layer| !layer.nodes().is_empty())
                });
        if !hierarchical_graphs {
            return false;
        }
    }
    if single_node && !include_children {
        return false;
    }

    if include_children {
        initialize_crossing_minimization_port_ids_hierarchy(graphs);
    } else {
        initialize_crossing_minimization_port_ids(&mut graphs[start]);
    }
    let mut sweep = HierarchySweep::new(graphs, start, include_children, cross_min_type);
    sweep.minimize(graphs)
}

/// The condition of `do { ... } while (oldNumberOfCrossings > crossingsInGraph)`. A count can be
/// `NaN`, and then the loop ends: no comparison with `NaN` is true.
fn sweep_improved_crossings(old_number_of_crossings: f64, crossings_in_graph: f64) -> bool {
    old_number_of_crossings > crossings_in_graph
}

fn fewer_than_two_in_out_parent_ports(parent_node: &LNode) -> bool {
    let east = parent_node
        .ports
        .iter()
        .filter(|port| port.side == PortSide::East)
        .count();
    let west = parent_node
        .ports
        .iter()
        .filter(|port| port.side == PortSide::West)
        .count();
    east < 2 && west < 2
}

fn sort_port_dummies_by_port_positions(
    parent_graph: &LGraph,
    child_graph: &LGraph,
    parent_node_index: NodeId,
    layer_close_to_node_edge: &[NodeId],
    side: PortSide,
) -> Vec<NodeId> {
    let parent_node = &parent_graph.layerless_nodes[parent_node_index];
    let mut sorted = Vec::with_capacity(layer_close_to_node_edge.len());
    for port in ports_in_north_south_east_west_order(parent_graph, parent_node_index, side) {
        let parent_port = &parent_node.ports[port.port];
        if !parent_port.inside_connections {
            continue;
        }
        if let Some(dummy) = parent_port.port_dummy.as_ref()
            && dummy.graph_id == child_graph.id
        {
            sorted.push(dummy.node);
        }
    }

    if sorted.len() == layer_close_to_node_edge.len() {
        sorted
    } else {
        layer_close_to_node_edge.to_vec()
    }
}

fn parent_port_order_by_dummy_layer(
    child_graph: &LGraph,
    parent_graph: &LGraph,
    dummy_layer: &[NodeId],
    parent_node_index: NodeId,
    on_right_most_layer: bool,
) -> Vec<PortId> {
    let mut order = Vec::new();
    let iter: Box<dyn Iterator<Item = NodeId> + '_> = if on_right_most_layer {
        Box::new(dummy_layer.iter().copied())
    } else {
        Box::new(dummy_layer.iter().copied().rev())
    };

    for dummy_node in iter {
        if let Some(port) =
            parent_port_for_dummy_node(parent_graph, parent_node_index, child_graph.id, dummy_node)
        {
            order.push(port);
            continue;
        }
        let Some(origin) = child_graph.layerless_nodes[dummy_node]
            .external_port()
            .and_then(|external| external.origin_port.as_ref())
        else {
            continue;
        };
        if origin.graph_id == parent_graph.id && origin.port.node == parent_node_index {
            order.push(origin.port.port);
        }
    }
    order
}

fn parent_port_for_dummy_node(
    parent_graph: &LGraph,
    parent_node_index: NodeId,
    child_graph_id: GraphId,
    dummy_node: NodeId,
) -> Option<PortId> {
    parent_graph
        .layerless_nodes
        .get(parent_node_index)?
        .ports
        .iter_with_ids()
        .find_map(|(port_index, port)| {
            let dummy = port.port_dummy.as_ref()?;
            (dummy.graph_id == child_graph_id && dummy.node == dummy_node).then_some(port_index)
        })
}

pub(crate) fn apply_parent_hierarchical_port_order_on_sweep_side(
    parent_graph: &mut LGraph,
    parent_node_index: NodeId,
    on_right_most_layer: bool,
    ordered_ports: &[PortId],
) {
    if ordered_ports.is_empty() {
        return;
    }

    let sweep_side = if on_right_most_layer {
        PortSide::East
    } else {
        PortSide::West
    };
    // The ports on the sweep side and the dummies in the layer need not match: a dummy on a
    // north or south side stands in the layer too, and a port beyond the dummies keeps its place.
    // `LayerSweepCrossingMinimizer.sortPortsByDummyPositionsInLastLayer` sets the ports one by one.
    let mut new_order =
        Vec::with_capacity(parent_graph.layerless_nodes[parent_node_index].ports.len());
    let mut ordered_iter = ordered_ports.iter().copied();

    for (index, port) in parent_graph.layerless_nodes[parent_node_index]
        .ports
        .iter_with_ids()
    {
        if port.side == sweep_side
            && port.inside_connections
            && let Some(next) = ordered_iter.next()
        {
            new_order.push(next);
            continue;
        }
        new_order.push(index);
    }

    let _changed = parent_graph.reorder_node_ports(parent_node_index, new_order);
}

fn cached_long_edge_target_node_orders(graph: &LGraph, node: NodeId) -> HashMap<NodeId, usize> {
    let mut target_node_model_order: HashMap<NodeId, usize> = HashMap::new();
    for port in &graph.layerless_nodes[node].ports {
        let Some(target_node) = port.long_edge_target_node else {
            continue;
        };
        let Some(edge_index) = port.outgoing_edges().first().copied() else {
            continue;
        };
        let edge = &graph.edges[edge_index];
        if !edge.reversed {
            let edge_order = edge.model_order.unwrap_or(0);
            target_node_model_order
                .entry(target_node)
                .and_modify(|order| *order = (*order).min(edge_order))
                .or_insert(edge_order);
        }
    }
    target_node_model_order
}

pub(crate) fn count_model_order_node_changes(
    graph: &LGraph,
    layers: &[Vec<NodeId>],
    strategy: OrderingStrategy,
) -> usize {
    let mut previous_layer_index = None;
    let mut wrong_model_order = 0usize;
    for layer in layers {
        let previous_layer = previous_layer_index
            .and_then(|index| layers.get(index))
            .unwrap_or(&layers[0]);
        let mut comparator = ModelOrderNodeComparator::new(graph, previous_layer, strategy);
        for i in 0..layer.len() {
            for j in (i + 1)..layer.len() {
                if graph.layerless_nodes[layer[i]].model_order.is_some()
                    && graph.layerless_nodes[layer[j]].model_order.is_some()
                    && comparator.compare(layer[i], layer[j]) == Ordering::Greater
                {
                    wrong_model_order += 1;
                }
            }
        }
        previous_layer_index = Some(previous_layer_index.map_or(0, |index| index + 1));
    }
    wrong_model_order
}

pub(crate) fn count_model_order_port_changes(graph: &LGraph, layers: &[Vec<NodeId>]) -> usize {
    let mut previous_layer_index = None;
    let mut wrong_model_order = 0usize;
    for layer in layers {
        let previous_layer = previous_layer_index
            .and_then(|index| layers.get(index))
            .unwrap_or(&layers[0]);
        for node in layer {
            let target_orders = cached_long_edge_target_node_orders(graph, *node);
            let mut comparator = ModelOrderPortComparator::new(
                graph,
                *node,
                previous_layer,
                graph.options.consider_model_order_strategy,
                &target_orders,
                graph.options.consider_model_order_port_model_order,
            );
            let ports = &graph.layerless_nodes[*node].ports;
            for i in 0..ports.len() {
                for j in (i + 1)..ports.len() {
                    if let (Some(left), Some(right)) = (ports.id_at(i), ports.id_at(j))
                        && comparator.compare(left, right) == Ordering::Greater
                    {
                        wrong_model_order += 1;
                    }
                }
            }
        }
        previous_layer_index = Some(previous_layer_index.map_or(0, |index| index + 1));
    }
    wrong_model_order
}
