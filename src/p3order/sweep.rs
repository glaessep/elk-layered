//! Layer sweep crossing minimization.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/LayerSweepCrossingMinimizer.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/BarycenterHeuristic.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/ModelOrderBarycenterHeuristic.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/GraphInfoHolder.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/AbstractBarycenterPortDistributor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/NodeRelativePortDistributor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/LayerTotalPortDistributor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/ForsterConstraintResolver.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/GreedySwitchHeuristic.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/SwitchDecider.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/CrossingMatrixFiller.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/BetweenLayerEdgeTwoNodeCrossingsCounter.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/NorthSouthEdgeNeighbouringNodeCrossingsCounter.java

use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::{
    EdgeId, GraphArena, GraphId, GraphParent, LGraph, LNode, LNodeKind, NodeId, OtherGraphs,
    PortId, PortRef, PortSide, PortType,
};
use crate::options::{OrderingStrategy, PortConstraints};
use crate::p3order::counting::{
    BinaryIndexedTree, CrossingsCounter, first_port_on_side, north_south_ports_with_incident_edges,
    port_degree, ports_in_north_south_east_west_order,
};
use crate::random::JavaRandom;

use super::{
    GraphInfoHolder, SweepCopy, count_model_order_node_changes, count_model_order_port_changes,
    initialize_crossing_minimization_port_ids, initialize_crossing_minimization_port_ids_hierarchy,
    update_bigger_and_smaller_associations,
};

#[derive(Debug, Clone)]
struct HierarchyGraphInfo {
    graph: GraphId,
    current_node_order: Vec<Vec<NodeId>>,
    currently_best_node_and_port_order: Option<SweepCopy>,
    best_node_and_port_order: Option<SweepCopy>,
    barycenter_heuristic: Option<BarycenterHeuristic>,
    /// `GraphInfoHolder.crossingsCounter`, whose port positions outlive each count.
    crossings_counter: CrossingsCounter,
    /// `GraphInfoHolder.portDistributor`.
    port_distributor: PortDistributor,
    random: JavaRandom,
    child_graphs: Vec<GraphId>,
    /// The node this graph lies in, copied from the graph once. `None` for the root.
    parent: Option<GraphParent>,
    has_external_ports: bool,
    use_bottom_up: bool,
}

impl HierarchyGraphInfo {
    fn new(graph: &LGraph, cross_min_type: CrossMinType) -> Self {
        let mut random = graph.random.clone();
        let current_node_order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let mut port_distributor =
            PortDistributor::create(cross_min_type, &mut random, &current_node_order);
        // The barycenter type keeps its distributor in its heuristic. The greedy switch types
        // keep theirs here.
        let barycenter_heuristic = (cross_min_type == CrossMinType::Barycenter).then(|| {
            BarycenterHeuristic::new(graph, random.clone(), port_distributor.hand_to_heuristic())
        });
        Self {
            graph: graph.id,
            current_node_order,
            currently_best_node_and_port_order: None,
            best_node_and_port_order: None,
            barycenter_heuristic,
            crossings_counter: CrossingsCounter::default(),
            port_distributor,
            random,
            child_graphs: Vec::new(),
            parent: graph.parent,
            has_external_ports: graph.graph_properties.external_ports,
            use_bottom_up: true,
        }
    }

    fn set_currently_best_node_and_port_order(&mut self, graph: &LGraph) {
        self.currently_best_node_and_port_order =
            Some(SweepCopy::new(graph, &self.current_node_order));
    }

    fn set_best_node_and_port_order(&mut self, copy: SweepCopy) {
        self.best_node_and_port_order = Some(copy);
    }

    fn get_best_sweep(&self, cross_min_type: CrossMinType) -> Option<&SweepCopy> {
        if cross_min_type.is_deterministic() {
            self.currently_best_node_and_port_order.as_ref()
        } else {
            self.best_node_and_port_order.as_ref()
        }
    }
}

struct HierarchySweep {
    infos: Vec<HierarchyGraphInfo>,
    index_by_graph: HashMap<GraphId, usize>,
    changed: HashSet<usize>,
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
    fn is_deterministic(self) -> bool {
        self != Self::Barycenter
    }

    fn always_improves(self) -> bool {
        self == Self::TwoSidedGreedySwitch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortDistributorKind {
    NodeRelative,
    LayerTotal,
}

fn create_port_distributor_kind(
    cross_min_type: CrossMinType,
    random: &mut JavaRandom,
) -> PortDistributorKind {
    if cross_min_type == CrossMinType::TwoSidedGreedySwitch || random.next_bool() {
        PortDistributorKind::NodeRelative
    } else {
        PortDistributorKind::LayerTotal
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
struct BarycenterState {
    summed_weight: f64,
    degree: usize,
    barycenter: Option<f64>,
    visited: bool,
}

#[derive(Debug, Clone)]
struct BarycenterPortDistributor {
    kind: PortDistributorKind,
    port_ranks: HashMap<usize, f64>,
    port_barycenters: HashMap<usize, f64>,
    node_positions: HashMap<NodeId, usize>,
    min_barycenter: f64,
    max_barycenter: f64,
}

impl BarycenterPortDistributor {
    fn new(kind: PortDistributorKind, order: &[Vec<NodeId>]) -> Self {
        let node_positions = order
            .iter()
            .flat_map(|layer| {
                layer
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(position, node)| (node, position))
            })
            .collect::<HashMap<_, _>>();
        Self {
            kind,
            port_ranks: HashMap::new(),
            port_barycenters: HashMap::new(),
            node_positions,
            min_barycenter: 0.0,
            max_barycenter: 0.0,
        }
    }

    fn distribute_ports_while_sweeping(
        &mut self,
        graph: &mut LGraph,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) {
        if order.is_empty() || current_index >= order.len() {
            return;
        }

        self.update_node_positions(order, current_index);
        let free_layer = order[current_index].clone();
        let side = if forward {
            PortSide::West
        } else {
            PortSide::East
        };

        if !is_first_layer(order, current_index, forward) {
            let fixed_layer_index = if forward {
                current_index - 1
            } else {
                current_index + 1
            };
            let fixed_layer = order[fixed_layer_index].clone();

            self.calculate_port_ranks(graph, &fixed_layer, port_type_for(forward));
            for node in &free_layer {
                self.distribute_ports(graph, *node, side, order);
            }

            self.calculate_port_ranks(graph, &free_layer, port_type_for(!forward));
            for node in &fixed_layer {
                if graph.layerless_nodes[*node].nested_graph.is_none() {
                    self.distribute_ports(graph, *node, side.opposed(), order);
                }
            }
        } else {
            for node in &free_layer {
                self.distribute_ports(graph, *node, side, order);
            }
        }
    }

    fn calculate_port_ranks(&mut self, graph: &LGraph, layer: &[NodeId], port_type: PortType) {
        let mut consumed_rank = 0.0;
        for node in layer {
            consumed_rank += match self.kind {
                PortDistributorKind::NodeRelative => {
                    self.calculate_node_relative_port_ranks(graph, *node, consumed_rank, port_type)
                }
                PortDistributorKind::LayerTotal => {
                    self.calculate_layer_total_port_ranks(graph, *node, consumed_rank, port_type)
                }
            };
        }
    }

    fn rank_of(&self, graph: &LGraph, port: PortRef) -> f64 {
        crossing_minimization_port_id(graph, port)
            .and_then(|id| self.port_ranks.get(&id).copied())
            .unwrap_or(0.0)
    }

    fn distribute_ports(
        &mut self,
        graph: &mut LGraph,
        node: NodeId,
        side: PortSide,
        order: &[Vec<NodeId>],
    ) {
        if graph.layerless_nodes[node]
            .port_constraints
            .is_order_fixed()
        {
            return;
        }

        for port_side in [side, PortSide::South, PortSide::North] {
            let ports = graph.ports_on_side(node, port_side);
            self.distribute_ports_on_side(graph, node, &ports, order);
        }
        self.sort_ports(graph, node);
    }

    fn distribute_ports_on_side(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        ports: &[PortRef],
        order: &[Vec<NodeId>],
    ) {
        let mut in_layer_ports = Vec::new();
        self.min_barycenter = 0.0;
        self.max_barycenter = 0.0;
        let absurdly_large = 2.0 * layer_node_count(graph, node, order) as f64 + 1.0;

        'port_loop: for port in ports.iter().copied() {
            let Some(port_data) = graph
                .layerless_nodes
                .get(port.node)
                .and_then(|node| node.ports.get(port.port))
            else {
                continue;
            };

            let north_south_port = matches!(port_data.side, PortSide::North | PortSide::South);
            let mut sum = 0.0;
            if north_south_port {
                let Some(dummy_node) = port_data
                    .port_dummy
                    .as_ref()
                    .filter(|dummy| dummy.graph_id == graph.id)
                    .map(|dummy| dummy.node)
                else {
                    continue;
                };
                sum += north_south_port_barycenter_sum(
                    graph,
                    &self.node_positions,
                    port,
                    dummy_node,
                    absurdly_large,
                )
                .unwrap_or(0.0);
            } else {
                for outgoing_edge in port_data.outgoing_edges() {
                    let connected_port = graph.edges[*outgoing_edge].target();
                    if same_layer(graph, connected_port.node, node) {
                        in_layer_ports.push(port);
                        continue 'port_loop;
                    }
                    sum += self.rank_of(graph, connected_port);
                }
                for incoming_edge in port_data.incoming_edges() {
                    let connected_port = graph.edges[*incoming_edge].source();
                    if same_layer(graph, connected_port.node, node) {
                        in_layer_ports.push(port);
                        continue 'port_loop;
                    }
                    sum -= self.rank_of(graph, connected_port);
                }
            }

            let degree = port_degree(graph, port);
            if degree > 0 {
                let barycenter = sum / degree as f64;
                self.set_port_barycenter(graph, port, barycenter);
                self.min_barycenter = self.min_barycenter.min(barycenter);
                self.max_barycenter = self.max_barycenter.max(barycenter);
            } else if north_south_port {
                self.set_port_barycenter(graph, port, sum);
            }
        }

        if !in_layer_ports.is_empty() {
            self.calculate_in_layer_ports_barycenter_values(graph, node, &in_layer_ports, order);
        }
    }

    fn calculate_in_layer_ports_barycenter_values(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        in_layer_ports: &[PortRef],
        order: &[Vec<NodeId>],
    ) {
        let Some(node_position) = self.node_positions.get(&node).copied() else {
            return;
        };
        let node_index_in_layer = node_position + 1;
        let layer_size = layer_node_count(graph, node, order) + 1;

        for in_layer_port in in_layer_ports {
            let mut sum = 0usize;
            let mut in_layer_connections = 0usize;

            for connected_port in connected_ports(graph, *in_layer_port) {
                if same_layer(graph, connected_port.node, node)
                    && let Some(position) = self.node_positions.get(&connected_port.node)
                {
                    sum += position + 1;
                    in_layer_connections += 1;
                }
            }

            if in_layer_connections == 0 {
                continue;
            }

            let barycenter = sum as f64 / in_layer_connections as f64;
            let port_side =
                graph.layerless_nodes[in_layer_port.node].ports[in_layer_port.port].side;
            let port_barycenter = match port_side {
                PortSide::East if barycenter < node_index_in_layer as f64 => {
                    self.min_barycenter - barycenter
                }
                PortSide::East => self.max_barycenter + (layer_size as f64 - barycenter),
                PortSide::West if barycenter < node_index_in_layer as f64 => {
                    self.max_barycenter + barycenter
                }
                PortSide::West => self.min_barycenter - (layer_size as f64 - barycenter),
                _ => continue,
            };
            self.set_port_barycenter(graph, *in_layer_port, port_barycenter);
        }
    }

    fn sort_ports(&self, graph: &mut LGraph, node: NodeId) {
        let mut order = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
        order.sort_by(|left, right| {
            let left_port = &graph.layerless_nodes[node].ports[*left];
            let right_port = &graph.layerless_nodes[node].ports[*right];

            let side_order = port_side_order(left_port.side).cmp(&port_side_order(right_port.side));
            if side_order != std::cmp::Ordering::Equal {
                return side_order;
            }

            let left_barycenter = self
                .port_barycenters
                .get(&left_port.crossing_minimization_id.unwrap_or(usize::MAX))
                .copied()
                .unwrap_or(0.0);
            let right_barycenter = self
                .port_barycenters
                .get(&right_port.crossing_minimization_id.unwrap_or(usize::MAX))
                .copied()
                .unwrap_or(0.0);

            if left_barycenter == 0.0 && right_barycenter == 0.0 {
                std::cmp::Ordering::Equal
            } else if left_barycenter == 0.0 {
                std::cmp::Ordering::Less
            } else if right_barycenter == 0.0 {
                std::cmp::Ordering::Greater
            } else {
                left_barycenter.total_cmp(&right_barycenter)
            }
        });
        // A sort that moves nothing is the common case, so skip the write.
        if order
            .iter()
            .copied()
            .eq(graph.layerless_nodes[node].ports.ids())
        {
            return;
        }
        graph.reorder_node_ports(node, order);
    }

    fn set_port_barycenter(&mut self, graph: &LGraph, port: PortRef, barycenter: f64) {
        if let Some(id) = crossing_minimization_port_id(graph, port) {
            self.port_barycenters.insert(id, barycenter);
        }
    }

    fn update_node_positions(&mut self, order: &[Vec<NodeId>], current_index: usize) {
        if let Some(layer) = order.get(current_index) {
            for (position, node) in layer.iter().copied().enumerate() {
                self.node_positions.insert(node, position);
            }
        }
    }

    fn calculate_node_relative_port_ranks(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        rank_sum: f64,
        port_type: PortType,
    ) -> f64 {
        match port_type {
            PortType::Input => {
                let input_count = actual_ports_of_type(graph, node, PortType::Input).len();
                let north_input_count = actual_ports_of_type(graph, node, PortType::Input)
                    .into_iter()
                    .filter(|port| {
                        graph.layerless_nodes[port.node].ports[port.port]
                            .side
                            .is_north()
                    })
                    .count();

                let increment = 1.0 / (input_count + 1) as f64;
                let mut north_position = rank_sum + north_input_count as f64 * increment;
                let mut rest_position = rank_sum + 1.0 - increment;
                for port in actual_ports_of_type(graph, node, PortType::Input) {
                    if graph.layerless_nodes[port.node].ports[port.port]
                        .side
                        .is_north()
                    {
                        if let Some(id) = crossing_minimization_port_id(graph, port) {
                            self.port_ranks.insert(id, north_position);
                        }
                        north_position -= increment;
                    } else {
                        if let Some(id) = crossing_minimization_port_id(graph, port) {
                            self.port_ranks.insert(id, rest_position);
                        }
                        rest_position -= increment;
                    }
                }
                1.0
            }
            PortType::Output => {
                let output_count = actual_ports_of_type(graph, node, PortType::Output).len();
                let increment = 1.0 / (output_count + 1) as f64;
                let mut position = rank_sum + increment;
                for port in actual_ports_of_type(graph, node, PortType::Output) {
                    if let Some(id) = crossing_minimization_port_id(graph, port) {
                        self.port_ranks.insert(id, position);
                    }
                    position += increment;
                }
                1.0
            }
        }
    }

    fn calculate_layer_total_port_ranks(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        rank_sum: f64,
        port_type: PortType,
    ) -> f64 {
        match port_type {
            PortType::Input => {
                let input_ports = actual_ports_of_type(graph, node, PortType::Input);
                let input_count = input_ports.len();
                let north_input_count = input_ports
                    .iter()
                    .filter(|port| {
                        graph.layerless_nodes[port.node].ports[port.port]
                            .side
                            .is_north()
                    })
                    .count();

                let mut north_position = rank_sum + north_input_count as f64;
                let mut rest_position = rank_sum + input_count as f64;
                for port in input_ports {
                    if graph.layerless_nodes[port.node].ports[port.port]
                        .side
                        .is_north()
                    {
                        if let Some(id) = crossing_minimization_port_id(graph, port) {
                            self.port_ranks.insert(id, north_position);
                        }
                        north_position -= 1.0;
                    } else {
                        if let Some(id) = crossing_minimization_port_id(graph, port) {
                            self.port_ranks.insert(id, rest_position);
                        }
                        rest_position -= 1.0;
                    }
                }
                input_count as f64
            }
            PortType::Output => {
                let mut position = 0.0;
                for port in actual_ports_of_type(graph, node, PortType::Output) {
                    position += 1.0;
                    if let Some(id) = crossing_minimization_port_id(graph, port) {
                        self.port_ranks.insert(id, rank_sum + position);
                    }
                }
                position
            }
        }
    }
}

impl HierarchySweep {
    fn new(graphs: &mut GraphArena, cross_min_type: CrossMinType) -> Self {
        let mut random = graphs.root().random.clone();
        let random_seed = random.next_long();
        graphs.root_mut().random = random.clone();

        let mut sweep = Self {
            infos: Vec::new(),
            index_by_graph: HashMap::new(),
            changed: HashSet::new(),
            random,
            random_seed,
            cross_min_type,
        };
        sweep.initialize_graphs(graphs);
        sweep
    }

    fn initialize_graphs(&mut self, graphs: &GraphArena) {
        // Parents come before their children.
        let mut found = vec![GraphId::ROOT];
        let mut index = 0usize;
        while index < found.len() {
            let id = found[index];
            let graph = &graphs[id];
            let info_index = self.infos.len();
            self.index_by_graph.insert(id, info_index);
            let mut info = HierarchyGraphInfo::new(graph, self.cross_min_type);
            if info.parent.is_none() {
                self.random = info.random.clone();
            }
            let children = graph
                .layers()
                .iter()
                .flat_map(|layer| layer.nodes().iter().copied())
                .filter_map(|node_index| graph.layerless_nodes[node_index].nested_graph)
                .collect::<Vec<_>>();
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

    fn sweep_reducing_crossings_barycenter(
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
    fn use_node_port_order_crossing_counter(&self, graphs: &GraphArena, info_index: usize) -> bool {
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

    fn set_currently_best_node_orders(&mut self, graphs: &GraphArena) {
        let changed = self.changed.iter().copied().collect::<Vec<_>>();
        for index in changed {
            let graph = &graphs[self.infos[index].graph];
            self.infos[index].set_currently_best_node_and_port_order(graph);
        }
    }

    fn save_all_node_orders_of_changed_graphs(&mut self, _graphs: &GraphArena) {
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

#[derive(Debug, Default)]
struct LayerSweepTypeDecider {
    node_info: HashMap<NodeId, NodeInfluence>,
}

#[derive(Debug, Clone, Copy, Default)]
struct NodeInfluence {
    connected_edges: usize,
    hierarchical_influence: usize,
    random_influence: usize,
}

impl NodeInfluence {
    fn transfer(&mut self, source: NodeInfluence) {
        self.hierarchical_influence += source.hierarchical_influence;
        self.random_influence += source.random_influence;
        self.connected_edges += source.connected_edges;
    }
}

impl LayerSweepTypeDecider {
    fn decide(&mut self, graph: &LGraph, parent_graph: Option<&LGraph>, boundary: f64) -> bool {
        let mut paths_to_random = 0usize;
        let mut paths_to_hierarchical = 0usize;
        let mut north_south_dummies = Vec::new();

        for layer in graph.layers() {
            for node in layer.nodes() {
                if is_north_south_node(graph, *node) {
                    north_south_dummies.push(*node);
                    continue;
                }

                let mut current = self.node_info_for(*node);
                if graph.layerless_nodes[*node].kind() == LNodeKind::ExternalPort {
                    current.hierarchical_influence = 1;
                    if external_dummy_origin_side(graph, parent_graph, *node) == PortSide::East {
                        paths_to_hierarchical += current.connected_edges;
                    }
                } else if !has_connected_ports_on_side(graph, *node, PortSide::West) {
                    current.random_influence = 1;
                } else if !has_connected_ports_on_side(graph, *node, PortSide::East) {
                    paths_to_random += current.connected_edges;
                }
                self.node_info.insert(*node, current);

                for edge in graph.node_outgoing_edges(*node) {
                    paths_to_random += current.random_influence;
                    paths_to_hierarchical += current.hierarchical_influence;
                    let target = graph.edges[edge].target().node;
                    self.transfer_info_to(current, target);
                }

                for port in ports_in_north_south_east_west_order(graph, *node, PortSide::North)
                    .into_iter()
                    .chain(ports_in_north_south_east_west_order(
                        graph,
                        *node,
                        PortSide::South,
                    ))
                {
                    if let Some(dummy) = graph.layerless_nodes[port.node].ports[port.port]
                        .port_dummy
                        .as_ref()
                        && dummy.graph_id == graph.id
                    {
                        paths_to_random += current.random_influence;
                        paths_to_hierarchical += current.hierarchical_influence;
                        self.transfer_info_to(current, dummy.node);
                    }
                }
            }

            for node in north_south_dummies.drain(..) {
                let current = self.node_info_for(node);
                for edge in graph.node_outgoing_edges(node) {
                    paths_to_random += current.random_influence;
                    paths_to_hierarchical += current.hierarchical_influence;
                    let target = graph.edges[edge].target().node;
                    self.transfer_info_to(current, target);
                }
            }
        }

        let all_paths = paths_to_random + paths_to_hierarchical;
        let normalized = if all_paths == 0 {
            f64::INFINITY
        } else {
            (paths_to_random as f64 - paths_to_hierarchical as f64) / all_paths as f64
        };
        normalized >= boundary
    }

    fn node_info_for(&mut self, node: NodeId) -> NodeInfluence {
        *self.node_info.entry(node).or_default()
    }

    fn transfer_info_to(&mut self, current: NodeInfluence, target: NodeId) {
        let target_info = self.node_info.entry(target).or_default();
        target_info.transfer(current);
        target_info.connected_edges += 1;
    }
}

#[derive(Debug, Clone)]
struct BarycenterHeuristic {
    states: HashMap<NodeId, BarycenterState>,
    random: JavaRandom,
    port_distributor: BarycenterPortDistributor,
    force_node_model_order: bool,
    /// `ForsterConstraintResolver.layoutUnits`: the nodes of each layout unit, in the order the
    /// layers had when the resolver was made. The order of the layers changes later.
    layout_units: HashMap<NodeId, Vec<NodeId>>,
    bigger_than: HashMap<NodeId, HashSet<NodeId>>,
    smaller_than: HashMap<NodeId, HashSet<NodeId>>,
}

impl BarycenterHeuristic {
    const RANDOM_AMOUNT: f64 = 0.07;

    fn new(
        graph: &LGraph,
        random: JavaRandom,
        port_distributor: BarycenterPortDistributor,
    ) -> Self {
        let states = graph
            .layers()
            .iter()
            .flat_map(|layer| layer.nodes().iter().copied())
            .map(|node| (node, BarycenterState::default()))
            .collect();
        let mut layout_units: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for node in graph
            .layers()
            .iter()
            .flat_map(|layer| layer.nodes().iter().copied())
        {
            if let Some(unit) = graph.in_layer_layout_unit(node) {
                layout_units.entry(unit).or_default().push(node);
            }
        }
        Self {
            states,
            random,
            port_distributor,
            force_node_model_order: graph.options.force_node_model_order,
            layout_units,
            bigger_than: HashMap::new(),
            smaller_than: HashMap::new(),
        }
    }

    fn set_first_layer_order(&mut self, graph: &LGraph, order: &mut [Vec<NodeId>], forward: bool) {
        let Some(layer) = order.get_mut(first_index(forward, order.len())) else {
            return;
        };
        self.minimize_layer(graph, layer, false, true, forward);
    }

    fn minimize_crossings(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        free_layer_index: usize,
        forward: bool,
        is_first_sweep: bool,
    ) {
        if !is_first_layer(order, free_layer_index, forward) {
            let fixed_layer_index = if forward {
                free_layer_index - 1
            } else {
                free_layer_index + 1
            };
            let port_type = if forward {
                PortType::Output
            } else {
                PortType::Input
            };
            self.port_distributor
                .calculate_port_ranks(graph, &order[fixed_layer_index], port_type);
        }

        let pre_ordered = !is_first_sweep
            || order[free_layer_index]
                .first()
                .is_some_and(|node| graph.layerless_nodes[*node].kind() == LNodeKind::ExternalPort);
        let mut nodes = order[free_layer_index].clone();
        self.minimize_layer(graph, &mut nodes, pre_ordered, false, forward);
        order[free_layer_index] = nodes;
    }

    fn minimize_layer(
        &mut self,
        graph: &LGraph,
        layer: &mut [NodeId],
        pre_ordered: bool,
        randomize: bool,
        forward: bool,
    ) {
        if randomize {
            self.randomize_barycenters(layer);
        } else {
            self.calculate_barycenters(graph, layer, forward);
            self.fill_in_unknown_barycenters(layer, pre_ordered);
        }

        if layer.len() > 1 {
            if self.force_node_model_order {
                self.insertion_sort_by_model_order(graph, layer);
            } else {
                layer.sort_by(|left, right| {
                    let left = self.states.get(left).and_then(|state| state.barycenter);
                    let right = self.states.get(right).and_then(|state| state.barycenter);
                    compare_barycenters(left, right)
                });
                resolve_forster_constraints(graph, layer, &mut self.states, &self.layout_units);
            }
        }
    }

    fn insertion_sort_by_model_order(&mut self, graph: &LGraph, layer: &mut [NodeId]) {
        for index in 1..layer.len() {
            let node = layer[index];
            let mut insert_at = index;
            while insert_at > 0
                && self.compare_model_order_nodes(graph, layer[insert_at - 1], node)
                    == std::cmp::Ordering::Greater
            {
                layer[insert_at] = layer[insert_at - 1];
                insert_at -= 1;
            }
            layer[insert_at] = node;
        }
        self.bigger_than.clear();
        self.smaller_than.clear();
    }

    fn compare_model_order_nodes(
        &mut self,
        graph: &LGraph,
        first: NodeId,
        second: NodeId,
    ) -> std::cmp::Ordering {
        if let Some(ordering) = self.transitive_ordering(first, second) {
            return ordering;
        }

        if let (Some(first_order), Some(second_order)) = (
            graph.layerless_nodes[first].model_order,
            graph.layerless_nodes[second].model_order,
        ) {
            let ordering = first_order.cmp(&second_order);
            self.update_model_order_associations(first, second, ordering);
            return ordering;
        }

        let first_barycenter = self.states.get(&first).and_then(|state| state.barycenter);
        let second_barycenter = self.states.get(&second).and_then(|state| state.barycenter);
        let ordering = compare_barycenters(first_barycenter, second_barycenter);
        self.update_model_order_associations(first, second, ordering);
        ordering
    }

    fn transitive_ordering(&mut self, first: NodeId, second: NodeId) -> Option<std::cmp::Ordering> {
        self.bigger_than.entry(first).or_default();
        self.bigger_than.entry(second).or_default();
        self.smaller_than.entry(first).or_default();
        self.smaller_than.entry(second).or_default();

        if self.bigger_than[&first].contains(&second) || self.smaller_than[&second].contains(&first)
        {
            Some(std::cmp::Ordering::Greater)
        } else if self.bigger_than[&second].contains(&first)
            || self.smaller_than[&first].contains(&second)
        {
            Some(std::cmp::Ordering::Less)
        } else {
            None
        }
    }

    fn update_model_order_associations(
        &mut self,
        first: NodeId,
        second: NodeId,
        ordering: std::cmp::Ordering,
    ) {
        match ordering {
            std::cmp::Ordering::Less => update_bigger_and_smaller_associations(
                first,
                second,
                &mut self.bigger_than,
                &mut self.smaller_than,
            ),
            std::cmp::Ordering::Greater => update_bigger_and_smaller_associations(
                second,
                first,
                &mut self.bigger_than,
                &mut self.smaller_than,
            ),
            std::cmp::Ordering::Equal => {}
        }
    }

    fn randomize_barycenters(&mut self, nodes: &[NodeId]) {
        for node in nodes {
            let value = self.random.next_double();
            let state = self.state_mut(*node);
            state.barycenter = Some(value);
            state.summed_weight = value;
            state.degree = 1;
            state.visited = false;
        }
    }

    fn calculate_barycenters(&mut self, graph: &LGraph, nodes: &[NodeId], forward: bool) {
        for node in nodes {
            self.state_mut(*node).visited = false;
        }

        for node in nodes {
            self.calculate_barycenter(graph, *node, forward);
        }
    }

    fn calculate_barycenter(&mut self, graph: &LGraph, node: NodeId, forward: bool) {
        if self.state_mut(node).visited {
            return;
        }
        {
            let state = self.state_mut(node);
            state.visited = true;
            state.degree = 0;
            state.summed_weight = 0.0;
            state.barycenter = None;
        }

        for port in graph.layerless_nodes[node].ports.ids() {
            let free_port = PortRef { node, port };
            for fixed_port in connected_ports_for_sweep(graph, free_port, forward) {
                if graph.layerless_nodes[fixed_port.node].layer_index()
                    == graph.layerless_nodes[node].layer_index()
                {
                    if fixed_port.node != node {
                        self.calculate_barycenter(graph, fixed_port.node, forward);
                        let fixed_state = self.state(fixed_port.node).clone();
                        let state = self.state_mut(node);
                        state.degree += fixed_state.degree;
                        state.summed_weight += fixed_state.summed_weight;
                    }
                } else {
                    let rank = self.port_distributor.rank_of(graph, fixed_port);
                    let state = self.state_mut(node);
                    state.summed_weight += rank;
                    state.degree += 1;
                }
            }
        }

        for associate in graph.barycenter_associates(node).to_vec() {
            if graph.layerless_nodes[associate].layer_index()
                == graph.layerless_nodes[node].layer_index()
            {
                self.calculate_barycenter(graph, associate, forward);
                let associate_state = self.state(associate).clone();
                let state = self.state_mut(node);
                state.degree += associate_state.degree;
                state.summed_weight += associate_state.summed_weight;
            }
        }

        let degree = self.state(node).degree;
        if degree > 0 {
            let perturbation =
                self.random.next_float() as f64 * Self::RANDOM_AMOUNT - Self::RANDOM_AMOUNT / 2.0;
            let state = self.state_mut(node);
            state.summed_weight += perturbation;
            state.barycenter = Some(state.summed_weight / degree as f64);
        }
    }

    fn fill_in_unknown_barycenters(&mut self, nodes: &[NodeId], pre_ordered: bool) {
        if pre_ordered {
            let mut last_value = -1.0;
            for (index, node) in nodes.iter().copied().enumerate() {
                let mut value = self.state(node).barycenter;
                if value.is_none() {
                    let next_value = nodes
                        .iter()
                        .skip(index + 1)
                        .find_map(|next| self.state(*next).barycenter)
                        .unwrap_or(last_value + 1.0);
                    value = Some((last_value + next_value) / 2.0);
                    let state = self.state_mut(node);
                    state.barycenter = value;
                    state.summed_weight = value.unwrap_or(0.0);
                    state.degree = 1;
                }
                last_value = value.unwrap_or(last_value);
            }
        } else {
            let max_barycenter = nodes
                .iter()
                .filter_map(|node| self.state(*node).barycenter)
                .fold(0.0, f64::max)
                + 2.0;
            for node in nodes {
                if self.state(*node).barycenter.is_none() {
                    let value = self.random.next_float() as f64 * max_barycenter - 1.0;
                    let state = self.state_mut(*node);
                    state.barycenter = Some(value);
                    state.summed_weight = value;
                    state.degree = 1;
                }
            }
        }
    }

    fn state(&self, node: NodeId) -> &BarycenterState {
        self.states
            .get(&node)
            .expect("barycenter state initialized for every layered node")
    }

    fn state_mut(&mut self, node: NodeId) -> &mut BarycenterState {
        self.states.entry(node).or_default()
    }
}

fn compare_barycenters(first: Option<f64>, second: Option<f64>) -> std::cmp::Ordering {
    match (first, second) {
        (Some(first), Some(second)) => first.total_cmp(&second),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ConstraintGroup {
    nodes: Vec<NodeId>,
    outgoing: Vec<usize>,
    incoming: Vec<usize>,
    incoming_count: usize,
    summed_weight: f64,
    degree: usize,
}

impl ConstraintGroup {
    fn new(node: NodeId) -> Self {
        Self {
            nodes: vec![node],
            outgoing: Vec::new(),
            incoming: Vec::new(),
            incoming_count: 0,
            summed_weight: 0.0,
            degree: 0,
        }
    }

    fn barycenter(&self, states: &HashMap<NodeId, BarycenterState>) -> Option<f64> {
        self.nodes
            .first()
            .and_then(|node| states.get(node))
            .and_then(|state| state.barycenter)
    }

    fn set_barycenter(&self, states: &mut HashMap<NodeId, BarycenterState>, value: f64) {
        for node in &self.nodes {
            states.entry(*node).or_default().barycenter = Some(value);
        }
    }

    fn reset_edges(&mut self) {
        self.outgoing.clear();
        self.incoming.clear();
        self.incoming_count = 0;
    }

    fn has_outgoing(&self) -> bool {
        !self.outgoing.is_empty()
    }

    fn has_incoming(&self) -> bool {
        !self.incoming.is_empty()
    }
}

fn resolve_forster_constraints(
    graph: &LGraph,
    layer: &mut [NodeId],
    states: &mut HashMap<NodeId, BarycenterState>,
    layout_units: &HashMap<NodeId, Vec<NodeId>>,
) {
    if layer.len() < 2 {
        return;
    }

    let mut groups = layer
        .iter()
        .copied()
        .map(ConstraintGroup::new)
        .collect::<Vec<_>>();
    if graph.in_layer_successor_constraints_between_non_dummies {
        process_forster_constraints(graph, states, layout_units, &mut groups, true);
        apply_forster_group_order(layer, states, &groups);
        groups = layer
            .iter()
            .copied()
            .map(ConstraintGroup::new)
            .collect::<Vec<_>>();
    }
    process_forster_constraints(graph, states, layout_units, &mut groups, false);

    apply_forster_group_order(layer, states, &groups);
}

fn apply_forster_group_order(
    layer: &mut [NodeId],
    states: &mut HashMap<NodeId, BarycenterState>,
    groups: &[ConstraintGroup],
) {
    let mut index = 0usize;
    for group in groups {
        for node in &group.nodes {
            if index < layer.len() {
                layer[index] = *node;
                index += 1;
            }
            if let Some(barycenter) = group.barycenter(states) {
                states.entry(*node).or_default().barycenter = Some(barycenter);
            }
        }
    }
}

fn process_forster_constraints(
    graph: &LGraph,
    states: &mut HashMap<NodeId, BarycenterState>,
    layout_units: &HashMap<NodeId, Vec<NodeId>>,
    groups: &mut Vec<ConstraintGroup>,
    only_between_normal_nodes: bool,
) {
    build_forster_constraints_graph(graph, layout_units, groups, only_between_normal_nodes);
    while let Some((predecessor, successor)) = find_violated_forster_constraint(states, groups) {
        handle_violated_forster_constraint(states, groups, predecessor, successor);
    }
}

fn build_forster_constraints_graph(
    graph: &LGraph,
    layout_units: &HashMap<NodeId, Vec<NodeId>>,
    groups: &mut [ConstraintGroup],
    only_between_normal_nodes: bool,
) {
    for group in groups.iter_mut() {
        group.reset_edges();
    }

    let index_by_node = groups
        .iter()
        .enumerate()
        .flat_map(|(index, group)| group.nodes.iter().map(move |node| (*node, index)))
        .collect::<HashMap<_, _>>();

    let mut last_non_dummy_node: Option<NodeId> = None;
    let mut edges = Vec::<(usize, usize)>::new();
    for (group_index, group) in groups.iter().enumerate() {
        let Some(node) = group.nodes.first().copied() else {
            continue;
        };

        if only_between_normal_nodes && graph.layerless_nodes[node].kind() != LNodeKind::Normal {
            continue;
        }

        for successor in graph.in_layer_successor_constraints(node) {
            if only_between_normal_nodes
                && graph.layerless_nodes[*successor].kind() != LNodeKind::Normal
            {
                continue;
            }
            if let Some(successor_group) = index_by_node.get(successor).copied() {
                edges.push((group_index, successor_group));
            }
        }

        if !only_between_normal_nodes && graph.layerless_nodes[node].kind() == LNodeKind::Normal {
            if let Some(previous) = last_non_dummy_node {
                let previous_unit_nodes = layout_unit_nodes(layout_units, previous);
                let current_unit_nodes = layout_unit_nodes(layout_units, node);
                for previous_unit in previous_unit_nodes {
                    for current_unit in current_unit_nodes {
                        if let (Some(left), Some(right)) = (
                            index_by_node.get(previous_unit).copied(),
                            index_by_node.get(current_unit).copied(),
                        ) {
                            edges.push((left, right));
                        }
                    }
                }
            }
            last_non_dummy_node = Some(node);
        }
    }

    for (from, to) in edges {
        add_forster_constraint(groups, from, to);
    }
}

/// `ForsterConstraintResolver.layoutUnits.get(unit)`.
fn layout_unit_nodes(layout_units: &HashMap<NodeId, Vec<NodeId>>, unit: NodeId) -> &[NodeId] {
    layout_units.get(&unit).map_or(&[], Vec::as_slice)
}

fn add_forster_constraint(groups: &mut [ConstraintGroup], from: usize, to: usize) {
    if from == to {
        return;
    }
    if !groups[from].outgoing.contains(&to) {
        groups[from].outgoing.push(to);
        groups[to].incoming_count += 1;
    }
}

fn find_violated_forster_constraint(
    states: &HashMap<NodeId, BarycenterState>,
    groups: &mut [ConstraintGroup],
) -> Option<(usize, usize)> {
    let mut active_groups = VecDeque::new();
    for (index, group) in groups.iter_mut().enumerate() {
        group.incoming.clear();
        if group.has_outgoing() && group.incoming_count == 0 {
            active_groups.push_back(index);
        }
    }

    while let Some(group_index) = active_groups.pop_front() {
        if groups[group_index].has_incoming() {
            for predecessor in groups[group_index].incoming.clone() {
                let predecessor_barycenter = groups[predecessor].barycenter(states).unwrap_or(0.0);
                let group_barycenter = groups[group_index].barycenter(states).unwrap_or(0.0);
                if predecessor_barycenter as f32 == group_barycenter as f32 {
                    if predecessor > group_index {
                        return Some((predecessor, group_index));
                    }
                } else if predecessor_barycenter > group_barycenter {
                    return Some((predecessor, group_index));
                }
            }
        }

        let outgoing = groups[group_index].outgoing.clone();
        for successor in outgoing {
            groups[successor].incoming.insert(0, group_index);
            if groups[successor].incoming_count == groups[successor].incoming.len() {
                active_groups.push_back(successor);
            }
        }
    }

    None
}

fn handle_violated_forster_constraint(
    states: &mut HashMap<NodeId, BarycenterState>,
    groups: &mut Vec<ConstraintGroup>,
    first_group: usize,
    second_group: usize,
) {
    let merged = merge_constraint_groups(states, &groups[first_group], &groups[second_group]);
    let merged_barycenter = merged.barycenter(states);
    let mut next = Vec::<ConstraintGroup>::with_capacity(groups.len().saturating_sub(1));
    let mut old_to_new = vec![usize::MAX; groups.len()];
    let mut merged_index = None;
    let mut inserted = false;

    for (index, group) in groups.iter().enumerate() {
        if index == first_group || index == second_group {
            continue;
        }
        if !inserted
            && let (Some(group_barycenter), Some(merged_barycenter)) =
                (group.barycenter(states), merged_barycenter)
            && group_barycenter > merged_barycenter
        {
            merged_index = Some(next.len());
            next.push(merged.clone());
            inserted = true;
        }
        old_to_new[index] = next.len();
        next.push(group.clone());
    }

    if !inserted {
        merged_index = Some(next.len());
        next.push(merged);
    }
    let merged_index = merged_index.expect("merged group should be inserted");
    old_to_new[first_group] = merged_index;
    old_to_new[second_group] = merged_index;
    remap_forster_constraints(&mut next, &old_to_new);

    *groups = next;
}

fn merge_constraint_groups(
    states: &mut HashMap<NodeId, BarycenterState>,
    first: &ConstraintGroup,
    second: &ConstraintGroup,
) -> ConstraintGroup {
    let mut nodes = first.nodes.clone();
    nodes.extend(second.nodes.iter().copied());

    let merged = ConstraintGroup {
        nodes,
        outgoing: merged_outgoing_constraints(first, second),
        incoming: Vec::new(),
        incoming_count: 0,
        summed_weight: first.summed_weight + second.summed_weight,
        degree: first.degree + second.degree,
    };

    let barycenter = if merged.degree > 0 {
        Some(merged.summed_weight / merged.degree as f64)
    } else {
        match (first.barycenter(states), second.barycenter(states)) {
            (Some(first), Some(second)) => Some((first + second) / 2.0),
            (Some(first), None) => Some(first),
            (None, Some(second)) => Some(second),
            (None, None) => None,
        }
    };
    if let Some(barycenter) = barycenter {
        merged.set_barycenter(states, barycenter);
    }

    merged
}

fn merged_outgoing_constraints(first: &ConstraintGroup, second: &ConstraintGroup) -> Vec<usize> {
    first
        .outgoing
        .iter()
        .chain(second.outgoing.iter())
        .copied()
        .collect::<Vec<_>>()
}

fn remap_forster_constraints(groups: &mut [ConstraintGroup], old_to_new: &[usize]) {
    for group in groups.iter_mut() {
        group.outgoing = group
            .outgoing
            .iter()
            .filter_map(|old_index| old_to_new.get(*old_index).copied())
            .filter(|new_index| *new_index != usize::MAX)
            .collect::<Vec<_>>();
    }

    for (group_index, group) in groups.iter_mut().enumerate() {
        group.outgoing.retain(|successor| *successor != group_index);
        let mut seen = HashSet::new();
        group.outgoing.retain(|successor| seen.insert(*successor));
        group.incoming.clear();
        group.incoming_count = 0;
    }

    let edges = groups
        .iter()
        .enumerate()
        .flat_map(|(from, group)| group.outgoing.iter().copied().map(move |to| (from, to)))
        .collect::<Vec<_>>();
    for (_, to) in edges {
        groups[to].incoming_count += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrossingCountSide {
    West,
    East,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Adjacency {
    position: usize,
    cardinality: usize,
    current_cardinality: usize,
}

impl Adjacency {
    fn new(position: usize) -> Self {
        Self {
            position,
            cardinality: 1,
            current_cardinality: 1,
        }
    }

    fn reset(&mut self) {
        self.current_cardinality = self.cardinality;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AdjacencyList {
    adjacency_list: Vec<Adjacency>,
    size: usize,
    current_size: usize,
    current_index: usize,
}

impl AdjacencyList {
    fn new<L: AsRef<[NodeId]>>(
        graph: &LGraph,
        current_node_order: &[L],
        node: NodeId,
        side: PortSide,
    ) -> Self {
        let mut adjacency_list = Vec::new();
        let mut size = 0usize;
        let port_positions =
            neighboring_layer_port_positions(graph, current_node_order, node, side);

        for port in ports_in_north_south_east_west_order(graph, node, side) {
            for edge in edges_connected_to_side(graph, port, side) {
                if !is_edge_self_loop(graph, edge) && !is_in_layer_edge(graph, edge) {
                    let adjacent_port = adjacent_port_of_side(graph, edge, side);
                    if let Some(&adjacent_port_position) = port_positions.get(&adjacent_port) {
                        add_adjacency(&mut adjacency_list, adjacent_port_position);
                        size += 1;
                    }
                }
            }
        }

        // Collections.sort is stable, and so is sort_by_key. Two entries at one position that the
        // collection did not merge stay two entries, in the order they were collected.
        adjacency_list.sort_by_key(|adjacency| adjacency.position);
        Self {
            adjacency_list,
            size,
            current_size: size,
            current_index: 0,
        }
    }

    fn reset(&mut self) {
        self.current_index = 0;
        self.current_size = self.size;
        if !self.is_empty() {
            self.current_adjacency_mut().reset();
        }
    }

    fn count_adjacencies_below_node_of_first_port(&self) -> usize {
        self.current_size - self.current_adjacency().current_cardinality
    }

    fn remove_first(&mut self) {
        if self.is_empty() {
            return;
        }

        if self.current_adjacency().current_cardinality == 1 {
            self.increment_current_index();
        } else {
            self.current_adjacency_mut().current_cardinality -= 1;
        }

        self.current_size -= 1;
    }

    fn increment_current_index(&mut self) {
        self.current_index += 1;
        if self.current_index < self.adjacency_list.len() {
            self.current_adjacency_mut().reset();
        }
    }

    fn is_empty(&self) -> bool {
        self.current_size == 0
    }

    fn first(&self) -> usize {
        self.current_adjacency().position
    }

    fn current_size(&self) -> usize {
        self.current_size
    }

    fn current_adjacency(&self) -> &Adjacency {
        &self.adjacency_list[self.current_index]
    }

    fn current_adjacency_mut(&mut self) -> &mut Adjacency {
        &mut self.adjacency_list[self.current_index]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BetweenLayerEdgeTwoNodeCrossingsCounter {
    upper_lower_crossings: usize,
    lower_upper_crossings: usize,
    eastern_adjacencies: HashMap<NodeId, AdjacencyList>,
    western_adjacencies: HashMap<NodeId, AdjacencyList>,
}

impl BetweenLayerEdgeTwoNodeCrossingsCounter {
    fn new() -> Self {
        Self::default()
    }

    fn count_eastern_edge_crossings(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        free_layer_index: usize,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_crossings(
            graph,
            current_node_order,
            free_layer_index,
            upper_node,
            lower_node,
            PortSide::East,
        );
    }

    fn count_western_edge_crossings(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        free_layer_index: usize,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_crossings(
            graph,
            current_node_order,
            free_layer_index,
            upper_node,
            lower_node,
            PortSide::West,
        );
    }

    fn count_both_side_crossings<L: AsRef<[NodeId]>>(
        &mut self,
        graph: &LGraph,
        current_node_order: &[L],
        free_layer_index: usize,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_crossings(
            graph,
            current_node_order,
            free_layer_index,
            upper_node,
            lower_node,
            PortSide::West,
        );
        self.add_crossings(
            graph,
            current_node_order,
            free_layer_index,
            upper_node,
            lower_node,
            PortSide::East,
        );
    }

    fn reset_crossing_count(&mut self) {
        self.upper_lower_crossings = 0;
        self.lower_upper_crossings = 0;
    }

    fn add_crossings<L: AsRef<[NodeId]>>(
        &mut self,
        graph: &LGraph,
        current_node_order: &[L],
        free_layer_index: usize,
        upper_node: NodeId,
        lower_node: NodeId,
        side: PortSide,
    ) {
        if !neighboring_layer_exists(current_node_order, free_layer_index, side) {
            return;
        }

        let (upper, lower) = match side {
            PortSide::East => {
                if self.eastern_adjacencies.is_empty() {
                    self.eastern_adjacencies =
                        build_adjacencies(graph, current_node_order, free_layer_index, side);
                }
                (
                    self.eastern_adjacencies.get(&upper_node).cloned(),
                    self.eastern_adjacencies.get(&lower_node).cloned(),
                )
            }
            PortSide::West => {
                if self.western_adjacencies.is_empty() {
                    self.western_adjacencies =
                        build_adjacencies(graph, current_node_order, free_layer_index, side);
                }
                (
                    self.western_adjacencies.get(&upper_node).cloned(),
                    self.western_adjacencies.get(&lower_node).cloned(),
                )
            }
            _ => (None, None),
        };

        let (Some(mut upper), Some(mut lower)) = (upper, lower) else {
            return;
        };
        upper.reset();
        lower.reset();
        if upper.current_size() == 0 || lower.current_size() == 0 {
            return;
        }
        self.count_crossings_by_merging_adjacency_lists(upper, lower);
    }

    fn count_crossings_by_merging_adjacency_lists(
        &mut self,
        mut upper_adjacencies: AdjacencyList,
        mut lower_adjacencies: AdjacencyList,
    ) {
        while !upper_adjacencies.is_empty() && !lower_adjacencies.is_empty() {
            if is_below(upper_adjacencies.first(), lower_adjacencies.first()) {
                self.upper_lower_crossings += upper_adjacencies.current_size();
                lower_adjacencies.remove_first();
            } else if is_below(lower_adjacencies.first(), upper_adjacencies.first()) {
                self.lower_upper_crossings += lower_adjacencies.current_size();
                upper_adjacencies.remove_first();
            } else {
                self.upper_lower_crossings +=
                    upper_adjacencies.count_adjacencies_below_node_of_first_port();
                self.lower_upper_crossings +=
                    lower_adjacencies.count_adjacencies_below_node_of_first_port();
                upper_adjacencies.remove_first();
                lower_adjacencies.remove_first();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CrossingMatrixFiller {
    is_crossing_matrix_filled: Vec<Vec<bool>>,
    crossing_matrix: Vec<Vec<usize>>,
    in_between_layer_crossing_counter: BetweenLayerEdgeTwoNodeCrossingsCounter,
    direction: CrossingCountSide,
    one_sided: bool,
}

impl CrossingMatrixFiller {
    fn new(
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

    fn get_crossing_matrix_entry(
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct InLayerCrossingCounter {
    port_positions: HashMap<PortRef, usize>,
    node_cardinalities: HashMap<NodeId, usize>,
    side: PortSide,
}

impl InLayerCrossingCounter {
    fn new(graph: &LGraph, free_layer: &[NodeId], side: PortSide) -> Self {
        let mut port_positions = HashMap::new();
        let mut node_cardinalities = HashMap::new();
        let mut position = 0usize;
        for node in free_layer {
            let ports = ports_for_counter_side(graph, *node, side, true);
            node_cardinalities.insert(*node, ports.len());
            for port in ports {
                port_positions.insert(port, position);
                position += 1;
            }
        }
        Self {
            port_positions,
            node_cardinalities,
            side,
        }
    }

    fn count_in_layer_crossings_between_nodes_in_both_orders(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) -> (usize, usize) {
        let mut ports =
            self.connected_in_layer_ports_sorted_by_position(graph, upper_node, lower_node);
        let upper_lower_crossings = self.count_in_layer_crossings_on_ports(graph, &ports);
        self.switch_nodes(graph, upper_node, lower_node);
        ports.sort_by_key(|port| self.position_of(*port));
        let lower_upper_crossings = self.count_in_layer_crossings_on_ports(graph, &ports);
        self.switch_nodes(graph, lower_node, upper_node);
        (upper_lower_crossings, lower_upper_crossings)
    }

    fn switch_nodes(&mut self, graph: &LGraph, was_upper_node: NodeId, was_lower_node: NodeId) {
        let lower_cardinality = *self.node_cardinalities.get(&was_lower_node).unwrap_or(&0);
        for port in ports_in_north_south_east_west_order(graph, was_upper_node, self.side) {
            if let Some(position) = self.port_positions.get_mut(&port) {
                *position += lower_cardinality;
            }
        }

        let upper_cardinality = *self.node_cardinalities.get(&was_upper_node).unwrap_or(&0);
        for port in ports_in_north_south_east_west_order(graph, was_lower_node, self.side) {
            if let Some(position) = self.port_positions.get_mut(&port) {
                *position = position.saturating_sub(upper_cardinality);
            }
        }
    }

    fn connected_in_layer_ports_sorted_by_position(
        &self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) -> Vec<PortRef> {
        let mut ports = HashSet::new();
        for node in [upper_node, lower_node] {
            for port in ports_in_north_south_east_west_order(graph, node, self.side) {
                for edge in connected_edges(graph, port) {
                    if is_edge_self_loop(graph, edge) {
                        continue;
                    }
                    ports.insert(port);
                    if is_in_layer_edge(graph, edge)
                        && let Some(other) = other_end_of_edge(graph, edge, port)
                    {
                        ports.insert(other);
                    }
                }
            }
        }

        let mut ports = ports.into_iter().collect::<Vec<_>>();
        ports.sort_by_key(|port| self.position_of(*port));
        ports
    }

    fn count_in_layer_crossings_on_ports(&self, graph: &LGraph, ports: &[PortRef]) -> usize {
        let mut crossings = 0usize;
        let mut ends = Vec::new();
        let mut index_tree = BinaryIndexedTree::new(self.port_positions.len());

        for port in ports {
            let Some(port_position) = self.position_of_checked(*port) else {
                continue;
            };
            index_tree.remove_all(port_position);
            let mut num_between_layer_edges = 0usize;

            for edge in connected_edges(graph, *port) {
                if is_in_layer_edge(graph, edge) {
                    let Some(other_end) = other_end_of_edge(graph, edge, *port) else {
                        continue;
                    };
                    let Some(end_position) = self.position_of_checked(other_end) else {
                        continue;
                    };
                    if end_position > port_position {
                        crossings += index_tree.rank(end_position).unwrap_or(0);
                        ends.push(end_position);
                    }
                } else {
                    num_between_layer_edges += 1;
                }
            }

            crossings += index_tree.size() * num_between_layer_edges;
            while let Some(end) = ends.pop() {
                index_tree.add(end);
            }
        }

        crossings
    }

    fn position_of(&self, port: PortRef) -> usize {
        self.position_of_checked(port).unwrap_or(usize::MAX)
    }

    fn position_of_checked(&self, port: PortRef) -> Option<usize> {
        self.port_positions.get(&port).copied()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NorthSouthEdgeNeighbouringNodeCrossingsCounter {
    upper_lower_crossings: usize,
    lower_upper_crossings: usize,
    /// The place of each northern and southern port among the ports of its side and node, by
    /// `crossing_minimization_id` as in `CrossingsCounter`. A port nobody numbered is at 0.
    port_positions: Vec<usize>,
}

impl NorthSouthEdgeNeighbouringNodeCrossingsCounter {
    /// Ports the constructor: it numbers the northern and southern ports of the layer's nodes.
    fn new(graph: &LGraph, layer: &[NodeId]) -> Self {
        let mut port_positions = Vec::new();
        for &node in layer {
            for side in [PortSide::South, PortSide::North] {
                for (position, port) in ports_in_north_south_east_west_order(graph, node, side)
                    .into_iter()
                    .enumerate()
                {
                    let Some(id) =
                        graph.layerless_nodes[port.node].ports[port.port].crossing_minimization_id
                    else {
                        continue;
                    };
                    if port_positions.len() <= id {
                        port_positions.resize(id + 1, 0);
                    }
                    port_positions[id] = position;
                }
            }
        }
        Self {
            port_positions,
            ..Self::default()
        }
    }

    fn count_crossings(&mut self, graph: &LGraph, upper_node: NodeId, lower_node: NodeId) {
        self.upper_lower_crossings = 0;
        self.lower_upper_crossings = 0;

        self.process_if_two_north_south_nodes(graph, upper_node, lower_node);
        self.process_if_north_south_long_edge_dummy_crossing(graph, upper_node, lower_node);
        self.process_if_normal_node_with_ns_ports_and_long_edge_dummy(
            graph, upper_node, lower_node,
        );
    }

    /// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.processIfTwoNorthSouthNodes`.
    fn process_if_two_north_south_nodes(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        if is_north_south_node(graph, upper_node)
            && is_north_south_node(graph, lower_node)
            && !have_different_origins(graph, upper_node, lower_node)
        {
            if north_south_dummy_is_north_of_normal_node(graph, upper_node) {
                self.count_crossings_of_two_north_south_dummies(graph, upper_node, lower_node);
            } else {
                self.count_crossings_of_two_north_south_dummies(graph, lower_node, upper_node);
            }
        }
    }

    /// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.countCrossingsOfTwoNorthSouthDummies`.
    fn count_crossings_of_two_north_south_dummies(
        &mut self,
        graph: &LGraph,
        further_from_normal_node: NodeId,
        closer_to_normal_node: NodeId,
    ) {
        let first_degree = |node: NodeId, side: PortSide| {
            first_port_on_side(graph, node, side).map_or(0, |port| port_degree(graph, port))
        };
        if self.origin_port_position_of(graph, further_from_normal_node)
            > self.origin_port_position_of(graph, closer_to_normal_node)
        {
            self.upper_lower_crossings = first_degree(closer_to_normal_node, PortSide::East);
            self.lower_upper_crossings = first_degree(further_from_normal_node, PortSide::West);
        } else {
            self.upper_lower_crossings = first_degree(closer_to_normal_node, PortSide::West);
            self.lower_upper_crossings = first_degree(further_from_normal_node, PortSide::East);
        }
    }

    fn process_if_north_south_long_edge_dummy_crossing(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        if is_north_south_node(graph, upper_node) && is_long_edge_dummy(graph, lower_node) {
            if north_south_dummy_is_north_of_normal_node(graph, upper_node) {
                self.upper_lower_crossings = 1;
            } else {
                self.lower_upper_crossings = 1;
            }
        } else if is_north_south_node(graph, lower_node) && is_long_edge_dummy(graph, upper_node) {
            if north_south_dummy_is_north_of_normal_node(graph, lower_node) {
                self.lower_upper_crossings = 1;
            } else {
                self.upper_lower_crossings = 1;
            }
        }
    }

    fn process_if_normal_node_with_ns_ports_and_long_edge_dummy(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        if graph.layerless_nodes[upper_node].kind() == LNodeKind::Normal
            && is_long_edge_dummy(graph, lower_node)
        {
            self.upper_lower_crossings =
                number_of_north_south_edges(graph, upper_node, PortSide::South);
            self.lower_upper_crossings =
                number_of_north_south_edges(graph, upper_node, PortSide::North);
        }
        if graph.layerless_nodes[lower_node].kind() == LNodeKind::Normal
            && is_long_edge_dummy(graph, upper_node)
        {
            self.upper_lower_crossings =
                number_of_north_south_edges(graph, lower_node, PortSide::North);
            self.lower_upper_crossings =
                number_of_north_south_edges(graph, lower_node, PortSide::South);
        }
    }

    fn origin_port_position_of(&self, graph: &LGraph, node: NodeId) -> usize {
        // The origin port is a northern or southern port of a node in the layer, so it has a place.
        origin_port_of(graph, node)
            .and_then(|origin| {
                graph.layerless_nodes[origin.node].ports[origin.port].crossing_minimization_id
            })
            .and_then(|id| self.port_positions.get(id).copied())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SwitchDecider {
    free_layer_index: usize,
    one_sided: bool,
    crossing_matrix_filler: CrossingMatrixFiller,
    left_in_layer_counter: InLayerCrossingCounter,
    right_in_layer_counter: InLayerCrossingCounter,
    north_south_counter: NorthSouthEdgeNeighbouringNodeCrossingsCounter,
}

impl SwitchDecider {
    fn new(
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        free_layer_index: usize,
        crossing_matrix_filler: CrossingMatrixFiller,
        one_sided: bool,
    ) -> Self {
        let free_layer = &current_node_order[free_layer_index];
        Self {
            free_layer_index,
            one_sided,
            crossing_matrix_filler,
            left_in_layer_counter: InLayerCrossingCounter::new(graph, free_layer, PortSide::West),
            right_in_layer_counter: InLayerCrossingCounter::new(graph, free_layer, PortSide::East),
            north_south_counter: NorthSouthEdgeNeighbouringNodeCrossingsCounter::new(
                graph, free_layer,
            ),
        }
    }

    fn notify_of_switch(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.left_in_layer_counter
            .switch_nodes(graph, upper_node, lower_node);
        self.right_in_layer_counter
            .switch_nodes(graph, upper_node, lower_node);
        self.crossing_matrix_filler = CrossingMatrixFiller::new(
            current_node_order[self.free_layer_index].len(),
            if self.one_sided {
                CrossMinType::OneSidedGreedySwitch
            } else {
                CrossMinType::TwoSidedGreedySwitch
            },
            self.crossing_matrix_filler.direction,
        );
    }

    fn does_switch_reduce_crossings(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        upper_node_index: usize,
        lower_node_index: usize,
    ) -> bool {
        if self.constraints_prevent_switch(
            graph,
            &current_node_order[self.free_layer_index],
            upper_node_index,
            lower_node_index,
        ) {
            return false;
        }

        let free_layer = &current_node_order[self.free_layer_index];
        let upper_node = free_layer[upper_node_index];
        let lower_node = free_layer[lower_node_index];
        let layer_positions = layer_position_map(free_layer);

        let left_in_layer = self
            .left_in_layer_counter
            .count_in_layer_crossings_between_nodes_in_both_orders(graph, upper_node, lower_node);
        let right_in_layer = self
            .right_in_layer_counter
            .count_in_layer_crossings_between_nodes_in_both_orders(graph, upper_node, lower_node);
        self.north_south_counter
            .count_crossings(graph, upper_node, lower_node);

        let upper_lower_crossings = self.crossing_matrix_filler.get_crossing_matrix_entry(
            graph,
            current_node_order,
            self.free_layer_index,
            &layer_positions,
            upper_node,
            lower_node,
        ) + left_in_layer.0
            + right_in_layer.0
            + self.north_south_counter.upper_lower_crossings;
        let lower_upper_crossings = self.crossing_matrix_filler.get_crossing_matrix_entry(
            graph,
            current_node_order,
            self.free_layer_index,
            &layer_positions,
            lower_node,
            upper_node,
        ) + left_in_layer.1
            + right_in_layer.1
            + self.north_south_counter.lower_upper_crossings;

        upper_lower_crossings > lower_upper_crossings
    }

    fn constraints_prevent_switch(
        &self,
        graph: &LGraph,
        free_layer: &[NodeId],
        upper_node_index: usize,
        lower_node_index: usize,
    ) -> bool {
        let upper_node = free_layer[upper_node_index];
        let lower_node = free_layer[lower_node_index];
        have_successor_constraints(graph, upper_node, lower_node)
            || have_layout_unit_constraints(graph, upper_node, lower_node)
            || are_normal_and_north_south_port_dummy(graph, upper_node, lower_node)
    }
}

/// `GraphInfoHolder.portDistributor`, which `ISweepPortDistributor.create` makes once for each
/// graph: a `GreedyPortDistributor` for the two-sided greedy switch and a barycenter one for the
/// other two types.
#[derive(Debug, Clone)]
enum PortDistributor {
    Greedy(GreedyPortDistributor),
    Barycenter(BarycenterPortDistributor),
    /// The barycenter type hands its distributor to `BarycenterHeuristic`, which distributes the
    /// ports itself.
    InHeuristic,
}

impl PortDistributor {
    /// `ISweepPortDistributor.create`. It draws from `random` as ELK does: nothing for the
    /// two-sided type, and one boolean for the others, which picks the kind of barycenter
    /// distributor.
    fn create(
        cross_min_type: CrossMinType,
        random: &mut JavaRandom,
        current_order: &[Vec<NodeId>],
    ) -> Self {
        if cross_min_type == CrossMinType::TwoSidedGreedySwitch {
            Self::Greedy(GreedyPortDistributor::default())
        } else {
            let kind = create_port_distributor_kind(cross_min_type, random);
            Self::Barycenter(BarycenterPortDistributor::new(kind, current_order))
        }
    }

    /// Hands the barycenter distributor to the heuristic of the barycenter type, which owns it.
    ///
    /// Panics if the distributor is not a barycenter one that is still here. That is a bug in the
    /// caller, never caller input: only the barycenter type hands it over, once.
    fn hand_to_heuristic(&mut self) -> BarycenterPortDistributor {
        match std::mem::replace(self, Self::InHeuristic) {
            Self::Barycenter(distributor) => distributor,
            _ => {
                panic!("only a barycenter port distributor that is still here goes to a heuristic")
            }
        }
    }

    fn distribute_ports_while_sweeping(
        &mut self,
        graph: &mut LGraph,
        others: OtherGraphs<'_>,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) -> bool {
        match self {
            Self::Greedy(distributor) => distributor.distribute_ports_while_sweeping(
                graph,
                others,
                order,
                current_index,
                forward,
            ),
            Self::Barycenter(distributor) => {
                distributor.distribute_ports_while_sweeping(graph, order, current_index, forward);
                // `AbstractBarycenterPortDistributor` cannot tell whether it improved anything.
                false
            }
            Self::InHeuristic => false,
        }
    }
}

/// Ports `GreedyPortDistributor`: puts the ports of each node on the side that faces the sweep in
/// the order that has the fewest crossings, switching neighbours one pair at a time.
///
/// ELK uses it for the two-sided greedy switch. The other crossing minimizers use a barycenter
/// port distributor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GreedyPortDistributor {
    /// The counter keeps the port positions of every count it has made, as ELK's counter does:
    /// it owns an array that lives as long as the graph's `GraphInfoHolder`.
    crossings_counter: CrossingsCounter,
}

impl GreedyPortDistributor {
    /// `GreedyPortDistributor.distributePortsWhileSweeping`. `others` holds the graphs inside the
    /// nodes of `graph`, whose layers count in the crossings at a node's ports.
    fn distribute_ports_while_sweeping(
        &mut self,
        graph: &mut LGraph,
        others: OtherGraphs<'_>,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) -> bool {
        if current_index >= order.len() {
            return false;
        }
        self.initialize(graph, order, current_index, forward);
        self.distribute_ports_in_layer(graph, others, order, current_index, forward)
    }

    /// `GreedyPortDistributor.initialize`: numbers the ports the counts will compare.
    fn initialize(
        &mut self,
        graph: &LGraph,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) {
        if forward && current_index > 0 {
            self.crossings_counter.init_for_counting_between(
                graph,
                &order[current_index - 1],
                &order[current_index],
            );
        } else if !forward && current_index + 1 < order.len() {
            self.crossings_counter.init_for_counting_between(
                graph,
                &order[current_index],
                &order[current_index + 1],
            );
        } else {
            self.crossings_counter
                .init_port_positions_for_in_layer_crossings_with_tree(
                    graph,
                    &order[current_index],
                    if forward {
                        PortSide::West
                    } else {
                        PortSide::East
                    },
                );
        }
    }

    /// `GreedyPortDistributor.distributePortsInLayer`.
    fn distribute_ports_in_layer(
        &mut self,
        graph: &mut LGraph,
        others: OtherGraphs<'_>,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) -> bool {
        let side = if forward {
            PortSide::West
        } else {
            PortSide::East
        };
        let mut improved = false;
        for &node in &order[current_index] {
            if graph.layerless_nodes[node]
                .port_constraints
                .is_order_fixed()
            {
                continue;
            }
            improved |= self.distribute_ports_on_node(graph, others, node, side, forward);
        }
        improved
    }

    /// `GreedyPortDistributor.distributePortsOnNode`.
    fn distribute_ports_on_node(
        &mut self,
        graph: &mut LGraph,
        others: OtherGraphs<'_>,
        node: NodeId,
        side: PortSide,
        forward: bool,
    ) -> bool {
        // The ports of the side in the order of the node's port list. A southern or western
        // side is read from the other end.
        let current_order = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
        let slots = current_order
            .iter()
            .enumerate()
            .filter(|(_, port)| graph.layerless_nodes[node].ports[**port].side == side)
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        let mut ports = slots
            .iter()
            .map(|slot| current_order[*slot])
            .collect::<Vec<_>>();
        if matches!(side, PortSide::South | PortSide::West) {
            ports.reverse();
        }

        // A node with children counts the crossings of its ports inside the node as well.
        let nested_graph = if ports.is_empty() {
            None
        } else {
            graph.layerless_nodes[node]
                .nested_graph
                .and_then(|nested| others.get(nested))
        };
        let inner_layers = nested_graph
            .map(|nested| nested.layers())
            .filter(|layers| !layers.is_empty());
        let mut hierarchical_counter = BetweenLayerEdgeTwoNodeCrossingsCounter::new();
        let inner_free_layer = inner_layers
            .as_ref()
            .map(|layers| if forward { 0 } else { layers.len() - 1 });

        let mut improved = false;
        loop {
            let mut continue_switching = false;
            for index in 0..ports.len().saturating_sub(1) {
                let upper_port = PortRef {
                    node,
                    port: ports[index],
                };
                let lower_port = PortRef {
                    node,
                    port: ports[index + 1],
                };
                let (mut upper_lower, mut lower_upper) = self
                    .crossings_counter
                    .count_crossings_between_ports_in_both_orders(graph, upper_port, lower_port);
                if let (Some(nested), Some(layers), Some(free_layer)) =
                    (nested_graph, inner_layers.as_ref(), inner_free_layer)
                {
                    let dummy_of = |port: PortRef| {
                        graph.layerless_nodes[node].ports[port.port]
                            .port_dummy
                            .as_ref()
                            .filter(|dummy| dummy.graph_id == nested.id)
                            .map(|dummy| dummy.node)
                    };
                    if let (Some(upper_node), Some(lower_node)) =
                        (dummy_of(upper_port), dummy_of(lower_port))
                    {
                        hierarchical_counter.count_both_side_crossings(
                            nested, layers, free_layer, upper_node, lower_node,
                        );
                        upper_lower += hierarchical_counter.upper_lower_crossings as f64;
                        lower_upper += hierarchical_counter.lower_upper_crossings as f64;
                    }
                }
                if upper_lower > lower_upper {
                    improved = true;
                    // The counter learns of the switch. The port list changes below, once.
                    self.crossings_counter
                        .switch_ports(graph, upper_port, lower_port);
                    ports.swap(index, index + 1);
                    continue_switching = true;
                }
            }
            if !continue_switching {
                break;
            }
        }

        if improved {
            // Write the new order of the side back to the node's port list.
            if matches!(side, PortSide::South | PortSide::West) {
                ports.reverse();
            }
            let mut new_order = current_order;
            for (slot, port) in slots.iter().zip(&ports) {
                new_order[*slot] = *port;
            }
            // The counter keys its positions by port id, which stays when a port moves. The order
            // is a permutation by construction, so a refusal is a bug in this function, never
            // caller input. It stays loud in release builds.
            assert!(
                graph.reorder_node_ports(node, new_order),
                "the switched port order of node {} is not a permutation of its ports",
                node.index()
            );
        }
        improved
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GreedySwitchHeuristic {
    greedy_switch_type: CrossMinType,
}

impl GreedySwitchHeuristic {
    fn new(greedy_switch_type: CrossMinType) -> Self {
        Self { greedy_switch_type }
    }

    fn minimize_crossings(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        free_layer_index: usize,
        forward: bool,
    ) -> bool {
        self.set_up_and_switch(graph, order, free_layer_index, forward, true)
    }

    fn set_first_layer_order(
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

#[cfg(test)]
pub(crate) fn minimize_crossings_layer_sweep(graph: &mut LGraph) -> bool {
    minimize_crossings_layer_sweep_with_type(graph, OtherGraphs::none(), CrossMinType::Barycenter)
}

pub(crate) fn minimize_crossings_layer_sweep_with_type(
    graph: &mut LGraph,
    others: OtherGraphs<'_>,
    cross_min_type: CrossMinType,
) -> bool {
    if graph.layers().is_empty() || graph.layers().iter().all(|layer| layer.nodes().is_empty()) {
        return false;
    }
    if graph.layers().len() == 1 && graph.layers()[0].nodes().len() <= 1 {
        return false;
    }

    initialize_crossing_minimization_port_ids(graph);
    match cross_min_type {
        CrossMinType::Barycenter => minimize_barycenter(graph),
        CrossMinType::OneSidedGreedySwitch => minimize_one_sided_greedy_switch(graph, others),
        CrossMinType::TwoSidedGreedySwitch => minimize_two_sided_greedy_switch(graph, others),
    }
}

pub(crate) fn minimize_crossings_layer_sweep_hierarchical_with_type(
    graphs: &mut GraphArena,
    cross_min_type: CrossMinType,
) -> bool {
    let graph = graphs.root();
    if graph.layers().is_empty() || graph.layers().iter().all(|layer| layer.nodes().is_empty()) {
        let hierarchical_graphs = graph
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

    initialize_crossing_minimization_port_ids_hierarchy(graphs);
    let mut sweep = HierarchySweep::new(graphs, cross_min_type);
    sweep.minimize(graphs)
}

fn minimize_barycenter(graph: &mut LGraph) -> bool {
    let mut graph_info = GraphInfoHolder::new(graph);

    let mut random = graph.random.clone();
    let random_seed = random.next_long();
    let mut port_distributor = PortDistributor::create(
        CrossMinType::Barycenter,
        &mut random,
        &graph_info.current_node_order,
    );
    random.set_seed(random_seed);

    let mut best_crossings = f64::MAX;
    let thoroughness = graph.options.thoroughness.max(1);
    // Match the shared FIRST_TRY / SECOND_TRY Property ID, as in the hierarchy sweep:
    // only the first run preserves model order before randomized attempts begin.
    let first_try_with_initial_order =
        graph.options.consider_model_order_strategy != OrderingStrategy::None;

    // One heuristic and one port distributor serve every run, as in GraphInfoHolder: the port
    // ranks and barycenters of an earlier run stay in the arrays until a later run overwrites them.
    let mut heuristic =
        BarycenterHeuristic::new(graph, random.clone(), port_distributor.hand_to_heuristic());
    for run_index in 0..thoroughness {
        let crossings = minimize_crossings_with_counter(
            graph,
            &mut graph_info,
            &mut heuristic,
            first_try_with_initial_order && run_index == 0,
        );
        random = heuristic.random.clone();

        if crossings < best_crossings {
            best_crossings = crossings;
            if let Some(copy) = graph_info.currently_best_node_and_port_order.clone() {
                graph_info.set_best_node_and_port_order(copy);
            }
            if best_crossings == 0.0 {
                break;
            }
        }
    }

    graph.random = random;
    let Some(best_sweep) = graph_info.get_best_sweep().cloned() else {
        return false;
    };
    best_sweep.transfer_node_and_port_orders_to_graph(graph, true)
}

fn minimize_two_sided_greedy_switch(graph: &mut LGraph, others: OtherGraphs<'_>) -> bool {
    let mut graph_info = GraphInfoHolder::new(graph);
    // LayerSweepCrossingMinimizer.initialize draws a seed for every cross-min type, also for
    // the greedy switch. This one is never used, but the draw moves the shared Random on for
    // the next connected component. ISweepPortDistributor.create draws nothing for two-sided.
    graph.random.next_long();
    let mut heuristic = GreedySwitchHeuristic::new(CrossMinType::TwoSidedGreedySwitch);
    let mut port_distributor = PortDistributor::create(
        CrossMinType::TwoSidedGreedySwitch,
        &mut graph.random,
        &graph_info.current_node_order,
    );
    let mut is_forward_sweep = graph.random.next_bool();

    loop {
        let mut sweep_improved = heuristic.set_first_layer_order(
            graph,
            &mut graph_info.current_node_order,
            is_forward_sweep,
        );
        sweep_improved |= sweep_reducing_crossings_greedy(
            graph,
            others,
            &mut graph_info,
            &mut heuristic,
            &mut port_distributor,
            is_forward_sweep,
        );
        if !sweep_improved {
            break;
        }
        is_forward_sweep = !is_forward_sweep;
    }

    graph_info.set_currently_best_node_and_port_order(graph);

    let Some(best_sweep) = graph_info.get_best_sweep().cloned() else {
        return false;
    };
    best_sweep.transfer_node_and_port_orders_to_graph(graph, true)
}

fn minimize_one_sided_greedy_switch(graph: &mut LGraph, others: OtherGraphs<'_>) -> bool {
    let mut graph_info = GraphInfoHolder::new(graph);
    // LayerSweepCrossingMinimizer.initialize draws a seed. That seed is never used, but the draw
    // moves the shared Random on for the next connected component. ISweepPortDistributor.create
    // then draws a boolean to choose the barycenter port distributor.
    graph.random.next_long();
    let mut port_distributor = PortDistributor::create(
        CrossMinType::OneSidedGreedySwitch,
        &mut graph.random,
        &graph_info.current_node_order,
    );
    let mut heuristic = GreedySwitchHeuristic::new(CrossMinType::OneSidedGreedySwitch);
    let mut is_forward_sweep = graph.random.next_bool();

    heuristic.set_first_layer_order(graph, &mut graph_info.current_node_order, is_forward_sweep);
    sweep_reducing_crossings_greedy(
        graph,
        others,
        &mut graph_info,
        &mut heuristic,
        &mut port_distributor,
        is_forward_sweep,
    );

    let mut crossings_in_graph = graph_info
        .crossings_counter
        .count_all_crossings_in_order(graph, &graph_info.current_node_order);
    loop {
        graph_info.set_currently_best_node_and_port_order(graph);
        if crossings_in_graph == 0.0 {
            break;
        }

        is_forward_sweep = !is_forward_sweep;
        let old_number_of_crossings = crossings_in_graph;
        sweep_reducing_crossings_greedy(
            graph,
            others,
            &mut graph_info,
            &mut heuristic,
            &mut port_distributor,
            is_forward_sweep,
        );
        crossings_in_graph = graph_info
            .crossings_counter
            .count_all_crossings_in_order(graph, &graph_info.current_node_order);
        if !sweep_improved_crossings(old_number_of_crossings, crossings_in_graph) {
            break;
        }
    }

    let Some(best_sweep) = graph_info.get_best_sweep().cloned() else {
        return false;
    };
    best_sweep.transfer_node_and_port_orders_to_graph(graph, true)
}

/// The condition of `do { ... } while (oldNumberOfCrossings > crossingsInGraph)`. A count can be
/// `NaN`, and then the loop ends: no comparison with `NaN` is true.
fn sweep_improved_crossings(old_number_of_crossings: f64, crossings_in_graph: f64) -> bool {
    old_number_of_crossings > crossings_in_graph
}

fn minimize_crossings_with_counter(
    graph: &mut LGraph,
    graph_info: &mut GraphInfoHolder,
    heuristic: &mut BarycenterHeuristic,
    first_try_with_initial_order: bool,
) -> f64 {
    let mut is_forward_sweep = heuristic.random.next_bool();

    let initial_crossings = graph_info
        .crossings_counter
        .count_all_crossings_in_order(graph, &graph_info.current_node_order);
    if initial_crossings == 0.0 && first_try_with_initial_order {
        graph_info.set_currently_best_node_and_port_order(graph);
        return 0.0;
    }

    if !first_try_with_initial_order
        || graph.options.consider_model_order_strategy == OrderingStrategy::None
    {
        heuristic.set_first_layer_order(
            graph,
            &mut graph_info.current_node_order,
            is_forward_sweep,
        );
    } else {
        is_forward_sweep = first_try_with_initial_order;
    }

    sweep_reducing_crossings(
        graph,
        graph_info,
        heuristic,
        is_forward_sweep,
        !first_try_with_initial_order,
    );

    let mut crossings_in_graph = graph_info
        .crossings_counter
        .count_all_crossings_in_order(graph, &graph_info.current_node_order);
    loop {
        graph_info.set_currently_best_node_and_port_order(graph);
        if crossings_in_graph == 0.0 {
            return 0.0;
        }

        is_forward_sweep = !is_forward_sweep;
        let old_number_of_crossings = crossings_in_graph;
        sweep_reducing_crossings(graph, graph_info, heuristic, is_forward_sweep, false);
        crossings_in_graph = graph_info
            .crossings_counter
            .count_all_crossings_in_order(graph, &graph_info.current_node_order);
        if !sweep_improved_crossings(old_number_of_crossings, crossings_in_graph) {
            return old_number_of_crossings;
        }
    }
}

fn sweep_reducing_crossings(
    graph: &mut LGraph,
    graph_info: &mut GraphInfoHolder,
    heuristic: &mut BarycenterHeuristic,
    forward: bool,
    first_sweep: bool,
) {
    let length = graph_info.current_node_order.len();
    if length == 0 {
        return;
    }

    heuristic.port_distributor.distribute_ports_while_sweeping(
        graph,
        &graph_info.current_node_order,
        first_index(forward, length),
        forward,
    );

    let mut index = first_free(forward, length);
    while is_not_end(length, index, forward) {
        let free_layer_index = index as usize;
        heuristic.minimize_crossings(
            graph,
            &mut graph_info.current_node_order,
            free_layer_index,
            forward,
            first_sweep,
        );
        heuristic.port_distributor.distribute_ports_while_sweeping(
            graph,
            &graph_info.current_node_order,
            free_layer_index,
            forward,
        );
        index = next_index(index, forward);
    }
}

fn sweep_reducing_crossings_greedy(
    graph: &mut LGraph,
    others: OtherGraphs<'_>,
    graph_info: &mut GraphInfoHolder,
    heuristic: &mut GreedySwitchHeuristic,
    port_distributor: &mut PortDistributor,
    forward: bool,
) -> bool {
    let length = graph_info.current_node_order.len();
    if length == 0 {
        return false;
    }

    let mut improved = port_distributor.distribute_ports_while_sweeping(
        graph,
        others,
        &graph_info.current_node_order,
        first_index(forward, length),
        forward,
    );
    let mut index = first_free(forward, length);
    while is_not_end(length, index, forward) {
        let free_layer_index = index as usize;
        improved |= heuristic.minimize_crossings(
            graph,
            &mut graph_info.current_node_order,
            free_layer_index,
            forward,
        );
        improved |= port_distributor.distribute_ports_while_sweeping(
            graph,
            others,
            &graph_info.current_node_order,
            free_layer_index,
            forward,
        );
        index = next_index(index, forward);
    }
    improved
}

fn actual_ports_of_type(graph: &LGraph, node: NodeId, port_type: PortType) -> Vec<PortRef> {
    graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .filter_map(|(port, port_data)| {
            let matches = match port_type {
                PortType::Input => !port_data.incoming_edges().is_empty(),
                PortType::Output => !port_data.outgoing_edges().is_empty(),
            };
            matches.then_some(PortRef { node, port })
        })
        .collect()
}

fn crossing_minimization_port_id(graph: &LGraph, port: PortRef) -> Option<usize> {
    graph
        .layerless_nodes
        .get(port.node)?
        .ports
        .get(port.port)
        .and_then(|port_data| port_data.crossing_minimization_id)
}

fn connected_ports(graph: &LGraph, port: PortRef) -> Vec<PortRef> {
    connected_edges(graph, port)
        .filter_map(|edge| other_end_of_edge(graph, edge, port))
        .collect()
}

fn same_layer(graph: &LGraph, left: NodeId, right: NodeId) -> bool {
    graph.layerless_nodes[left].layer_index() == graph.layerless_nodes[right].layer_index()
}

fn north_south_port_barycenter_sum(
    graph: &LGraph,
    node_positions: &HashMap<NodeId, usize>,
    port: PortRef,
    dummy_node: NodeId,
    absurdly_large: f64,
) -> Option<f64> {
    let dummy = graph.layerless_nodes.get(dummy_node)?;
    let mut input = false;
    let mut output = false;

    for dummy_port in &dummy.ports {
        if dummy_port.node() != dummy_node {
            continue;
        }
        if !dummy_port.outgoing_edges().is_empty() {
            output = true;
        } else if !dummy_port.incoming_edges().is_empty() {
            input = true;
        }
    }

    let position = node_positions.get(&dummy_node).copied()? as f64;
    let side = graph.layerless_nodes[port.node].ports[port.port].side;

    let sum = if input && !output {
        if side == PortSide::North {
            -position
        } else {
            absurdly_large - position
        }
    } else if output && !input {
        position + 1.0
    } else if input && output {
        if side == PortSide::North {
            0.0
        } else {
            absurdly_large / 2.0
        }
    } else {
        0.0
    };
    Some(sum)
}

fn layer_node_count(graph: &LGraph, node: NodeId, order: &[Vec<NodeId>]) -> usize {
    let Some(layer_index) = graph.layerless_nodes[node].layer_index() else {
        return 0;
    };
    order.get(layer_index).map_or(0, Vec::len)
}

fn port_type_for(forward: bool) -> PortType {
    if forward {
        PortType::Output
    } else {
        PortType::Input
    }
}

fn port_side_order(side: PortSide) -> u8 {
    match side {
        PortSide::Undefined => 0,
        PortSide::North => 1,
        PortSide::East => 2,
        PortSide::South => 3,
        PortSide::West => 4,
    }
}

fn build_adjacencies<L: AsRef<[NodeId]>>(
    graph: &LGraph,
    current_node_order: &[L],
    free_layer_index: usize,
    side: PortSide,
) -> HashMap<NodeId, AdjacencyList> {
    current_node_order
        .get(free_layer_index)
        .into_iter()
        .flat_map(|layer| layer.as_ref().iter())
        .copied()
        .map(|node| {
            (
                node,
                AdjacencyList::new(graph, current_node_order, node, side),
            )
        })
        .collect()
}

/// `BetweenLayerEdgeTwoNodeCrossingsCounter.AdjacencyList.addAdjacencyOf`. An edge to the
/// position of the entry added last raises that entry's cardinality; any other edge adds a new
/// entry. So two edges to one port merge only when they are collected one after the other. ELK
/// sorts the list afterwards and never merges the entries that the sort brings together. The
/// count then takes the second entry at a position as an adjacency below the first, and a switch
/// can look better than it is. elkjs counts this way, so this port does too.
fn add_adjacency(adjacency_list: &mut Vec<Adjacency>, position: usize) {
    if let Some(last) = adjacency_list.last_mut()
        && last.position == position
    {
        last.cardinality += 1;
        last.current_cardinality += 1;
        return;
    }
    adjacency_list.push(Adjacency::new(position));
}

fn neighboring_layer_port_positions<L: AsRef<[NodeId]>>(
    graph: &LGraph,
    current_node_order: &[L],
    node: NodeId,
    side: PortSide,
) -> HashMap<PortRef, usize> {
    let mut positions = HashMap::new();
    let Some(layer_index) = graph.layerless_nodes[node].layer_index() else {
        return positions;
    };
    let neighbor = match side {
        PortSide::West if layer_index > 0 => Some((layer_index - 1, PortSide::East)),
        PortSide::East if layer_index + 1 < current_node_order.len() => {
            Some((layer_index + 1, PortSide::West))
        }
        _ => None,
    };
    let Some((neighbor_index, neighbor_side)) = neighbor else {
        return positions;
    };

    for (position, port) in current_node_order[neighbor_index]
        .as_ref()
        .iter()
        .copied()
        .flat_map(|node| ports_in_north_south_east_west_order(graph, node, neighbor_side))
        .enumerate()
    {
        positions.insert(port, position);
    }
    positions
}

fn neighboring_layer_exists<L: AsRef<[NodeId]>>(
    current_node_order: &[L],
    free_layer_index: usize,
    side: PortSide,
) -> bool {
    match side {
        PortSide::West => free_layer_index > 0,
        PortSide::East => free_layer_index + 1 < current_node_order.len(),
        _ => false,
    }
}

fn edges_connected_to_side(graph: &LGraph, port: PortRef, side: PortSide) -> Vec<EdgeId> {
    match side {
        PortSide::West => graph.layerless_nodes[port.node].ports[port.port]
            .incoming_edges()
            .to_vec(),
        PortSide::East => graph.layerless_nodes[port.node].ports[port.port]
            .outgoing_edges()
            .to_vec(),
        _ => connected_edges(graph, port).collect(),
    }
}

fn adjacent_port_of_side(graph: &LGraph, edge: EdgeId, side: PortSide) -> PortRef {
    if side == PortSide::West {
        graph.edges[edge].source()
    } else {
        graph.edges[edge].target()
    }
}

fn is_below(first_port: usize, second_port: usize) -> bool {
    first_port > second_port
}

fn layer_position_map(layer: &[NodeId]) -> HashMap<NodeId, usize> {
    layer
        .iter()
        .copied()
        .enumerate()
        .map(|(position, node)| (node, position))
        .collect()
}

fn ports_for_counter_side(
    graph: &LGraph,
    node: NodeId,
    side: PortSide,
    top_down: bool,
) -> Vec<PortRef> {
    let mut ports = graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .filter_map(|(port_index, port)| {
            (port.side == side).then_some(PortRef {
                node,
                port: port_index,
            })
        })
        .collect::<Vec<_>>();
    let preserve_order =
        (side == PortSide::East && top_down) || (side == PortSide::West && !top_down);
    if !preserve_order {
        ports.reverse();
    }
    ports
}

fn connected_edges(graph: &LGraph, port: PortRef) -> impl Iterator<Item = EdgeId> + '_ {
    graph.layerless_nodes[port.node].ports[port.port]
        .incoming_edges()
        .iter()
        .chain(
            graph.layerless_nodes[port.node].ports[port.port]
                .outgoing_edges()
                .iter(),
        )
        .copied()
}

fn is_edge_self_loop(graph: &LGraph, edge: EdgeId) -> bool {
    graph
        .edges
        .get(edge)
        .is_some_and(|edge| edge.source().node == edge.target().node)
}

fn is_in_layer_edge(graph: &LGraph, edge: EdgeId) -> bool {
    let Some(edge) = graph.edges.get(edge) else {
        return false;
    };
    graph.layerless_nodes[edge.source().node].layer_index()
        == graph.layerless_nodes[edge.target().node].layer_index()
}

fn other_end_of_edge(graph: &LGraph, edge: EdgeId, port: PortRef) -> Option<PortRef> {
    let edge = graph.edges.get(edge)?;
    if edge.source() == port {
        Some(edge.target())
    } else if edge.target() == port {
        Some(edge.source())
    } else {
        None
    }
}

fn are_normal_and_north_south_port_dummy(
    graph: &LGraph,
    upper_node: NodeId,
    lower_node: NodeId,
) -> bool {
    (is_north_south_node(graph, upper_node)
        && graph.layerless_nodes[lower_node].kind() == LNodeKind::Normal)
        || (is_north_south_node(graph, lower_node)
            && graph.layerless_nodes[upper_node].kind() == LNodeKind::Normal)
}

/// `SwitchDecider.haveSuccessorConstraints`.
fn have_successor_constraints(graph: &LGraph, upper_node: NodeId, lower_node: NodeId) -> bool {
    graph
        .in_layer_successor_constraints(upper_node)
        .contains(&lower_node)
}

/// `SwitchDecider.haveLayoutUnitConstraints`.
fn have_layout_unit_constraints(graph: &LGraph, upper_node: NodeId, lower_node: NodeId) -> bool {
    let neither_node_is_long_edge_dummy =
        !is_long_edge_dummy(graph, upper_node) && !is_long_edge_dummy(graph, lower_node);

    // If the nodes are part of a layout unit with more nodes than themselves, the layout units
    // must be equal for a switch to be allowed.
    let upper_layout_unit = graph.in_layer_layout_unit(upper_node);
    let lower_layout_unit = graph.in_layer_layout_unit(lower_node);
    let are_in_different_layout_units = upper_layout_unit != lower_layout_unit;

    let part_of_multi_node_layout_unit =
        |node: NodeId, unit: Option<NodeId>| unit.is_some_and(|unit| unit != node);
    let mut nodes_have_layout_units = part_of_multi_node_layout_unit(upper_node, upper_layout_unit)
        || part_of_multi_node_layout_unit(lower_node, lower_layout_unit);

    let upper_node_has_northern_edges = has_edges_on_side(graph, upper_node, PortSide::North);
    let lower_node_has_southern_edges = has_edges_on_side(graph, lower_node, PortSide::South);

    // ELK's hotfix for #162: if north or south edges are present, there must be a layout unit.
    nodes_have_layout_units |= has_edges_on_side(graph, upper_node, PortSide::South)
        || has_edges_on_side(graph, lower_node, PortSide::North);

    let has_layout_unit_constraint = (nodes_have_layout_units && are_in_different_layout_units)
        || (upper_node_has_northern_edges || lower_node_has_southern_edges);

    neither_node_is_long_edge_dummy && has_layout_unit_constraint
}

/// `SwitchDecider.hasEdgesOnSide`: whether a port of the side has an edge or a dummy that holds
/// its edges.
fn has_edges_on_side(graph: &LGraph, node: NodeId, side: PortSide) -> bool {
    graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .filter(|(_, port)| port.side == side)
        .any(|(index, port)| {
            port.port_dummy.is_some()
                || connected_edges(graph, PortRef { node, port: index })
                    .next()
                    .is_some()
        })
}

fn is_north_south_node(graph: &LGraph, node: NodeId) -> bool {
    graph.layerless_nodes[node].kind() == LNodeKind::NorthSouthPort
}

fn is_long_edge_dummy(graph: &LGraph, node: NodeId) -> bool {
    graph.layerless_nodes[node].kind() == LNodeKind::LongEdge
}

/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.originPortOf`: the port of the regular node a
/// dummy was made for.
fn origin_port_of(graph: &LGraph, node: NodeId) -> Option<PortRef> {
    graph.layerless_nodes[node]
        .ports
        .first()
        .and_then(|port| port.origin_port)
}

/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.isNorthOfNormalNode`.
fn north_south_dummy_is_north_of_normal_node(graph: &LGraph, node: NodeId) -> bool {
    origin_port_of(graph, node).is_some_and(|origin| {
        graph.layerless_nodes[origin.node].ports[origin.port].side == PortSide::North
    })
}

/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.haveDifferentOrigins`: two dummies have the
/// origin of their ports in different nodes.
fn have_different_origins(graph: &LGraph, upper_node: NodeId, lower_node: NodeId) -> bool {
    origin_port_of(graph, upper_node).map(|origin| origin.node)
        != origin_port_of(graph, lower_node).map(|origin| origin.node)
}

/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.numberOfNorthSouthEdges`: the ports of the
/// side that have a dummy.
fn number_of_north_south_edges(graph: &LGraph, node: NodeId, side: PortSide) -> usize {
    north_south_ports_with_incident_edges(graph, node, side).count()
}

fn connected_ports_for_sweep(graph: &LGraph, port: PortRef, forward: bool) -> Vec<PortRef> {
    let edge_indices = if forward {
        graph.layerless_nodes[port.node].ports[port.port].incoming_edges()
    } else {
        graph.layerless_nodes[port.node].ports[port.port].outgoing_edges()
    };

    edge_indices
        .iter()
        .filter_map(|edge| {
            if forward {
                graph.edges.get(*edge).map(|edge| edge.source())
            } else {
                graph.edges.get(*edge).map(|edge| edge.target())
            }
        })
        .collect()
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

fn has_connected_ports_on_side(graph: &LGraph, node: NodeId, side: PortSide) -> bool {
    ports_in_north_south_east_west_order(graph, node, side)
        .into_iter()
        .any(|port| connected_edges(graph, port).next().is_some())
}

fn external_dummy_origin_side(
    graph: &LGraph,
    parent_graph: Option<&LGraph>,
    node: NodeId,
) -> PortSide {
    let Some(dummy) = graph.layerless_nodes.get(node) else {
        return PortSide::Undefined;
    };
    // A node that is no external port dummy has no origin and no side of its own.
    let external = dummy.external_port();
    if let Some(origin) = external.and_then(|external| external.origin_port.as_ref()) {
        if origin.graph_id == graph.id
            && let Some(side) = port_side_at(graph, origin.port)
        {
            return side;
        }
        if let Some(parent_graph) = parent_graph
            && origin.graph_id == parent_graph.id
            && let Some(side) = port_side_at(parent_graph, origin.port)
        {
            return side;
        }
    }
    dummy.external_port_side()
}

fn port_side_at(graph: &LGraph, port: PortRef) -> Option<PortSide> {
    graph
        .layerless_nodes
        .get(port.node)
        .and_then(|node| node.ports.get(port.port))
        .map(|port| port.side)
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

fn apply_parent_hierarchical_port_order_on_sweep_side(
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

fn first_index(forward: bool, length: usize) -> usize {
    if forward { 0 } else { length - 1 }
}

fn first_free(forward: bool, length: usize) -> isize {
    if forward { 1 } else { length as isize - 2 }
}

fn next_index(index: isize, forward: bool) -> isize {
    if forward { index + 1 } else { index - 1 }
}

fn is_not_end(length: usize, free_layer_index: isize, forward: bool) -> bool {
    if forward {
        free_layer_index < length as isize
    } else {
        free_layer_index >= 0
    }
}

fn is_first_layer(order: &[Vec<NodeId>], current_index: usize, forward: bool) -> bool {
    current_index == first_index(forward, order.len())
}

trait PortSideExt {
    fn is_north(&self) -> bool;
}

impl PortSideExt for crate::graph::PortSide {
    fn is_north(&self) -> bool {
        *self == crate::graph::PortSide::North
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
    use crate::intermediate::split_long_edges;
    use crate::options::{ElkDirection, HierarchyHandling, LayeredOptions};
    use crate::p2layers::layer_network_simplex;
    use crate::p3order::{process_port_sides, sort_port_lists};

    // Mutation: in `add_adjacency`, merge with any entry at the same position, not only the last.
    #[test]
    fn an_adjacency_merges_only_with_the_entry_collected_just_before_it() {
        let mut list = Vec::new();
        for position in [5, 5, 3, 5] {
            add_adjacency(&mut list, position);
        }
        list.sort_by_key(|adjacency| adjacency.position);
        let entries: Vec<(usize, usize)> = list
            .iter()
            .map(|adjacency| (adjacency.position, adjacency.cardinality))
            .collect();
        // The first two edges merge; the third edge to position 5 comes after another position,
        // so it stays its own entry, and the stable sort keeps it behind the merged one.
        assert_eq!(entries, vec![(3, 1), (5, 2), (5, 1)]);
    }

    fn node(id: &str) -> ElkInputNode {
        ElkInputNode {
            ports: Vec::new(),
            parent_settings: Default::default(),
            id: id.to_string(),
            width: 80.0,
            height: 40.0,
            parent: None,
            direction: None,
            hierarchy_handling: None,
            layer_constraint: None,
            port_alignment: None,
            port_constraints: None,
            node_label_placement: crate::options::NodeLabelPlacement::FIXED,
            node_flexibility: crate::options::NodeFlexibility::None,
            ports_surrounding: None,
            nested_options: None,
            labels: Vec::new(),
            node_size: Default::default(),
            self_loop_distribution: Default::default(),
            self_loop_ordering: Default::default(),
        }
    }

    fn edge(id: &str, source: &str, target: &str) -> ElkInputEdge {
        ElkInputEdge {
            source_port: None,
            target_port: None,
            id: id.to_string(),
            source: source.to_string(),
            target: target.to_string(),
            labels: Vec::new(),
            minlen: 1,
            inside_self_loops_yo: false,
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
            container: None,
        }
    }

    fn nested_node(id: &str, parent: Option<&str>) -> ElkInputNode {
        ElkInputNode {
            ports: Vec::new(),
            parent_settings: Default::default(),
            id: id.to_string(),
            width: 54.0,
            height: 68.0,
            parent: parent.map(str::to_string),
            direction: None,
            hierarchy_handling: None,
            layer_constraint: None,
            port_alignment: None,
            port_constraints: None,
            node_label_placement: crate::options::NodeLabelPlacement::FIXED,
            node_flexibility: crate::options::NodeFlexibility::None,
            ports_surrounding: None,
            nested_options: None,
            labels: Vec::new(),
            node_size: Default::default(),
            self_loop_distribution: Default::default(),
            self_loop_ordering: Default::default(),
        }
    }

    fn nested_group(id: &str, parent: Option<&str>) -> ElkInputNode {
        ElkInputNode {
            ports: Vec::new(),
            parent_settings: Default::default(),
            id: id.to_string(),
            width: 0.0,
            height: 0.0,
            parent: parent.map(str::to_string),
            direction: None,
            hierarchy_handling: None,
            layer_constraint: None,
            port_alignment: None,
            port_constraints: None,
            node_label_placement: crate::options::NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopCenter,
            ),
            node_flexibility: crate::options::NodeFlexibility::None,
            ports_surrounding: None,
            nested_options: Some(Box::new(crate::options::LayeredOptions {
                spacing: crate::options::SpacingOptions::layered_base_value(30.0),
                ..Default::default()
            })),
            labels: Vec::new(),
            node_size: Default::default(),
            self_loop_distribution: Default::default(),
            self_loop_ordering: Default::default(),
        }
    }

    fn prepared_graph(
        nodes: Vec<ElkInputNode>,
        edges: Vec<ElkInputEdge>,
    ) -> crate::graph::GraphArena {
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes,
            edges,
        })
        .unwrap();
        layer_network_simplex(&mut graph).unwrap();
        split_long_edges(&mut graph);
        process_port_sides(&mut graph, crate::graph::OtherGraphs::none());
        sort_port_lists(&mut graph);
        graph
    }

    #[test]
    fn complete_dag_randomizes_after_the_shared_initial_order_attempt() {
        // Pinned elkjs 0.9.3, seed 1: both property names have the same ID. Giving
        // SECOND_TRY a distinct ID instead leaves this graph at four crossings.
        for hierarchical in [false, true] {
            let nodes = (0..6).map(|i| node(&format!("n{i}"))).collect();
            let mut edges = Vec::new();
            for source in 0..6 {
                for target in source + 1..6 {
                    edges.push(edge(
                        &format!("e{source}{target}"),
                        &format!("n{source}"),
                        &format!("n{target}"),
                    ));
                }
            }
            let mut graph = import_graph(&ElkInputGraph {
                id: "root".into(),
                options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
                nodes,
                edges,
            })
            .unwrap();
            // Run every processor the schedule has before the sweep, as the full runner does.
            crate::pipeline::run_processors_until(
                &mut graph,
                crate::pipeline::ProcessorKind::SortByInputModelProcessor,
            )
            .unwrap();

            if hierarchical {
                minimize_crossings_layer_sweep_hierarchical_with_type(
                    &mut graph,
                    CrossMinType::Barycenter,
                );
            } else {
                minimize_crossings_layer_sweep(&mut graph);
            }

            assert_eq!(CrossingsCounter::default().count_all_crossings(&graph), 3.0);
            let layer_order = graph.layers()[2]
                .nodes()
                .iter()
                .map(|&index| {
                    let node = &graph.layerless_nodes[index];
                    if node.kind() == LNodeKind::Normal {
                        node.id.as_str()
                    } else {
                        let edge = node
                            .ports
                            .iter()
                            .flat_map(|port| port.outgoing_edges())
                            .next()
                            .unwrap();
                        graph.edges[*edge].id.as_str()
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                layer_order,
                ["e03", "e15", "e13", "e14", "n2", "e04", "e05"]
            );
        }
    }

    // Mutation: hand the random stream back to the root only (`if index == 0`). The stream of the
    // nested graph is then not advanced.
    #[test]
    fn hierarchical_sweep_advances_graph_random_streams_without_order_changes() {
        for cross_min_type in [
            CrossMinType::Barycenter,
            CrossMinType::OneSidedGreedySwitch,
            CrossMinType::TwoSidedGreedySwitch,
        ] {
            let mut root = LGraph::new("root", LayeredOptions::default());
            root.options.consider_model_order_strategy = OrderingStrategy::NodesAndEdges;
            root.random = JavaRandom::new(17);
            let mut child = LGraph::new("child", LayeredOptions::default());
            child.random = JavaRandom::new(31);
            let parent = LNode::new("parent", 10.0, 10.0, None);
            root.layerless_nodes.push(parent);
            root.set_node_layer(NodeId::new(0), 0);
            let mut graphs = GraphArena::new(root);
            let child_id = graphs.nest(GraphId::ROOT, NodeId::new(0), child);

            // Source initialize() draws nextLong from the root. GraphInfoHolder draws
            // a distributor boolean from each graph except for two-sided greedy switch.
            // Barycenter then resets only the root stream before its first sweep; the
            // zero-crossing early return still consumes the sweep-direction boolean.
            let mut expected_root = JavaRandom::new(17);
            let seed = expected_root.next_long();
            let mut expected_child = JavaRandom::new(31);
            if cross_min_type != CrossMinType::TwoSidedGreedySwitch {
                expected_root.next_bool();
                expected_child.next_bool();
            }
            if cross_min_type == CrossMinType::Barycenter {
                expected_root.set_seed(seed);
            }
            expected_root.next_bool();

            minimize_crossings_layer_sweep_hierarchical_with_type(&mut graphs, cross_min_type);
            assert_eq!(graphs.root().layers()[0].nodes(), vec![NodeId::new(0)]);
            assert_eq!(graphs.root().random, expected_root, "{cross_min_type:?}");
            assert_eq!(
                graphs[child_id].random, expected_child,
                "{cross_min_type:?}",
            );
        }
    }

    #[test]
    fn hierarchical_sweep_matches_source_node_port_counter_gate() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(crate::graph::LNode::new("A", 10.0, 10.0, None));
        graph.set_node_layer(NodeId::new(0), 0);
        graph
            .options
            .consider_model_order_crossing_counter_node_influence = 0.0;
        graph
            .options
            .consider_model_order_crossing_counter_port_influence = 1.0;

        let mut graphs = GraphArena::new(graph);
        let sweep = HierarchySweep::new(&mut graphs, CrossMinType::Barycenter);

        assert!(!sweep.use_node_port_order_crossing_counter(&graphs, 0));

        graphs
            .root_mut()
            .options
            .consider_model_order_crossing_counter_node_influence = 1.0;

        assert!(sweep.use_node_port_order_crossing_counter(&graphs, 0));
    }

    // Mutation: list the child graphs in node order, not in layer order: sort `children` by node
    // index in `HierarchySweep::new`.
    #[test]
    fn hierarchical_sweep_discovers_child_graphs_in_current_node_order() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("second", 10.0, 10.0, None));
        graph
            .layerless_nodes
            .push(LNode::new("first", 10.0, 10.0, None));
        graph.set_layer_nodes(0, vec![NodeId::new(1), NodeId::new(0)]);
        let mut graphs = GraphArena::new(graph);
        let second = graphs.nest(
            GraphId::ROOT,
            NodeId::new(0),
            LGraph::new("second:nested", LayeredOptions::default()),
        );
        let first = graphs.nest(
            GraphId::ROOT,
            NodeId::new(1),
            LGraph::new("first:nested", LayeredOptions::default()),
        );

        let sweep = HierarchySweep::new(&mut graphs, CrossMinType::Barycenter);

        // The root lists node 1 before node 0, so its nested graph comes first.
        assert_eq!(sweep.infos[0].child_graphs, vec![first, second]);
    }

    #[test]
    fn layer_sweep_type_decider_uses_origin_port_side_for_external_dummy() {
        let mut parent_graph = LGraph::new("root", LayeredOptions::default());
        parent_graph
            .layerless_nodes
            .push(LNode::new("parent", 10.0, 10.0, None));
        let origin = parent_graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::East,
                Default::default(),
            )
            .unwrap();

        let mut child_graph = LGraph::new("child", LayeredOptions::default());
        let mut external = LNode::new("external:parent", 0.0, 0.0, None);
        external.set_kind(LNodeKind::ExternalPort);
        external.external_port_mut().unwrap().side = PortSide::West;
        external.external_port_mut().unwrap().origin_port = Some(crate::graph::GraphPortRef {
            graph_id: parent_graph.id,
            port: origin,
        });
        child_graph.layerless_nodes.push(external);

        assert_eq!(
            external_dummy_origin_side(&child_graph, Some(&parent_graph), NodeId::new(0)),
            PortSide::East
        );
    }

    #[test]
    fn hierarchical_sweep_saves_currently_best_snapshot_not_current_order() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("A", 10.0, 10.0, None));
        graph
            .layerless_nodes
            .push(LNode::new("B", 10.0, 10.0, None));
        graph.set_node_layer(NodeId::new(0), 0);
        graph.set_node_layer(NodeId::new(1), 0);

        let mut graphs = GraphArena::new(graph);
        let mut sweep = HierarchySweep::new(&mut graphs, CrossMinType::Barycenter);
        sweep.changed.insert(0);
        sweep.set_currently_best_node_orders(&graphs);
        sweep.infos[0].current_node_order[0] = vec![NodeId::new(1), NodeId::new(0)];

        sweep.save_all_node_orders_of_changed_graphs(&graphs);

        assert_eq!(
            sweep.infos[0]
                .best_node_and_port_order
                .as_ref()
                .map(|copy| copy.node_order.as_slice()),
            Some([vec![NodeId::new(0), NodeId::new(1)]].as_slice())
        );
    }

    #[test]
    fn barycenter_orders_free_layer_by_fixed_layer_port_ranks() {
        let mut graph = prepared_graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Right", "Top", "Right"),
                edge("Bottom-Left", "Bottom", "Left"),
            ],
        );
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
        graph.set_layer_nodes(0, vec![top, bottom]);
        graph.set_layer_nodes(1, vec![left, right]);
        initialize_crossing_minimization_port_ids(&mut graph);

        let mut order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let port_distributor =
            BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
        let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);

        heuristic.minimize_crossings(&graph, &mut order, 1, true, false);

        assert_eq!(order[1], vec![right, left]);
    }

    #[test]
    fn forced_model_order_keeps_real_nodes_in_input_order() {
        let mut graph = prepared_graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Right", "Top", "Right"),
                edge("Bottom-Left", "Bottom", "Left"),
            ],
        );
        graph.options.force_node_model_order = true;
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
        graph.set_layer_nodes(0, vec![top, bottom]);
        graph.set_layer_nodes(1, vec![left, right]);
        initialize_crossing_minimization_port_ids(&mut graph);

        let mut order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let port_distributor =
            BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
        let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);

        heuristic.minimize_crossings(&graph, &mut order, 1, true, false);

        assert_eq!(order[1], vec![left, right]);
    }

    #[test]
    fn forced_model_order_still_places_dummy_nodes_by_barycenter() {
        let mut graph = LGraph::new(
            "root",
            LayeredOptions {
                force_node_model_order: true,
                ..LayeredOptions::default()
            },
        );
        let mut first = LNode::new("first", 10.0, 10.0, None);
        first.model_order = Some(0);
        let mut dummy = LNode::new("dummy", 0.0, 0.0, None);
        dummy.set_kind(LNodeKind::LongEdge);
        let mut second = LNode::new("second", 10.0, 10.0, None);
        second.model_order = Some(1);
        graph.layerless_nodes = vec![first, dummy, second].into();
        graph.set_node_layer(NodeId::new(0), 0);
        graph.set_node_layer(NodeId::new(1), 0);
        graph.set_node_layer(NodeId::new(2), 0);
        let order = vec![vec![NodeId::new(0), NodeId::new(1), NodeId::new(2)]];
        let port_distributor =
            BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
        let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);
        heuristic.state_mut(NodeId::new(0)).barycenter = Some(2.0);
        heuristic.state_mut(NodeId::new(1)).barycenter = Some(0.0);
        heuristic.state_mut(NodeId::new(2)).barycenter = Some(1.0);
        let mut layer = order[0].clone();

        heuristic.insertion_sort_by_model_order(&graph, &mut layer);

        assert_eq!(layer, vec![NodeId::new(1), NodeId::new(0), NodeId::new(2)]);
    }

    #[test]
    fn barycenter_port_ranks_keep_same_id_ports_distinct() {
        let mut graph = prepared_graph(
            vec![node("Top"), node("Bottom"), node("Free")],
            vec![
                edge("Top-Free", "Top", "Free"),
                edge("Bottom-Free", "Bottom", "Free"),
            ],
        );
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let free = graph.node_named("Free");
        graph.set_layer_nodes(0, vec![top, bottom]);
        graph.set_layer_nodes(1, vec![free]);
        for port in graph.layerless_nodes[top].ports.slots_mut() {
            port.id = "shared".to_string();
        }
        for port in graph.layerless_nodes[bottom].ports.slots_mut() {
            port.id = "shared".to_string();
        }
        initialize_crossing_minimization_port_ids(&mut graph);

        let order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let mut distributor =
            BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
        distributor.calculate_port_ranks(&graph, &order[0], PortType::Output);
        distributor.distribute_ports(&mut graph, free, PortSide::West, &order);

        let free_ports = graph.layerless_nodes[free]
            .ports
            .iter()
            .map(|port| {
                port.incoming_edges()
                    .first()
                    .map(|edge| graph.edges[*edge].id.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(free_ports, vec![Some("Bottom-Free"), Some("Top-Free")]);
    }

    #[test]
    fn barycenter_port_ranks_persist_between_rank_calculations() {
        let mut graph = prepared_graph(
            vec![node("A"), node("B"), node("C")],
            vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
        );
        let a = graph.node_named("A");
        let b = graph.node_named("B");
        graph.set_layer_nodes(0, vec![a]);
        graph.set_layer_nodes(1, vec![b]);
        initialize_crossing_minimization_port_ids(&mut graph);
        let a_out = graph.layerless_nodes[a]
            .ports
            .iter()
            .position(|port| !port.outgoing_edges().is_empty())
            .unwrap();
        let mut distributor =
            BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &[vec![a], vec![b]]);

        distributor.calculate_port_ranks(&graph, &[a], PortType::Output);
        let before = distributor.rank_of(
            &graph,
            PortRef {
                node: a,
                port: PortId::new(a_out),
            },
        );
        distributor.calculate_port_ranks(&graph, &[b], PortType::Input);
        let after = distributor.rank_of(
            &graph,
            PortRef {
                node: a,
                port: PortId::new(a_out),
            },
        );

        assert_eq!(before, after);
        assert_ne!(after, 0.0);
    }

    #[test]
    fn layer_sweep_keeps_best_crossing_order() {
        let mut graph = prepared_graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Right", "Top", "Right"),
                edge("Bottom-Left", "Bottom", "Left"),
            ],
        );
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
        graph.set_layer_nodes(0, vec![top, bottom]);
        graph.set_layer_nodes(1, vec![left, right]);

        let before = CrossingsCounter::default().count_all_crossings(&graph);
        assert!(minimize_crossings_layer_sweep(&mut graph));
        let after = CrossingsCounter::default().count_all_crossings(&graph);

        assert_eq!(before, 1.0);
        assert_eq!(after, 0.0);
    }

    #[test]
    fn parent_hierarchical_port_order_only_rewrites_sweep_side() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(crate::graph::LNode::new("parent", 80.0, 40.0, None));
        let west_a = graph
            .add_port(
                NodeId::new(0),
                PortType::Input,
                PortSide::West,
                Default::default(),
            )
            .unwrap()
            .port;
        let east_a = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::East,
                Default::default(),
            )
            .unwrap()
            .port;
        let west_b = graph
            .add_port(
                NodeId::new(0),
                PortType::Input,
                PortSide::West,
                Default::default(),
            )
            .unwrap()
            .port;
        let east_b = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::East,
                Default::default(),
            )
            .unwrap()
            .port;
        graph.layerless_nodes[NodeId::new(0)].ports[west_a].id = "west_a".to_string();
        graph.layerless_nodes[NodeId::new(0)].ports[east_a].id = "east_a".to_string();
        graph.layerless_nodes[NodeId::new(0)].ports[west_b].id = "west_b".to_string();
        graph.layerless_nodes[NodeId::new(0)].ports[east_b].id = "east_b".to_string();
        for port in graph.layerless_nodes[NodeId::new(0)].ports.slots_mut() {
            port.inside_connections = true;
        }

        apply_parent_hierarchical_port_order_on_sweep_side(
            &mut graph,
            NodeId::new(0),
            true,
            &[east_b, east_a],
        );

        let port_ids = graph.layerless_nodes[NodeId::new(0)]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>();
        let port_sides = graph.layerless_nodes[NodeId::new(0)]
            .ports
            .iter()
            .map(|port| port.side)
            .collect::<Vec<_>>();
        assert_eq!(port_ids, vec!["west_a", "east_b", "west_b", "east_a"]);
        assert_eq!(
            port_sides,
            vec![
                PortSide::West,
                PortSide::East,
                PortSide::West,
                PortSide::East
            ]
        );
    }

    // Mutation: swap the ends of the edges when the ports change places, as the old index remap did.
    #[test]
    fn parent_hierarchical_port_order_leaves_edge_references_alone() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(crate::graph::LNode::new("parent", 80.0, 40.0, None));
        let _west_a = graph
            .add_port(
                NodeId::new(0),
                PortType::Input,
                PortSide::West,
                Default::default(),
            )
            .unwrap()
            .port;
        let east_a = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::East,
                Default::default(),
            )
            .unwrap()
            .port;
        let _west_b = graph
            .add_port(
                NodeId::new(0),
                PortType::Input,
                PortSide::West,
                Default::default(),
            )
            .unwrap()
            .port;
        let east_b = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::East,
                Default::default(),
            )
            .unwrap()
            .port;
        for port in graph.layerless_nodes[NodeId::new(0)].ports.slots_mut() {
            port.inside_connections = true;
        }
        graph
            .layerless_nodes
            .push(crate::graph::LNode::new("target", 10.0, 10.0, None));
        let target_port = graph
            .add_port(
                NodeId::new(1),
                PortType::Input,
                PortSide::West,
                Default::default(),
            )
            .unwrap()
            .port;

        let edge_a = graph
            .add_edge({
                let mut built_edge = crate::graph::LayeredEdge::new(
                    "edge-a".to_string(),
                    PortRef {
                        node: NodeId::new(0),
                        port: east_a,
                    },
                    PortRef {
                        node: NodeId::new(1),
                        port: target_port,
                    },
                );
                built_edge.source_node_id = "parent".to_string();
                built_edge.target_node_id = "target".to_string();
                built_edge
            })
            .unwrap();
        let edge_b = graph
            .add_edge({
                let mut built_edge = crate::graph::LayeredEdge::new(
                    "edge-b".to_string(),
                    PortRef {
                        node: NodeId::new(0),
                        port: east_b,
                    },
                    PortRef {
                        node: NodeId::new(1),
                        port: target_port,
                    },
                );
                built_edge.source_node_id = "parent".to_string();
                built_edge.target_node_id = "target".to_string();
                built_edge
            })
            .unwrap();

        apply_parent_hierarchical_port_order_on_sweep_side(
            &mut graph,
            NodeId::new(0),
            true,
            &[east_b, east_a],
        );

        // The ports moved in the list. The edges still start at the ports they started at.
        assert_eq!(graph.edges[edge_a].source().port, east_a);
        assert_eq!(graph.edges[edge_b].source().port, east_b);
        assert_eq!(
            graph.layerless_nodes[NodeId::new(0)]
                .ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>(),
            vec!["parent:0", "parent:3", "parent:2", "parent:1"]
        );
        assert_eq!(
            graph.layerless_nodes[NodeId::new(0)]
                .ports
                .iter()
                .enumerate()
                .map(|(index, port)| (index, port.id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (0, "parent:0"),
                (1, "parent:3"),
                (2, "parent:2"),
                (3, "parent:1"),
            ]
        );
    }

    #[test]
    fn north_south_hierarchical_ports_use_source_barycenter_keys() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(crate::graph::LNode::new("parent", 80.0, 40.0, None));
        graph
            .layerless_nodes
            .push(crate::graph::LNode::new("input_dummy", 0.0, 0.0, None));
        graph.layerless_nodes[NodeId::new(1)].set_kind(LNodeKind::ExternalPort);
        graph
            .layerless_nodes
            .push(crate::graph::LNode::new("output_dummy", 0.0, 0.0, None));
        graph.layerless_nodes[NodeId::new(2)].set_kind(LNodeKind::ExternalPort);
        graph
            .layerless_nodes
            .push(crate::graph::LNode::new("both_dummy", 0.0, 0.0, None));
        graph.layerless_nodes[NodeId::new(3)].set_kind(LNodeKind::ExternalPort);
        graph.set_node_layer(NodeId::new(1), 0);
        graph.set_node_layer(NodeId::new(2), 0);
        graph.set_node_layer(NodeId::new(3), 0);
        graph.set_layer_nodes(0, vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]);

        let north_input = graph
            .add_port(
                NodeId::new(0),
                PortType::Input,
                PortSide::North,
                Default::default(),
            )
            .unwrap();
        let south_input = graph
            .add_port(
                NodeId::new(0),
                PortType::Input,
                PortSide::South,
                Default::default(),
            )
            .unwrap();
        let north_output = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::North,
                Default::default(),
            )
            .unwrap();
        let south_both = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::South,
                Default::default(),
            )
            .unwrap();
        for (port, dummy) in [
            (north_input, NodeId::new(1)),
            (south_input, NodeId::new(1)),
            (north_output, NodeId::new(2)),
            (south_both, NodeId::new(3)),
        ] {
            graph.layerless_nodes[NodeId::new(0)].ports[port.port].port_dummy =
                Some(crate::graph::GraphNodeRef {
                    graph_id: GraphId::ROOT,
                    node: dummy,
                });
        }

        let in_dummy_port = graph
            .add_port(
                NodeId::new(1),
                PortType::Input,
                PortSide::South,
                Default::default(),
            )
            .unwrap();
        let out_dummy_port = graph
            .add_port(
                NodeId::new(2),
                PortType::Output,
                PortSide::South,
                Default::default(),
            )
            .unwrap();
        let both_in_dummy_port = graph
            .add_port(
                NodeId::new(3),
                PortType::Input,
                PortSide::South,
                Default::default(),
            )
            .unwrap();
        let both_out_dummy_port = graph
            .add_port(
                NodeId::new(3),
                PortType::Output,
                PortSide::South,
                Default::default(),
            )
            .unwrap();

        graph
            .add_edge({
                let mut built_edge = crate::graph::LayeredEdge::new(
                    "north-in".to_string(),
                    north_input,
                    in_dummy_port,
                );
                built_edge.source_node_id = "parent".to_string();
                built_edge.target_node_id = "input_dummy".to_string();
                built_edge
            })
            .unwrap();
        graph
            .add_edge({
                let mut built_edge =
                    crate::graph::LayeredEdge::new("out".to_string(), out_dummy_port, north_output);
                built_edge.source_node_id = "output_dummy".to_string();
                built_edge.target_node_id = "parent".to_string();
                built_edge
            })
            .unwrap();
        graph
            .add_edge({
                let mut built_edge = crate::graph::LayeredEdge::new(
                    "both-in".to_string(),
                    south_both,
                    both_in_dummy_port,
                );
                built_edge.source_node_id = "parent".to_string();
                built_edge.target_node_id = "both_dummy".to_string();
                built_edge
            })
            .unwrap();
        graph
            .add_edge({
                let mut built_edge = crate::graph::LayeredEdge::new(
                    "both-out".to_string(),
                    both_out_dummy_port,
                    south_both,
                );
                built_edge.source_node_id = "both_dummy".to_string();
                built_edge.target_node_id = "parent".to_string();
                built_edge
            })
            .unwrap();

        let node_positions = graph.layers()[0]
            .nodes()
            .iter()
            .copied()
            .enumerate()
            .map(|(position, node)| (node, position))
            .collect::<HashMap<_, _>>();
        assert_eq!(
            north_south_port_barycenter_sum(
                &graph,
                &node_positions,
                north_input,
                NodeId::new(1),
                7.0
            ),
            Some(-0.0)
        );
        assert_eq!(
            north_south_port_barycenter_sum(
                &graph,
                &node_positions,
                south_input,
                NodeId::new(1),
                7.0
            ),
            Some(7.0)
        );
        assert_eq!(
            north_south_port_barycenter_sum(
                &graph,
                &node_positions,
                north_output,
                NodeId::new(2),
                7.0
            ),
            Some(2.0)
        );
        assert_eq!(
            north_south_port_barycenter_sum(
                &graph,
                &node_positions,
                south_both,
                NodeId::new(3),
                7.0
            ),
            Some(3.5)
        );

        let current_node_positions = [
            (NodeId::new(3), 0usize),
            (NodeId::new(2), 1),
            (NodeId::new(1), 2),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();
        assert_eq!(
            north_south_port_barycenter_sum(
                &graph,
                &current_node_positions,
                south_input,
                NodeId::new(1),
                7.0
            ),
            Some(5.0)
        );
    }

    #[test]
    fn barycenter_sweep_distributes_free_and_fixed_layer_ports() {
        let mut graph = prepared_graph(
            vec![
                node("Top"),
                node("Bottom"),
                node("Left"),
                node("Right"),
                node("Free"),
                node("Fixed"),
            ],
            vec![
                edge("Top-Free", "Top", "Free"),
                edge("Bottom-Free", "Bottom", "Free"),
                edge("Free-Right", "Free", "Right"),
                edge("Free-Left", "Free", "Left"),
                edge("Fixed-Right", "Fixed", "Right"),
                edge("Fixed-Left", "Fixed", "Left"),
            ],
        );

        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
        let free = graph.node_named("Free");
        let fixed = graph.node_named("Fixed");

        graph.set_layer_nodes(0, vec![top, bottom, fixed]);
        graph.set_layer_nodes(1, vec![free]);
        graph.set_layer_nodes(2, vec![right, left]);
        initialize_crossing_minimization_port_ids(&mut graph);

        let mut graph_info = GraphInfoHolder::new(&graph);
        let port_distributor = BarycenterPortDistributor::new(
            PortDistributorKind::NodeRelative,
            &graph_info.current_node_order,
        );
        let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);

        sweep_reducing_crossings(&mut graph, &mut graph_info, &mut heuristic, true, false);

        let free_ports = graph.layerless_nodes[free]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>();
        let fixed_ports = graph.layerless_nodes[fixed]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(free_ports, vec!["Free:2", "Free:3", "Free:1", "Free:0"]);
        assert_eq!(fixed_ports, vec!["Fixed:0", "Fixed:1"]);
    }

    #[test]
    fn hierarchical_sweep_orders_nested_sources_by_child_boundary_ports() {
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes: vec![
                nested_group("O", None),
                nested_group("A", Some("O")),
                nested_group("B", Some("A")),
                nested_node("b", Some("A")),
                nested_node("a", Some("A")),
                nested_node("c", Some("B")),
            ],
            edges: vec![edge("L_b_B_0", "b", "B"), edge("L_a_c_0", "a", "c")],
        })
        .unwrap();
        graph.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

        crate::compound::preprocess_source_ported_compound_graph(&mut graph);
        crate::configurator::configure_graph_properties(&mut graph).unwrap();

        crate::pipeline::execute_ported_compound_processors(&mut graph).unwrap();

        // The layout is top to bottom, so the order inside a layer is the order along x.
        let o = &graph[graph.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];
        let a = &graph[o.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];
        let x_of = |id: &str| {
            a.layerless_nodes
                .iter()
                .find(|node| node.id == id)
                .unwrap()
                .position
                .x
        };
        assert!(x_of("a") < x_of("b"));
    }

    #[test]
    fn hierarchical_barycenter_keeps_current_order_between_randomized_tries() {
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes: vec![
                nested_group("bar", None),
                nested_group("foo", None),
                nested_node("A", None),
                nested_node("B", None),
                nested_node("G", None),
                nested_node("E", Some("bar")),
                nested_node("F", Some("bar")),
                nested_node("C", Some("foo")),
                nested_node("D", Some("foo")),
            ],
            edges: vec![
                edge("L_A_B_0", "A", "B"),
                edge("L_B_C_0", "B", "C"),
                edge("L_C_D_0", "C", "D"),
                edge("L_B_D_0", "B", "D"),
                edge("L_D_E_0", "D", "E"),
                edge("L_E_A_0", "E", "A"),
                edge("L_E_F_0", "E", "F"),
                edge("L_F_D_0", "F", "D"),
                edge("L_F_G_0", "F", "G"),
                edge("L_B_G_0", "B", "G"),
                edge("L_G_D_0", "G", "D"),
            ],
        })
        .unwrap();
        graph.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

        crate::pipeline::execute_ported_compound_processors(&mut graph).unwrap();

        // Pinned elkjs 0.9.3: the inverted L_D_E port dummy comes before both groups. The old
        // test read the layers after the sweep: [dummy, bar, B] and [dummy, foo]. The layout is
        // top to bottom, so the order inside a layer is the order along x. Assigning SECOND_TRY a
        // distinct Property ID instead gives [bar, B, dummy] and [foo, dummy]. The dummies stay in
        // the node list after the long edge joiner and keep the x their layer gave them.
        let find = |id: &str| {
            graph
                .layerless_nodes
                .iter()
                .find(|node| node.id == id)
                .unwrap()
        };
        let (bar, foo, b) = (find("bar"), find("foo"), find("B"));
        let mut dummies = graph
            .layerless_nodes
            .iter()
            .filter(|node| node.id.starts_with("invertedPort:"))
            .collect::<Vec<_>>();
        assert_eq!(dummies.len(), 2);
        dummies.sort_by(|a, b| a.position.y.total_cmp(&b.position.y));
        let (first_dummy, last_dummy) = (dummies[0], dummies[1]);
        // Layers run along y. A node is in the layer of the group whose vertical extent holds it.
        let spans = |group: &LNode, other: &LNode| {
            group.position.y <= other.position.y
                && other.position.y <= group.position.y + group.size.height
        };
        assert!(spans(bar, first_dummy) && spans(foo, last_dummy) && spans(bar, b));
        assert!(
            first_dummy.position.x < bar.position.x && bar.position.x < b.position.x,
            "first layer along x: dummy {}, bar {}, B {}",
            first_dummy.position.x,
            bar.position.x,
            b.position.x
        );
        assert!(
            last_dummy.position.x < foo.position.x,
            "last layer along x: dummy {}, foo {}",
            last_dummy.position.x,
            foo.position.x
        );
    }

    #[test]
    fn forster_constraints_restore_successor_order_after_barycenter_sort() {
        let mut graph = prepared_graph(
            vec![node("A"), node("B"), node("C")],
            vec![edge("A-C", "A", "C"), edge("B-C", "B", "C")],
        );
        let a = graph.node_named("A");
        let b = graph.node_named("B");
        let c = graph.node_named("C");
        graph.set_layer_nodes(0, vec![c]);
        graph.set_layer_nodes(1, vec![b, a]);
        graph
            .tables
            .in_layer
            .row_mut(a)
            .successor_constraints
            .push(b);

        let mut order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let port_distributor =
            BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
        let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);

        heuristic.minimize_crossings(&graph, &mut order, 1, true, false);

        assert_eq!(order[1], vec![a, b]);
    }

    #[test]
    fn forster_single_node_groups_merge_by_current_barycenters() {
        let mut states = HashMap::new();
        states.insert(
            NodeId::new(0),
            BarycenterState {
                summed_weight: 9.0,
                degree: 3,
                barycenter: Some(3.0),
                visited: false,
            },
        );
        states.insert(
            NodeId::new(1),
            BarycenterState {
                summed_weight: 1.0,
                degree: 1,
                barycenter: Some(1.0),
                visited: false,
            },
        );
        let first = ConstraintGroup::new(NodeId::new(0));
        let second = ConstraintGroup::new(NodeId::new(1));

        let merged = merge_constraint_groups(&mut states, &first, &second);

        assert_eq!(merged.barycenter(&states), Some(2.0));
        assert_eq!(states[&NodeId::new(0)].barycenter, Some(2.0));
        assert_eq!(states[&NodeId::new(1)].barycenter, Some(2.0));
    }

    #[test]
    fn two_sided_greedy_switch_orders_layer_by_both_neighbors() {
        let mut graph = prepared_graph(
            vec![
                node("LeftTop"),
                node("LeftBottom"),
                node("MiddleTop"),
                node("MiddleBottom"),
                node("RightTop"),
                node("RightBottom"),
            ],
            vec![
                edge("LeftTop-MiddleBottom", "LeftTop", "MiddleBottom"),
                edge("LeftBottom-MiddleTop", "LeftBottom", "MiddleTop"),
                edge("MiddleTop-RightBottom", "MiddleTop", "RightBottom"),
                edge("MiddleBottom-RightTop", "MiddleBottom", "RightTop"),
            ],
        );
        let left_top = graph.node_named("LeftTop");
        let left_bottom = graph.node_named("LeftBottom");
        let middle_top = graph.node_named("MiddleTop");
        let middle_bottom = graph.node_named("MiddleBottom");
        let right_top = graph.node_named("RightTop");
        let right_bottom = graph.node_named("RightBottom");
        graph.set_layer_nodes(0, vec![left_top, left_bottom]);
        graph.set_layer_nodes(1, vec![middle_top, middle_bottom]);
        graph.set_layer_nodes(2, vec![right_top, right_bottom]);

        let before = CrossingsCounter::default().count_all_crossings(&graph);
        let mut order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let mut heuristic = GreedySwitchHeuristic::new(CrossMinType::TwoSidedGreedySwitch);

        assert!(heuristic.minimize_crossings(&graph, &mut order, 1, true));
        assert_eq!(order[1], vec![middle_bottom, middle_top]);

        graph.set_layer_nodes(1, order[1].clone());
        let after = CrossingsCounter::default().count_all_crossings(&graph);

        assert_eq!(before, 2.0);
        assert_eq!(after, 0.0);
    }

    #[test]
    fn one_sided_greedy_switch_reduces_between_layer_crossings() {
        let mut graph = prepared_graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Right", "Top", "Right"),
                edge("Bottom-Left", "Bottom", "Left"),
            ],
        );
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
        graph.set_layer_nodes(0, vec![top, bottom]);
        graph.set_layer_nodes(1, vec![left, right]);

        let before = CrossingsCounter::default().count_all_crossings(&graph);
        minimize_crossings_layer_sweep_with_type(
            &mut graph,
            OtherGraphs::none(),
            CrossMinType::OneSidedGreedySwitch,
        );
        let after = CrossingsCounter::default().count_all_crossings(&graph);

        assert_eq!(before, 1.0);
        assert_eq!(after, 0.0);
    }
}

/// Tests of the parts of the greedy switch that deal with north and south port dummies.
#[cfg(test)]
mod north_south_tests {
    use super::*;
    use crate::graph::{LPoint, LayeredEdge};
    use crate::options::{ElkDirection, LayeredOptions};

    fn graph() -> LGraph {
        LGraph::new(
            "g",
            LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
        )
    }

    fn port(node: NodeId, port: usize) -> PortRef {
        PortRef {
            node,
            port: PortId::new(port),
        }
    }

    fn add_edge(graph: &mut LGraph, source: PortRef, target: PortRef) {
        let id = format!("e{}", graph.edges.len());
        graph
            .add_edge(LayeredEdge::new(id, source, target))
            .expect("both ports exist");
    }

    /// A node on `layer` with the given ports, `FIXED_SIDE`.
    fn add_node(graph: &mut LGraph, id: &str, layer: usize, sides: &[PortSide]) -> NodeId {
        let index = graph.next_node_id();
        let mut node = LNode::new(id, 40.0, 20.0, None);
        node.port_constraints = PortConstraints::FixedSide;
        graph.layerless_nodes.push(node);
        graph.set_node_layer(index, layer);
        for side in sides {
            let port_type = if *side == PortSide::East {
                PortType::Output
            } else {
                PortType::Input
            };
            graph.add_port(index, port_type, *side, LPoint::default());
        }
        index
    }

    /// A long edge dummy on the layer, with a neighbour on each side.
    fn add_long_edge_dummy(graph: &mut LGraph, layer: usize) -> NodeId {
        let index = graph.next_node_id();
        let mut dummy = LNode::new("long", 0.0, 0.0, None);
        dummy.set_kind(LNodeKind::LongEdge);
        graph.layerless_nodes.push(dummy);
        graph.set_node_layer(index, layer);
        graph.add_port(index, PortType::Input, PortSide::West, LPoint::default());
        graph.add_port(index, PortType::Output, PortSide::East, LPoint::default());
        index
    }

    fn dummy_of(graph: &LGraph, node: NodeId, port: usize) -> NodeId {
        graph.layerless_nodes[node].ports[PortId::new(port)]
            .port_dummy
            .as_ref()
            .unwrap()
            .node
    }

    /// Two nodes with one north port each, a neighbour in front. Every node gets the given
    /// number of edges for its port. Returns the graph, the nodes and their dummies.
    fn two_nodes_with_north_ports(edges: [usize; 2]) -> (LGraph, [NodeId; 2], [NodeId; 2]) {
        let mut graph = graph();
        let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 5]);
        let a = add_node(&mut graph, "a", 1, &[PortSide::North, PortSide::South]);
        let b = add_node(&mut graph, "b", 1, &[PortSide::North, PortSide::South]);
        let mut next = 0;
        for (node, count) in [a, b].into_iter().zip(edges) {
            for _ in 0..count {
                add_edge(&mut graph, port(neighbour, next), port(node, 0));
                next += 1;
            }
        }
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let dummies = [dummy_of(&graph, a, 0), dummy_of(&graph, b, 0)];
        (graph, [a, b], dummies)
    }

    // Mutation: look for the constraint in the list of the lower node, or ignore the list.
    #[test]
    fn successor_constraints_prevent_a_switch_in_one_direction_only() {
        let (graph, [a, _], [north, _]) = two_nodes_with_north_ports([1, 1]);
        // The northern dummy must stay above its node.
        assert!(have_successor_constraints(&graph, north, a));
        assert!(!have_successor_constraints(&graph, a, north));
    }

    // Mutation: leave out the check of the layout units, which keeps a unit together.
    #[test]
    fn dummies_of_different_nodes_may_not_switch_places() {
        let (graph, [a, b], [north_a, north_b]) = two_nodes_with_north_ports([1, 1]);
        // The southern dummy of the first node stands next to the northern one of the second.
        assert_eq!(graph.layers()[1].nodes().len(), 4);
        assert!(have_layout_unit_constraints(&graph, north_a, north_b));
        assert!(have_layout_unit_constraints(&graph, a, north_b));
        assert!(have_layout_unit_constraints(&graph, a, b));
    }

    // Mutation: compare the layout units as "both have one" instead of "both are the same".
    #[test]
    fn dummies_of_one_node_may_switch_places() {
        let mut graph = graph();
        let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 2]);
        let a = add_node(&mut graph, "a", 1, &[PortSide::North, PortSide::North]);
        add_edge(&mut graph, port(neighbour, 0), port(a, 0));
        add_edge(&mut graph, port(neighbour, 1), port(a, 1));
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let (first, second) = (dummy_of(&graph, a, 0), dummy_of(&graph, a, 1));

        // Neither has an edge on a northern or southern side, and both are in the unit of `a`.
        assert!(!have_layout_unit_constraints(&graph, first, second));
        assert!(!have_layout_unit_constraints(&graph, second, first));
    }

    // Mutation: apply the layout unit rule to long edge dummies.
    #[test]
    fn a_long_edge_dummy_may_switch_with_any_node() {
        let (mut graph, [a, _], [north_a, _]) = two_nodes_with_north_ports([1, 1]);
        let long = add_long_edge_dummy(&mut graph, 1);
        for node in [a, north_a] {
            assert!(!have_layout_unit_constraints(&graph, node, long));
            assert!(!have_layout_unit_constraints(&graph, long, node));
        }
    }

    // Mutation: ask only for the edges of the ports, not for the dummies that hold them.
    #[test]
    fn a_port_with_a_dummy_counts_as_a_port_with_edges() {
        let (graph, [a, _], _) = two_nodes_with_north_ports([1, 1]);
        // The edge of the northern port of `a` lives at the dummy now.
        assert!(
            graph.layerless_nodes[a].ports[PortId::new(0)]
                .incoming_edges()
                .is_empty()
        );
        assert!(has_edges_on_side(&graph, a, PortSide::North));
        // The southern port has no edge and no dummy.
        assert!(!has_edges_on_side(&graph, a, PortSide::South));
    }

    // Mutation: drop the successor or the layout unit rule from `constraints_prevent_switch`.
    #[test]
    fn the_switch_decider_applies_all_three_rules() {
        let (mut graph, [a, b], [north_a, north_b]) = two_nodes_with_north_ports([1, 1]);
        let long = add_long_edge_dummy(&mut graph, 1);
        // Two plain nodes that only a successor constraint ties together, and a plain node next
        // to a dummy that has no layout unit and no constraint.
        let first = add_node(&mut graph, "first", 1, &[]);
        let second = add_node(&mut graph, "second", 1, &[]);
        graph
            .tables
            .in_layer
            .row_mut(first)
            .successor_constraints
            .push(second);
        let plain = add_node(&mut graph, "plain", 1, &[]);
        let bare_dummy = add_node(&mut graph, "bare", 1, &[]);
        graph.layerless_nodes[bare_dummy].set_kind(LNodeKind::NorthSouthPort);
        let order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let filler = CrossingMatrixFiller::new(
            order[1].len(),
            CrossMinType::TwoSidedGreedySwitch,
            CrossingCountSide::West,
        );
        let decider = SwitchDecider::new(&graph, &order, 1, filler, false);
        let layer = order[1].clone();
        let index = |node: NodeId| {
            layer
                .iter()
                .position(|candidate| *candidate == node)
                .unwrap()
        };

        // A dummy and its node (rules 1 and 3), dummies of different nodes (rule 2).
        assert!(decider.constraints_prevent_switch(&graph, &layer, index(north_a), index(a)));
        assert!(decider.constraints_prevent_switch(&graph, &layer, index(a), index(north_a)));
        assert!(decider.constraints_prevent_switch(&graph, &layer, index(north_a), index(north_b)));
        assert!(decider.constraints_prevent_switch(&graph, &layer, index(a), index(b)));
        // Only the successor constraint ties these two, and only in one direction.
        assert!(decider.constraints_prevent_switch(&graph, &layer, index(first), index(second)));
        assert!(!decider.constraints_prevent_switch(&graph, &layer, index(second), index(first)));
        // Only the kinds of these two nodes forbid the switch.
        assert!(decider.constraints_prevent_switch(
            &graph,
            &layer,
            index(plain),
            index(bare_dummy)
        ));
        assert!(decider.constraints_prevent_switch(
            &graph,
            &layer,
            index(bare_dummy),
            index(plain)
        ));
        // A long edge dummy is free.
        assert!(!decider.constraints_prevent_switch(&graph, &layer, index(a), index(long)));
        assert!(!decider.constraints_prevent_switch(&graph, &layer, index(long), index(north_b)));
    }

    fn counter_for(graph: &LGraph) -> NorthSouthEdgeNeighbouringNodeCrossingsCounter {
        NorthSouthEdgeNeighbouringNodeCrossingsCounter::new(graph, graph.layers()[1].nodes())
    }

    /// A node with two northern ports, the first with one edge and the second with two. Returns
    /// the graph, the node and the dummies of the first and second port.
    fn two_north_ports() -> (LGraph, NodeId, NodeId, NodeId) {
        let mut graph = graph();
        let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 3]);
        let a = add_node(&mut graph, "a", 1, &[PortSide::North, PortSide::North]);
        add_edge(&mut graph, port(neighbour, 0), port(a, 0));
        add_edge(&mut graph, port(neighbour, 1), port(a, 1));
        add_edge(&mut graph, port(neighbour, 2), port(a, 1));
        crate::north_south::preprocess_north_south_ports(&mut graph);
        // The sweep numbers every port before it counts, and the counter reads the numbers.
        let ports = graph
            .layerless_nodes
            .iter_mut()
            .flat_map(|node| node.ports.slots_mut());
        for (id, port) in ports.enumerate() {
            port.crossing_minimization_id = Some(id);
        }
        let (first, second) = (dummy_of(&graph, a, 0), dummy_of(&graph, a, 1));
        (graph, a, first, second)
    }

    // Mutation: leave out the count for two dummies of one node, or swap the roles of the dummy
    // further from the node and the one closer to it.
    #[test]
    fn two_dummies_of_one_node_cross_as_often_as_the_edges_of_the_inner_port() {
        let (graph, _, first, second) = two_north_ports();
        let mut counter = counter_for(&graph);

        // The dummy of the left port is further from the node's left corner in this order. The
        // dummy of the right port has two edges.
        counter.count_crossings(&graph, first, second);
        assert_eq!(
            (counter.upper_lower_crossings, counter.lower_upper_crossings),
            (2, 0)
        );
        counter.count_crossings(&graph, second, first);
        assert_eq!(
            (counter.upper_lower_crossings, counter.lower_upper_crossings),
            (0, 2)
        );
    }

    // Mutation: count the crossings of dummies that belong to different nodes.
    #[test]
    fn dummies_of_different_nodes_do_not_cross_each_other() {
        let (graph, _, [north_a, north_b]) = two_nodes_with_north_ports([1, 2]);
        let mut counter = counter_for(&graph);
        counter.count_crossings(&graph, north_a, north_b);
        assert_eq!(
            (counter.upper_lower_crossings, counter.lower_upper_crossings),
            (0, 0)
        );
    }

    // Mutation: swap the northern and southern case, or read the side of the dummy's own port
    // instead of the side of the port it was made for.
    #[test]
    fn a_dummy_crosses_a_long_edge_on_the_side_of_its_port() {
        let mut graph = graph();
        let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 2]);
        let a = add_node(&mut graph, "a", 1, &[PortSide::North, PortSide::South]);
        add_edge(&mut graph, port(neighbour, 0), port(a, 0));
        add_edge(&mut graph, port(neighbour, 1), port(a, 1));
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let (north, south) = (dummy_of(&graph, a, 0), dummy_of(&graph, a, 1));
        let long = add_long_edge_dummy(&mut graph, 1);
        let mut counter = counter_for(&graph);
        let result = |counter: &NorthSouthEdgeNeighbouringNodeCrossingsCounter| {
            (counter.upper_lower_crossings, counter.lower_upper_crossings)
        };

        counter.count_crossings(&graph, north, long);
        assert_eq!(result(&counter), (1, 0));
        counter.count_crossings(&graph, long, north);
        assert_eq!(result(&counter), (0, 1));
        counter.count_crossings(&graph, south, long);
        assert_eq!(result(&counter), (0, 1));
        counter.count_crossings(&graph, long, south);
        assert_eq!(result(&counter), (1, 0));
    }

    // Mutation: count the ports that have edges instead of the ports that have a dummy.
    #[test]
    fn a_node_crosses_a_long_edge_once_for_each_port_with_a_dummy() {
        let mut graph = graph();
        let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 3]);
        let a = add_node(
            &mut graph,
            "a",
            1,
            &[PortSide::North, PortSide::South, PortSide::South],
        );
        add_edge(&mut graph, port(neighbour, 0), port(a, 0));
        add_edge(&mut graph, port(neighbour, 1), port(a, 1));
        add_edge(&mut graph, port(neighbour, 2), port(a, 2));
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let long = add_long_edge_dummy(&mut graph, 1);
        let mut counter = counter_for(&graph);

        // The node above the long edge: the long edge passes the southern ports' dummies.
        counter.count_crossings(&graph, a, long);
        assert_eq!(
            (counter.upper_lower_crossings, counter.lower_upper_crossings),
            (2, 1)
        );
        counter.count_crossings(&graph, long, a);
        assert_eq!(
            (counter.upper_lower_crossings, counter.lower_upper_crossings),
            (1, 2)
        );
    }

    // Mutation: collect the layout units from the layers in reverse order.
    #[test]
    fn the_heuristic_keeps_the_layout_units_in_the_order_of_its_start() {
        let (graph, [a, _], [north_a, _]) = two_nodes_with_north_ports([1, 1]);
        let order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let distributor = BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
        let heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), distributor);
        // The dummy is above the node in the layer, so it comes first.
        assert_eq!(heuristic.layout_units[&a], vec![north_a, a]);
    }
}

/// Tests of `GreedyPortDistributor`.
#[cfg(test)]
mod greedy_port_distributor_tests {
    use super::*;
    use crate::graph::{GraphNodeRef, LPoint, LayeredEdge};
    use crate::options::{ElkDirection, LayeredOptions};

    fn graph(id: &str) -> LGraph {
        LGraph::new(
            id,
            LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
        )
    }

    // Mutation: draw a boolean for the two-sided type too, or draw none for the one-sided type.
    // The shared `Random` then sits one step off for the next connected component.
    #[test]
    fn creating_a_port_distributor_draws_as_isweep_port_distributor_create_does() {
        let order = vec![vec![NodeId::new(0)]];
        let next = |cross_min_type| {
            let mut random = JavaRandom::new(7);
            let distributor = PortDistributor::create(cross_min_type, &mut random, &order);
            (distributor, random.next_long())
        };
        let untouched = JavaRandom::new(7).next_long();
        let mut after_one_draw = JavaRandom::new(7);
        after_one_draw.next_bool();
        let after_one_draw = after_one_draw.next_long();

        let (two_sided, seen) = next(CrossMinType::TwoSidedGreedySwitch);
        assert!(matches!(two_sided, PortDistributor::Greedy(_)));
        assert_eq!(seen, untouched);
        for cross_min_type in [CrossMinType::OneSidedGreedySwitch, CrossMinType::Barycenter] {
            let (distributor, seen) = next(cross_min_type);
            assert!(matches!(distributor, PortDistributor::Barycenter(_)));
            assert_eq!(seen, after_one_draw);
        }
    }

    fn port(node: NodeId, port: usize) -> PortRef {
        PortRef {
            node,
            port: PortId::new(port),
        }
    }

    fn add_edge(graph: &mut LGraph, source: PortRef, target: PortRef) {
        let id = format!("e{}", graph.edges.len());
        graph
            .add_edge(LayeredEdge::new(id, source, target))
            .expect("both ports exist");
    }

    /// A node on `layer` with one port per side given, in the given order of the port list.
    fn add_node(
        graph: &mut LGraph,
        id: &str,
        layer: usize,
        constraints: PortConstraints,
        sides: &[PortSide],
    ) -> NodeId {
        let index = graph.next_node_id();
        let mut node = LNode::new(id, 40.0, 20.0, None);
        node.port_constraints = constraints;
        graph.layerless_nodes.push(node);
        graph.set_node_layer(index, layer);
        for (position, side) in sides.iter().enumerate() {
            let port_type = if matches!(side, PortSide::East) {
                PortType::Output
            } else {
                PortType::Input
            };
            let added = graph
                .add_port(index, port_type, *side, LPoint::default())
                .unwrap();
            graph.layerless_nodes[index].ports[added.port].id = format!("{id}.p{position}");
        }
        index
    }

    fn order(graph: &LGraph) -> Vec<Vec<NodeId>> {
        graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect()
    }

    fn port_ids(graph: &LGraph, node: NodeId) -> Vec<String> {
        graph.layerless_nodes[node]
            .ports
            .iter()
            .map(|port| port.id.clone())
            .collect()
    }

    /// Two nodes `top` and `bottom` in layer 0, one node `a` in layer 1. The ports of `a` are the
    /// west ports `a.p0` and `a.p1`, listed from bottom to top. `top` connects to `a.p0`, so to
    /// the lower port, and `bottom` to `a.p1`: the edges cross.
    fn crossed(constraints: PortConstraints) -> (LGraph, NodeId) {
        let mut graph = graph("g");
        let top = add_node(
            &mut graph,
            "top",
            0,
            PortConstraints::FixedSide,
            &[PortSide::East],
        );
        let bottom = add_node(
            &mut graph,
            "bottom",
            0,
            PortConstraints::FixedSide,
            &[PortSide::East],
        );
        let a = add_node(
            &mut graph,
            "a",
            1,
            constraints,
            &[PortSide::West, PortSide::West],
        );
        add_edge(&mut graph, port(top, 0), port(a, 0));
        add_edge(&mut graph, port(bottom, 0), port(a, 1));
        crate::p3order::initialize_crossing_minimization_port_ids(&mut graph);
        (graph, a)
    }

    // Mutation: never switch, or switch when the crossings would stay the same.
    #[test]
    fn crossed_ports_on_the_side_of_the_sweep_switch_places() {
        let (mut graph, a) = crossed(PortConstraints::FixedSide);
        let order = order(&graph);
        let mut distributor = GreedyPortDistributor::default();

        let improved = distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            1,
            true,
        );

        assert!(improved);
        // The list runs from bottom to top, so the upper edge now ends at the second port.
        assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
        // The ports moved in the list. The edges did not move: each still ends at its port.
        let top_edge = &graph.edges[EdgeId::new(0)];
        let bottom_edge = &graph.edges[EdgeId::new(1)];
        assert_eq!(top_edge.target(), port(a, 0));
        assert_eq!(bottom_edge.target(), port(a, 1));
        assert_eq!(
            graph.layerless_nodes[a].ports[PortId::new(0)].incoming_edges(),
            vec![EdgeId::new(0)]
        );
        assert_eq!(
            graph.layerless_nodes[a].ports.position(PortId::new(0)),
            Some(1)
        );
    }

    // Mutation: switch ports that do not cross.
    #[test]
    fn ports_that_do_not_cross_stay_where_they_are() {
        let (mut graph, a) = crossed(PortConstraints::FixedSide);
        let order = order(&graph);
        let mut distributor = GreedyPortDistributor::default();
        distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            1,
            true,
        );
        assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);

        // A second pass finds nothing to improve.
        let improved = distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            1,
            true,
        );

        assert!(!improved);
        assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
    }

    // Mutation: ignore the port constraints of the node.
    #[test]
    fn a_node_with_a_fixed_port_order_keeps_its_ports() {
        for constraints in [PortConstraints::FixedOrder, PortConstraints::FixedPos] {
            let (mut graph, a) = crossed(constraints);
            let order = order(&graph);
            let mut distributor = GreedyPortDistributor::default();

            let improved = distributor.distribute_ports_while_sweeping(
                &mut graph,
                crate::graph::OtherGraphs::none(),
                &order,
                1,
                true,
            );

            assert!(!improved, "{constraints:?}");
            assert_eq!(port_ids(&graph, a), ["a.p0", "a.p1"], "{constraints:?}");
        }
    }

    // Mutation: distribute the ports of the west side in a backward sweep.
    #[test]
    fn only_the_side_that_faces_the_sweep_is_distributed() {
        let (mut graph, a) = crossed(PortConstraints::FixedSide);
        let order = order(&graph);
        let mut distributor = GreedyPortDistributor::default();

        let improved = distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            1,
            false,
        );

        assert!(!improved);
        assert_eq!(port_ids(&graph, a), ["a.p0", "a.p1"]);
    }

    // Mutation: read the ports of an east side from the other end of the list.
    #[test]
    fn crossed_ports_on_the_east_side_switch_places_in_a_backward_sweep() {
        let mut graph = graph("g");
        let a = add_node(
            &mut graph,
            "a",
            0,
            PortConstraints::FixedSide,
            &[PortSide::East, PortSide::East],
        );
        let top = add_node(
            &mut graph,
            "top",
            1,
            PortConstraints::FixedSide,
            &[PortSide::West],
        );
        let bottom = add_node(
            &mut graph,
            "bottom",
            1,
            PortConstraints::FixedSide,
            &[PortSide::West],
        );
        // The list of the east side runs from top to bottom. `a.p0` is the upper port and goes
        // to the lower node.
        add_edge(&mut graph, port(a, 0), port(bottom, 0));
        add_edge(&mut graph, port(a, 1), port(top, 0));
        crate::p3order::initialize_crossing_minimization_port_ids(&mut graph);
        let order = order(&graph);
        let mut distributor = GreedyPortDistributor::default();

        let improved = distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            0,
            false,
        );

        assert!(improved);
        assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
    }

    // Mutation: switch ports whose counts are equal. The loop would then never end.
    #[test]
    fn ports_without_edges_stay_where_they_are() {
        let mut graph = graph("g");
        let a = add_node(
            &mut graph,
            "a",
            0,
            PortConstraints::FixedSide,
            &[PortSide::West, PortSide::West],
        );
        let b = add_node(
            &mut graph,
            "b",
            1,
            PortConstraints::FixedSide,
            &[PortSide::West],
        );
        crate::p3order::initialize_crossing_minimization_port_ids(&mut graph);
        let order = order(&graph);
        let mut distributor = GreedyPortDistributor::default();

        let improved = distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            0,
            true,
        );

        assert!(!improved);
        assert_eq!(port_ids(&graph, a), ["a.p0", "a.p1"]);
        assert_eq!(port_ids(&graph, b), ["b.p0"]);
    }

    // Mutation: leave the positions of the counter at the slot of a port that moved.
    #[test]
    fn the_positions_of_the_counter_move_with_the_ports() {
        let (mut graph, a) = crossed(PortConstraints::FixedSide);
        let order = order(&graph);
        let mut distributor = GreedyPortDistributor::default();
        distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            1,
            true,
        );

        // The west ports of layer 1 are numbered after the east ports of layer 0, so they get 2
        // and 3. The port `a.p0` had position 2 before the switch. It is the second in the list
        // now, and the switch gave it the position of the other one.
        let counter = &distributor.crossings_counter;
        assert_eq!(graph.layerless_nodes[a].ports[PortId::new(0)].id, "a.p0");
        assert_eq!(counter.position_of(&graph, port(a, 1)), 2);
        assert_eq!(counter.position_of(&graph, port(a, 0)), 3);
    }

    // Mutation: leave the positions of the counter at the slot of a port that moved, so that the
    // next count reads the position of the wrong port.
    #[test]
    fn the_counter_follows_the_ports_that_moved() {
        let (mut graph, a) = crossed(PortConstraints::FixedSide);
        let order = order(&graph);
        let mut distributor = GreedyPortDistributor::default();
        distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            1,
            true,
        );

        // The port that was first (port 0) is second now. In the numbering of the west side of
        // layer 1, a west port further down the list is further up, so the first port of the list
        // (port 1) is the lower one.
        distributor
            .crossings_counter
            .init_for_counting_between(&graph, &order[0], &order[1]);
        let counts = distributor
            .crossings_counter
            .count_crossings_between_ports_in_both_orders(&graph, port(a, 1), port(a, 0));
        // The edges are parallel now. Switching the two ports makes them cross.
        assert_eq!(counts, (0.0, 1.0));
    }

    /// A node `x` in layer 0 whose two east ports cannot move, and a node `a` in layer 1 with
    /// two west ports that can. The edges cross until the ports of `a` switch places.
    fn crossed_between_two_nodes() -> (LGraph, NodeId) {
        let mut graph = graph("g");
        let x = add_node(
            &mut graph,
            "x",
            0,
            PortConstraints::FixedOrder,
            &[PortSide::East, PortSide::East],
        );
        let a = add_node(
            &mut graph,
            "a",
            1,
            PortConstraints::FixedSide,
            &[PortSide::West, PortSide::West],
        );
        // `x.p0` is the upper port of `x`, and `a.p0` is the lower port of `a`.
        add_edge(&mut graph, port(x, 0), port(a, 0));
        add_edge(&mut graph, port(x, 1), port(a, 1));
        (graph, a)
    }

    /// A seed for which the first sweep of a greedy switch runs forward: the minimizer draws one
    /// long, the one-sided type draws a boolean for its port distributor, and then the direction
    /// comes. A backward first sweep would distribute the east ports only, and these cannot move.
    fn seed_for_a_forward_first_sweep(cross_min_type: CrossMinType) -> i64 {
        (0..)
            .find(|seed| {
                let mut random = JavaRandom::new(*seed);
                random.next_long();
                if cross_min_type == CrossMinType::OneSidedGreedySwitch {
                    random.next_bool();
                }
                random.next_bool()
            })
            .unwrap()
    }

    // Mutation: do not call the port distributor in the flat sweep of the two-sided greedy switch.
    #[test]
    fn the_flat_two_sided_sweep_distributes_ports() {
        let (mut graph, a) = crossed_between_two_nodes();
        graph.random = JavaRandom::new(seed_for_a_forward_first_sweep(
            CrossMinType::TwoSidedGreedySwitch,
        ));
        minimize_crossings_layer_sweep_with_type(
            &mut graph,
            OtherGraphs::none(),
            CrossMinType::TwoSidedGreedySwitch,
        );
        assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
    }

    // Mutation: do not call the barycenter port distributor in the flat sweep of the one-sided
    // greedy switch, as the port did before. ELK's `ISweepPortDistributor.create` makes one for
    // every type but the two-sided greedy switch.
    #[test]
    fn the_flat_one_sided_sweep_distributes_ports() {
        let (mut graph, a) = crossed_between_two_nodes();
        graph.random = JavaRandom::new(seed_for_a_forward_first_sweep(
            CrossMinType::OneSidedGreedySwitch,
        ));
        minimize_crossings_layer_sweep_with_type(
            &mut graph,
            OtherGraphs::none(),
            CrossMinType::OneSidedGreedySwitch,
        );
        assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
    }

    // Mutation: do not call the port distributor in the hierarchical sweep of the two-sided
    // greedy switch, or do not call the barycenter one in the hierarchical sweep of the one-sided
    // type.
    #[test]
    fn the_hierarchical_greedy_sweeps_distribute_ports() {
        for cross_min_type in [
            CrossMinType::TwoSidedGreedySwitch,
            CrossMinType::OneSidedGreedySwitch,
        ] {
            let (mut graph, a) = crossed_between_two_nodes();
            graph.random = JavaRandom::new(seed_for_a_forward_first_sweep(cross_min_type));
            let mut graphs = GraphArena::new(graph);
            minimize_crossings_layer_sweep_hierarchical_with_type(&mut graphs, cross_min_type);
            assert_eq!(
                port_ids(graphs.root(), a),
                ["a.p1", "a.p0"],
                "{cross_min_type:?}"
            );
        }
    }

    /// A parent node `p` in layer 0 with two west ports and a nested graph. The nested graph has
    /// the dummies of the ports in layer 0 and two nodes in layer 1, joined so that the edges
    /// cross when the ports stay as they are. The parent has no edge outside.
    fn parent_with_crossing_inside() -> (GraphArena, NodeId) {
        let mut inner = graph("inner");
        let first = inner.next_node_id();
        inner.layerless_nodes.push(LNode::new("d0", 1.0, 1.0, None));
        inner.layerless_nodes[first].set_kind(LNodeKind::ExternalPort);
        inner.set_node_layer(first, 0);
        inner.add_port(first, PortType::Output, PortSide::East, LPoint::default());
        let second = inner.next_node_id();
        inner.layerless_nodes.push(LNode::new("d1", 1.0, 1.0, None));
        inner.layerless_nodes[second].set_kind(LNodeKind::ExternalPort);
        inner.set_node_layer(second, 0);
        inner.add_port(second, PortType::Output, PortSide::East, LPoint::default());
        let c0 = add_node(
            &mut inner,
            "c0",
            1,
            PortConstraints::FixedSide,
            &[PortSide::West],
        );
        let c1 = add_node(
            &mut inner,
            "c1",
            1,
            PortConstraints::FixedSide,
            &[PortSide::West],
        );
        inner.id = GraphId::new(1);
        let inner_id = inner.id;
        // `d1` is below `d0`. `d1` goes to the lower node and `d0` to the upper one.
        add_edge(&mut inner, port(second, 0), port(c1, 0));
        add_edge(&mut inner, port(first, 0), port(c0, 0));
        // The ports of the parent, in the order of the list, stand for d0 and d1.
        let mut outer = graph("outer");
        let parent = add_node(
            &mut outer,
            "p",
            0,
            PortConstraints::FixedSide,
            &[PortSide::West, PortSide::West],
        );
        for (port_index, dummy) in [first, second].into_iter().enumerate() {
            outer.layerless_nodes[parent].ports[PortId::new(port_index)].port_dummy =
                Some(GraphNodeRef {
                    graph_id: inner_id,
                    node: dummy,
                });
        }
        crate::p3order::initialize_crossing_minimization_port_ids(&mut outer);
        let mut graphs = GraphArena::new(outer);
        let nested = graphs.nest(GraphId::ROOT, parent, inner);
        assert_eq!(
            nested, inner_id,
            "the dummies name the graph inside the node"
        );
        (graphs, parent)
    }

    // Mutation: leave out the crossings inside the node.
    #[test]
    fn the_crossings_inside_a_node_with_children_decide_the_order_of_its_ports() {
        let (mut graphs, parent) = parent_with_crossing_inside();
        let order = order(graphs.root());
        let mut distributor = GreedyPortDistributor::default();

        let (graph, others) = graphs.split_mut(GraphId::ROOT);
        let improved = distributor.distribute_ports_while_sweeping(graph, others, &order, 0, true);

        assert!(improved);
        assert_eq!(port_ids(graph, parent), ["p.p1", "p.p0"]);
        // The dummies keep their ports.
        let dummies = graph.layerless_nodes[parent]
            .ports
            .iter()
            .map(|port| port.port_dummy.as_ref().unwrap().node)
            .collect::<Vec<_>>();
        assert_eq!(dummies, vec![NodeId::new(1), NodeId::new(0)]);
    }
}
