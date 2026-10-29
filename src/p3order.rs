//! Phase 3 ordering processors.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/PortSideProcessor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/PortListSorter.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/SortByInputModelProcessor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/preserveorder/ModelOrderNodeComparator.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/preserveorder/ModelOrderPortComparator.java

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::graph::{NodeId, NodeSlice, 
    GraphArena, GraphId, LGraph, LNodeKind, OtherGraphs, PortId, PortRef, PortSide,
};
use crate::options::{OrderingStrategy, PortConstraints, PortSortingStrategy};

pub mod counting;
pub mod sweep;

pub(crate) fn initialize_crossing_minimization_port_ids(graph: &mut LGraph) {
    clear_crossing_minimization_port_ids(graph);
    let order = graph
        .layers
        .iter()
        .map(|layer| layer.nodes.clone())
        .collect::<Vec<_>>();
    assign_crossing_minimization_port_ids(graph, &order);
}

pub(crate) fn initialize_crossing_minimization_port_ids_hierarchy(graphs: &mut GraphArena) {
    // The ids are numbered inside each graph, so the order of the graphs does not matter.
    for graph in graphs.iter_mut() {
        initialize_crossing_minimization_port_ids(graph);
    }
}

fn clear_crossing_minimization_port_ids(graph: &mut LGraph) {
    for node in &mut graph.layerless_nodes {
        for port in node.ports.slots_mut() {
            port.crossing_minimization_id = None;
        }
    }
}

fn assign_crossing_minimization_port_ids(graph: &mut LGraph, order: &[Vec<NodeId>]) {
    let mut next_port_id = 0usize;
    for layer in order {
        for node in layer {
            let Some(node_data) = graph.layerless_nodes.get_node_mut(*node) else {
                continue;
            };
            node_data.ports.for_each_listed_mut(|port| {
                port.crossing_minimization_id = Some(next_port_id);
                next_port_id += 1;
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepCopy {
    pub node_order: Vec<Vec<NodeId>>,
    /// The ports of each node, in its port order, by layer and place in the layer. Java keeps
    /// the `LPort` objects here. A `PortId` is the same identity.
    pub port_orders: Vec<Vec<Vec<PortId>>>,
}

impl SweepCopy {
    pub fn new(graph: &LGraph, node_order: &[Vec<NodeId>]) -> Self {
        Self {
            node_order: node_order.to_vec(),
            port_orders: node_order
                .iter()
                .map(|layer| {
                    layer
                        .iter()
                        .map(|node| graph.layerless_nodes[*node].ports.ids().collect())
                        .collect()
                })
                .collect(),
        }
    }

    pub fn transfer_node_and_port_orders_to_graph(
        &self,
        graph: &mut LGraph,
        set_port_constraints: bool,
    ) -> bool {
        if self.node_order.len() != graph.layers.len() {
            return false;
        }

        for (layer_index, layer_order) in self.node_order.iter().enumerate() {
            if layer_order.len() != graph.layers[layer_index].nodes.len() {
                return false;
            }
        }

        for (layer_index, layer_order) in self.node_order.iter().enumerate() {
            graph.layers[layer_index].nodes = layer_order.clone();
            for (position, node) in layer_order.iter().copied().enumerate() {
                graph.layerless_nodes[node].layer_index = Some(layer_index);
                let Some(port_order) = self
                    .port_orders
                    .get(layer_index)
                    .and_then(|layer| layer.get(position))
                else {
                    return false;
                };
                if !graph.reorder_node_ports(node, port_order.iter().copied()) {
                    return false;
                }
                if set_port_constraints
                    && !graph.layerless_nodes[node]
                        .port_constraints
                        .is_order_fixed()
                {
                    graph.layerless_nodes[node].port_constraints = PortConstraints::FixedOrder;
                }
            }
        }

        correct_north_south_port_sides(graph);

        true
    }
}

/// Ports the end of `SweepCopy.transferNodeAndPortOrdersToGraph` and `assertCorrectPortSides`.
///
/// The option `allowNonFlowPortsToSwitchSides` lets the crossing minimization place the dummy of
/// a port on either side of its node. A dummy below its node means the port is a southern one,
/// and a dummy above means a northern one, so the side of the port changes to match. The ports of
/// each dummy and of each node that has a dummy are sorted again, side by side.
fn correct_north_south_port_sides(graph: &mut LGraph) {
    let mut to_sort: Vec<NodeId> = Vec::new();
    // Which nodes are in `to_sort` already, and where a node sits in the layer being read.
    let mut queued = vec![false; graph.layerless_nodes.len()];
    let mut position_in_layer = vec![usize::MAX; graph.layerless_nodes.len()];
    for layer_index in 0..graph.layers.len() {
        let layer = graph.layers[layer_index].nodes.clone();
        for (position, node) in layer.iter().copied().enumerate() {
            position_in_layer[node.index()] = position;
        }
        for (position, dummy) in layer.iter().copied().enumerate() {
            if graph.layerless_nodes[dummy].kind != LNodeKind::NorthSouthPort {
                continue;
            }
            // The node the dummy was made for is in its layout unit, in the same layer.
            let Some(origin) = graph.layerless_nodes[dummy].in_layer_layout_unit else {
                continue;
            };
            // `position` and `origin_position` are the final places in the layer. Java compares
            // the same numbers: `transferNodeAndPortOrdersToGraph` sets `node.id = j` for every
            // node of the layer first, and only then `assertCorrectPortSides` compares
            // `dummy.id > origin.id`. The ids from before the sweep play no part.
            let origin_position = position_in_layer[origin.index()];
            if origin_position == usize::MAX {
                continue;
            }
            let origin_port = graph.layerless_nodes[dummy]
                .ports
                .first()
                .and_then(|port| port.origin_port);
            if let Some(origin_port) = origin_port.filter(|port| port.node == origin) {
                let port = &mut graph.layerless_nodes[origin].ports[origin_port.port];
                if port.side == PortSide::North && position > origin_position {
                    port.set_side(PortSide::South);
                } else if port.side == PortSide::South && origin_position > position {
                    port.set_side(PortSide::North);
                }
            }
            for node in [origin, dummy] {
                if !queued[node.index()] {
                    queued[node.index()] = true;
                    to_sort.push(node);
                }
            }
        }
        for node in layer {
            position_in_layer[node.index()] = usize::MAX;
        }
    }

    // The side of a port may have changed, so the list is sorted again, as `PortListSorter` sorts
    // a node with a fixed port order. Writing the list back costs one copy of the ids, and a
    // port keeps its id, so a sorted list needs no special case.
    for node in to_sort {
        let constraints = graph.layerless_nodes[node].port_constraints;
        let mut order = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
        order
            .sort_by(|left, right| compare_combined_ports(graph, node, *left, *right, constraints));
        // A sort of the port indices is a permutation, so a refusal is a bug, never caller
        // input. It stays loud in release builds.
        assert!(
            graph.reorder_node_ports(node, order),
            "the sorted port order of node {} is not a permutation of its ports",
            node.index()
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphInfoHolder {
    pub current_node_order: Vec<Vec<NodeId>>,
    pub currently_best_node_and_port_order: Option<SweepCopy>,
    pub best_node_and_port_order: Option<SweepCopy>,
    /// `GraphInfoHolder.crossingsCounter`, whose port positions outlive each count.
    pub crossings_counter: crate::p3order::counting::CrossingsCounter,
}

impl GraphInfoHolder {
    pub fn new(graph: &LGraph) -> Self {
        Self {
            current_node_order: graph
                .layers
                .iter()
                .map(|layer| layer.nodes.clone())
                .collect(),
            currently_best_node_and_port_order: None,
            best_node_and_port_order: None,
            crossings_counter: Default::default(),
        }
    }

    pub fn set_currently_best_node_and_port_order(&mut self, graph: &LGraph) {
        self.currently_best_node_and_port_order =
            Some(SweepCopy::new(graph, &self.current_node_order));
    }

    pub fn set_best_node_and_port_order(&mut self, copy: SweepCopy) {
        self.best_node_and_port_order = Some(copy);
    }

    pub fn get_best_sweep(&self) -> Option<&SweepCopy> {
        self.best_node_and_port_order
            .as_ref()
            .or(self.currently_best_node_and_port_order.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SortableF64(f64);

impl Eq for SortableF64 {}

impl Ord for SortableF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl PartialOrd for SortableF64 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn process_port_sides(graph: &mut LGraph, others: OtherGraphs<'_>) {
    let layerless_nodes = graph.node_ids().collect::<Vec<_>>();
    for node in layerless_nodes {
        process_node_port_sides(graph, others, node);
    }

    let layered_nodes = graph
        .layers
        .iter()
        .flat_map(|layer| layer.nodes.iter().copied())
        .collect::<Vec<_>>();
    for node in layered_nodes {
        process_node_port_sides(graph, others, node);
    }
}

fn process_node_port_sides(graph: &mut LGraph, others: OtherGraphs<'_>, node: NodeId) {
    let side_fixed = graph.layerless_nodes[node].port_constraints.is_side_fixed();

    // `set_port_side` writes a side and never changes the list, so the places stay valid.
    for position in 0..graph.layerless_nodes[node].ports.len() {
        let Some(port) = graph.layerless_nodes[node].ports.id_at(position) else {
            continue;
        };
        if !side_fixed || graph.layerless_nodes[node].ports[port].side == PortSide::Undefined {
            set_port_side(graph, others, PortRef { node, port });
        }
    }

    if !side_fixed {
        graph.layerless_nodes[node].port_constraints = PortConstraints::FixedSide;
    }
}

pub fn set_port_side(graph: &mut LGraph, others: OtherGraphs<'_>, port: PortRef) {
    let port_data = &graph.layerless_nodes[port.node].ports[port.port];
    let side = if let Some(dummy) = port_data.port_dummy.as_ref() {
        external_port_dummy_side(graph, others, port.node, dummy.graph_id, dummy.node)
    } else if port_data.net_flow() < 0 {
        PortSide::East
    } else {
        PortSide::West
    };
    if side != PortSide::Undefined {
        graph.layerless_nodes[port.node].ports[port.port].set_side(side);
    }
}

fn external_port_dummy_side(
    graph: &LGraph,
    others: OtherGraphs<'_>,
    port_node: NodeId,
    dummy_graph_id: GraphId,
    dummy_node: NodeId,
) -> PortSide {
    if dummy_graph_id == graph.id {
        return graph
            .layerless_nodes
            .get_node(dummy_node)
            .map(|node| node.external_port_side)
            .unwrap_or(PortSide::Undefined);
    }

    // The dummy is in the graph inside the port's node, and no other graph.
    graph
        .layerless_nodes
        .get_node(port_node)
        .and_then(|node| node.nested_graph)
        .filter(|&nested| nested == dummy_graph_id)
        .and_then(|nested| others.get(nested))
        .and_then(|nested| nested.layerless_nodes.get_node(dummy_node))
        .map(|node| node.external_port_side)
        .unwrap_or(PortSide::Undefined)
}

pub fn sort_port_lists(graph: &mut LGraph) {
    let node_indices = graph
        .layers
        .iter()
        .flat_map(|layer| layer.nodes.iter().copied())
        .collect::<Vec<_>>();

    for node in node_indices {
        let constraints = graph.layerless_nodes[node].port_constraints;
        if constraints.is_order_fixed() {
            let mut order = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
            order.sort_by(|left, right| {
                compare_combined_ports(graph, node, *left, *right, constraints)
            });
            graph.reorder_node_ports(node, order);
        } else if constraints.is_side_fixed() {
            let keys = graph.layerless_nodes[node]
                .ports
                .ids()
                .map(|port| {
                    (
                        port_side_order(graph.layerless_nodes[node].ports[port].side),
                        port,
                    )
                })
                .collect::<Vec<_>>();
            reorder_by_keys(graph, node, keys);
            reverse_west_and_south_side(graph, node);

            if graph.options.port_sorting_strategy == PortSortingStrategy::PortDegree {
                let keys = graph.layerless_nodes[node]
                    .ports
                    .ids()
                    .enumerate()
                    .map(|(position, port)| {
                        (port_degree_east_west_key(graph, node, port, position), port)
                    })
                    .collect::<Vec<_>>();
                reorder_by_keys(graph, node, keys);
            }
        }
    }
}

pub fn sort_by_input_model(graph: &mut LGraph) {
    let mut layer_index = 0usize;
    while layer_index < graph.layers.len() {
        let previous_layer_index = if layer_index == 0 { 0 } else { layer_index - 1 };
        let previous_layer = graph.layers[previous_layer_index].nodes.clone();
        let layer_nodes = graph.layers[layer_index].nodes.clone();

        for node in layer_nodes {
            let constraints = graph.layerless_nodes[node].port_constraints;
            if constraints != PortConstraints::FixedOrder
                && constraints != PortConstraints::FixedPos
            {
                let target_orders = long_edge_target_node_preprocessing(graph, node);
                let port_order =
                    sorted_ports_by_model_order(graph, node, &previous_layer, &target_orders);
                graph.reorder_node_ports(node, port_order);
            }
        }

        let node_order =
            sorted_nodes_by_model_order(graph, &graph.layers[layer_index].nodes, &previous_layer);
        graph.layers[layer_index].nodes = node_order;
        layer_index += 1;
    }
}

fn reorder_by_keys<K: Ord>(graph: &mut LGraph, node: NodeId, mut keys: Vec<(K, PortId)>) {
    keys.sort_by(|left, right| left.0.cmp(&right.0));
    graph.reorder_node_ports(node, keys.into_iter().map(|(_, port)| port));
}

fn compare_combined_ports(
    graph: &LGraph,
    node: NodeId,
    first: PortId,
    second: PortId,
    constraints: PortConstraints,
) -> Ordering {
    let first_port = &graph.layerless_nodes[node].ports[first];
    let second_port = &graph.layerless_nodes[node].ports[second];
    let side_order = port_side_order(first_port.side).cmp(&port_side_order(second_port.side));
    if side_order != Ordering::Equal || !constraints.is_order_fixed() {
        return side_order;
    }

    if constraints == PortConstraints::FixedOrder
        && let (Some(first_index), Some(second_index)) =
            (first_port.port_index, second_port.port_index)
    {
        let index_order = first_index.cmp(&second_index);
        if index_order != Ordering::Equal {
            return index_order;
        }
    }

    compare_fixed_pos(first_port.side, first_port.position, second_port.position)
}

fn compare_fixed_pos(
    side: PortSide,
    first: crate::graph::LPoint,
    second: crate::graph::LPoint,
) -> Ordering {
    match side {
        PortSide::North => SortableF64(first.x).cmp(&SortableF64(second.x)),
        PortSide::East => SortableF64(first.y).cmp(&SortableF64(second.y)),
        PortSide::South => SortableF64(second.x).cmp(&SortableF64(first.x)),
        PortSide::West => SortableF64(second.y).cmp(&SortableF64(first.y)),
        PortSide::Undefined => Ordering::Equal,
    }
}

fn reverse_west_and_south_side(graph: &mut LGraph, node: NodeId) {
    reverse_side_range(graph, node, PortSide::South);
    reverse_side_range(graph, node, PortSide::West);
}

fn reverse_side_range(graph: &mut LGraph, node: NodeId, side: PortSide) {
    let ports = &graph.layerless_nodes[node].ports;
    if ports.is_empty() {
        return;
    }
    let mut order = ports.ids().collect::<Vec<_>>();

    // Match PortListSorter.java:105-143 at ELK 62d5909f and elkjs 0.9.3.
    // The published provider caps high at len - 1 and rereads low while advancing
    // high. South can temporarily include west ports; the subsequent west pass
    // must inspect the reordered list. A conventional side range changes routes.
    let lower_bound = port_side_order(side);
    let upper_bound = lower_bound + 1;
    let mut current_side = port_side_order(ports[order[0]].side);
    let mut low = 0;
    while low < ports.len() - 1 && current_side < lower_bound {
        low += 1;
        current_side = port_side_order(ports[order[low]].side);
    }
    let mut high = low;
    while high < ports.len() - 1 && current_side < upper_bound {
        high += 1;
        current_side = port_side_order(ports[order[low]].side);
    }
    if high <= low + 2 {
        return;
    }

    order[low..high].reverse();
    graph.reorder_node_ports(node, order);
}

/// The sort key of a port. `position` is the port's place in the node's list: it breaks ties, as
/// the sort is stable.
fn port_degree_east_west_key(
    graph: &LGraph,
    node: NodeId,
    port: PortId,
    position: usize,
) -> (u8, i64, usize) {
    let port_data = &graph.layerless_nodes[node].ports[port];
    let degree_key = match port_data.side {
        PortSide::East => -(real_out_degree(graph, node, port) as i64),
        PortSide::West => real_in_degree(graph, node, port) as i64,
        _ => 0,
    };
    (port_side_order(port_data.side), degree_key, position)
}

fn real_in_degree(graph: &LGraph, node: NodeId, port: PortId) -> usize {
    graph.layerless_nodes[node].ports[port]
        .incoming_edges
        .iter()
        .filter(|edge| !graph.edges[**edge].reversed)
        .count()
}

fn real_out_degree(graph: &LGraph, node: NodeId, port: PortId) -> usize {
    graph.layerless_nodes[node].ports[port]
        .outgoing_edges
        .iter()
        .filter(|edge| !graph.edges[**edge].reversed)
        .count()
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

// elkjs 0.9.3's GWT Arrays sort uses insertion sort below seven elements, then
// alternating merge buffers. Model-order comparators cache relations, including a
// fallback that returns Less for an unseen pair in either direction. Preserve the
// exact comparison schedule for both node and port sorting; Rust's sort_by is not
// interchangeable here. Source: elk-worker.js insertionSort/mergeSort_0/merge_1.
fn sort_model_order<T: Copy>(order: &mut [T], mut compare: impl FnMut(T, T) -> Ordering) {
    fn merge_sort<T: Copy>(
        source: &mut [T],
        destination: &mut [T],
        compare: &mut impl FnMut(T, T) -> Ordering,
    ) {
        if destination.len() < 7 {
            for index in 1..destination.len() {
                let mut current = index;
                while current > 0
                    && compare(destination[current - 1], destination[current]) == Ordering::Greater
                {
                    destination.swap(current - 1, current);
                    current -= 1;
                }
            }
            return;
        }

        let middle = destination.len() / 2;
        let (source_left, source_right) = source.split_at_mut(middle);
        let (destination_left, destination_right) = destination.split_at_mut(middle);
        merge_sort(destination_left, source_left, compare);
        merge_sort(destination_right, source_right, compare);
        if compare(source[middle - 1], source[middle]) != Ordering::Greater {
            destination.copy_from_slice(source);
            return;
        }

        let mut left = 0;
        let mut right = middle;
        for item in destination {
            if right >= source.len()
                || (left < middle && compare(source[left], source[right]) != Ordering::Greater)
            {
                *item = source[left];
                left += 1;
            } else {
                *item = source[right];
                right += 1;
            }
        }
    }

    let mut source = order.to_vec();
    merge_sort(&mut source, order, &mut compare);
}

fn sorted_ports_by_model_order(
    graph: &LGraph,
    node: NodeId,
    previous_layer: &[NodeId],
    target_node_model_order: &HashMap<NodeId, usize>,
) -> Vec<PortId> {
    let mut order = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
    // ELK's comparator records transitive relations while it compares, including equal-order
    // cases. Keep one comparator for this stable sort: key sorting, unstable sorting, or rebuilding
    // the comparator per comparison changes observable Mermaid ordering.
    let mut comparator = ModelOrderPortComparator::new(
        graph,
        node,
        previous_layer,
        graph.options.consider_model_order_strategy,
        target_node_model_order,
        graph.options.consider_model_order_port_model_order,
    );
    sort_model_order(&mut order, |left, right| comparator.compare(left, right));
    order
}

fn sorted_nodes_by_model_order(
    graph: &LGraph,
    nodes: &[NodeId],
    previous_layer: &[NodeId],
) -> Vec<NodeId> {
    let mut order = nodes.to_vec();
    // See sorted_ports_by_model_order: comparison order and shared comparator state are semantic.
    let mut comparator = ModelOrderNodeComparator::new(
        graph,
        previous_layer,
        graph.options.consider_model_order_strategy,
    );
    sort_model_order(&mut order, |left, right| comparator.compare(left, right));
    order
}

pub fn long_edge_target_node_preprocessing(
    graph: &mut LGraph,
    node: NodeId,
) -> HashMap<NodeId, usize> {
    let mut target_node_model_order: HashMap<NodeId, usize> = HashMap::new();

    for port in graph.layerless_nodes[node].ports.slots_mut() {
        port.long_edge_target_node = None;
    }

    for position in 0..graph.layerless_nodes[node].ports.len() {
        let Some(port) = graph.layerless_nodes[node].ports.id_at(position) else {
            continue;
        };
        if graph.layerless_nodes[node].ports[port]
            .outgoing_edges
            .is_empty()
        {
            continue;
        }

        let Some(target_node) = target_node(graph, PortRef { node, port }) else {
            continue;
        };
        graph.layerless_nodes[node].ports[port].long_edge_target_node = Some(target_node);
        let edge_index = graph.layerless_nodes[node].ports[port].outgoing_edges[0];
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

fn cached_long_edge_target_node_orders(graph: &LGraph, node: NodeId) -> HashMap<NodeId, usize> {
    let mut target_node_model_order: HashMap<NodeId, usize> = HashMap::new();
    for port in &graph.layerless_nodes[node].ports {
        let Some(target_node) = port.long_edge_target_node else {
            continue;
        };
        let Some(edge_index) = port.outgoing_edges.first().copied() else {
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

pub fn target_node(graph: &LGraph, port: PortRef) -> Option<NodeId> {
    let mut edge = graph
        .layerless_nodes
        .get_node(port.node)
        .and_then(|node| node.ports.get(port.port))
        .and_then(|port| port.outgoing_edges.first())
        .copied()?;
    let mut remaining_hops = graph.edges.len().saturating_add(1);
    while remaining_hops > 0 {
        remaining_hops -= 1;
        let edge_data = graph.edges.get(edge)?;
        let node = edge_data.target.node;
        if let Some(target) = graph.layerless_nodes[node]
            .long_edge_target
            .map(|port_ref| port_ref.node)
        {
            return Some(target);
        }
        if graph.layerless_nodes[node].kind == LNodeKind::Normal {
            return Some(node);
        }
        edge = graph.layerless_nodes[node]
            .ports
            .iter()
            .find_map(|port| port.outgoing_edges.first().copied())?;
    }
    None
}

pub(super) fn count_model_order_node_changes(
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

pub(super) fn count_model_order_port_changes(graph: &LGraph, layers: &[Vec<NodeId>]) -> usize {
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

struct ModelOrderNodeComparator<'a> {
    graph: &'a LGraph,
    previous_layer: &'a [NodeId],
    ordering_strategy: OrderingStrategy,
    bigger_than: HashMap<NodeId, HashSet<NodeId>>,
    smaller_than: HashMap<NodeId, HashSet<NodeId>>,
}

impl<'a> ModelOrderNodeComparator<'a> {
    fn new(
        graph: &'a LGraph,
        previous_layer: &'a [NodeId],
        ordering_strategy: OrderingStrategy,
    ) -> Self {
        Self {
            graph,
            previous_layer,
            ordering_strategy,
            bigger_than: HashMap::new(),
            smaller_than: HashMap::new(),
        }
    }

    fn compare(&mut self, n1: NodeId, n2: NodeId) -> Ordering {
        if let Some(ordering) = self.cached_ordering(n1, n2) {
            return ordering;
        }

        if self.ordering_strategy == OrderingStrategy::PreferEdges
            || self.graph.layerless_nodes[n1].model_order.is_none()
            || self.graph.layerless_nodes[n2].model_order.is_none()
        {
            let p1_source_port = last_previous_layer_source_port(self.graph, n1);
            let p2_source_port = last_previous_layer_source_port(self.graph, n2);

            if let (Some(p1), Some(p2)) = (p1_source_port, p2_source_port) {
                if p1.node == p2.node {
                    for port_index in self.graph.layerless_nodes[p1.node].ports.ids() {
                        if port_index == p1.port {
                            self.update_bigger_smaller(n2, n1);
                            return Ordering::Less;
                        }
                        if port_index == p2.port {
                            self.update_bigger_smaller(n1, n2);
                            return Ordering::Greater;
                        }
                    }
                }

                for previous_node in self.previous_layer {
                    if *previous_node == p1.node {
                        self.update_bigger_smaller(n2, n1);
                        return Ordering::Less;
                    }
                    if *previous_node == p2.node {
                        self.update_bigger_smaller(n1, n2);
                        return Ordering::Greater;
                    }
                }
            }

            if self.graph.layerless_nodes[n1].model_order.is_none()
                || self.graph.layerless_nodes[n2].model_order.is_none()
            {
                return self.compare_node_model_orders(
                    n1,
                    n2,
                    self.model_order_from_connected_edges(n1),
                    self.model_order_from_connected_edges(n2),
                );
            }
        }

        self.compare_node_model_orders(
            n1,
            n2,
            self.graph.layerless_nodes[n1].model_order.unwrap_or(0) as i64,
            self.graph.layerless_nodes[n2].model_order.unwrap_or(0) as i64,
        )
    }

    fn compare_node_model_orders(
        &mut self,
        n1: NodeId,
        n2: NodeId,
        n1_order: i64,
        n2_order: i64,
    ) -> Ordering {
        if n1_order > n2_order {
            self.update_bigger_smaller(n1, n2);
        } else {
            self.update_bigger_smaller(n2, n1);
        }
        n1_order.cmp(&n2_order)
    }

    fn model_order_from_connected_edges(&self, node: NodeId) -> i64 {
        for port in &self.graph.layerless_nodes[node].ports {
            if let Some(edge) = port.incoming_edges.first() {
                return self.graph.edges[*edge].model_order.unwrap_or(0) as i64;
            }
        }
        self.graph
            .options
            .consider_model_order_long_edge_strategy
            .return_value()
    }

    fn cached_ordering(&mut self, first: NodeId, second: NodeId) -> Option<Ordering> {
        self.ensure_node(first);
        self.ensure_node(second);
        if self.bigger_than[&first].contains(&second) {
            return Some(Ordering::Greater);
        }
        if self.bigger_than[&second].contains(&first) {
            return Some(Ordering::Less);
        }
        if self.smaller_than[&first].contains(&second) {
            return Some(Ordering::Less);
        }
        if self.smaller_than[&second].contains(&first) {
            return Some(Ordering::Greater);
        }
        None
    }

    fn ensure_node(&mut self, node: NodeId) {
        self.bigger_than.entry(node).or_default();
        self.smaller_than.entry(node).or_default();
    }

    fn update_bigger_smaller(&mut self, bigger: NodeId, smaller: NodeId) {
        update_bigger_and_smaller_associations(
            bigger,
            smaller,
            &mut self.bigger_than,
            &mut self.smaller_than,
        );
    }
}

struct ModelOrderPortComparator<'a> {
    graph: &'a LGraph,
    node: NodeId,
    previous_layer: &'a [NodeId],
    strategy: OrderingStrategy,
    target_node_model_order: &'a HashMap<NodeId, usize>,
    port_model_order: bool,
    bigger_than: HashMap<PortId, HashSet<PortId>>,
    smaller_than: HashMap<PortId, HashSet<PortId>>,
}

impl<'a> ModelOrderPortComparator<'a> {
    fn new(
        graph: &'a LGraph,
        node: NodeId,
        previous_layer: &'a [NodeId],
        strategy: OrderingStrategy,
        target_node_model_order: &'a HashMap<NodeId, usize>,
        port_model_order: bool,
    ) -> Self {
        Self {
            graph,
            node,
            previous_layer,
            strategy,
            target_node_model_order,
            port_model_order,
            bigger_than: HashMap::new(),
            smaller_than: HashMap::new(),
        }
    }

    fn compare(&mut self, original_p1: PortId, original_p2: PortId) -> Ordering {
        let mut p1 = original_p1;
        let mut p2 = original_p2;
        let p1_side = self.graph.layerless_nodes[self.node].ports[p1].side;
        let p2_side = self.graph.layerless_nodes[self.node].ports[p2].side;

        if self.port_model_order && p1_side == PortSide::West && p2_side == PortSide::West {
            std::mem::swap(&mut p1, &mut p2);
        }

        if let Some(ordering) = self.cached_ordering(p1, p2) {
            return ordering;
        }

        if self.graph.layerless_nodes[self.node].ports[p1].side
            != self.graph.layerless_nodes[self.node].ports[p2].side
        {
            let result = port_side_order(self.graph.layerless_nodes[self.node].ports[p1].side).cmp(
                &port_side_order(self.graph.layerless_nodes[self.node].ports[p2].side),
            );
            if result == Ordering::Less {
                self.update_bigger_smaller(p2, p1);
            } else {
                self.update_bigger_smaller(p1, p2);
            }
            return result;
        }

        if !self.graph.layerless_nodes[self.node].ports[p1]
            .incoming_edges
            .is_empty()
            && !self.graph.layerless_nodes[self.node].ports[p2]
                .incoming_edges
                .is_empty()
        {
            if self.port_model_order {
                let result = self.check_port_model_order(p1, p2);
                if result != Ordering::Equal {
                    self.update_from_result(p1, p2, result);
                    return result;
                }
            }

            let p1_node = self.incoming_source_node(p1);
            let p2_node = self.incoming_source_node(p2);
            if p1_node == p2_node {
                let p1_order = self.incoming_edge_order(p1);
                let p2_order = self.incoming_edge_order(p2);
                return self.compare_port_orders(p1, p2, p1_order, p2_order);
            }

            for previous_node in self.previous_layer {
                if Some(*previous_node) == p1_node {
                    self.update_bigger_smaller(p1, p2);
                    return Ordering::Greater;
                }
                if Some(*previous_node) == p2_node {
                    self.update_bigger_smaller(p2, p1);
                    return Ordering::Less;
                }
            }
        }

        if !self.graph.layerless_nodes[self.node].ports[p1]
            .outgoing_edges
            .is_empty()
            && !self.graph.layerless_nodes[self.node].ports[p2]
                .outgoing_edges
                .is_empty()
        {
            let p1_target = self.graph.layerless_nodes[self.node].ports[p1].long_edge_target_node;
            let p2_target = self.graph.layerless_nodes[self.node].ports[p2].long_edge_target_node;

            if self.strategy == OrderingStrategy::PreferNodes
                && let (Some(p1_target), Some(p2_target)) = (p1_target, p2_target)
                && let (Some(p1_order), Some(p2_order)) = (
                    self.graph.layerless_nodes[p1_target].model_order,
                    self.graph.layerless_nodes[p2_target].model_order,
                )
            {
                return self.compare_port_orders(p1, p2, p1_order, p2_order);
            }

            if self.port_model_order {
                let result = self.check_port_model_order(p1, p2);
                if result != Ordering::Equal {
                    self.update_from_result(p1, p2, result);
                    return result;
                }
            }

            let mut p1_order = self.outgoing_edge_order(p1);
            let mut p2_order = self.source_outgoing_edge_order_for_second_port(p1, p2);

            if p1_target.is_some() && p1_target == p2_target {
                let p1_reversed = self.outgoing_edge_reversed(p1);
                let p2_reversed = self.outgoing_edge_reversed(p2);
                if p1_reversed && !p2_reversed {
                    self.update_bigger_smaller(p1, p2);
                    return Ordering::Greater;
                }
                if !p1_reversed && p2_reversed {
                    self.update_bigger_smaller(p2, p1);
                    return Ordering::Less;
                }
                return self.compare_port_orders(p1, p2, p1_order, p2_order);
            }

            if let Some(target) = p1_target
                && let Some(order) = self.target_node_model_order.get(&target)
            {
                p1_order = *order;
            }
            if let Some(target) = p2_target
                && let Some(order) = self.target_node_model_order.get(&target)
            {
                p2_order = *order;
            }
            return self.compare_port_orders(p1, p2, p1_order, p2_order);
        }

        let p1_incoming = !self.graph.layerless_nodes[self.node].ports[p1]
            .incoming_edges
            .is_empty();
        let p1_outgoing = !self.graph.layerless_nodes[self.node].ports[p1]
            .outgoing_edges
            .is_empty();
        let p2_incoming = !self.graph.layerless_nodes[self.node].ports[p2]
            .incoming_edges
            .is_empty();
        let p2_outgoing = !self.graph.layerless_nodes[self.node].ports[p2]
            .outgoing_edges
            .is_empty();

        if p1_incoming && p2_outgoing {
            self.update_bigger_smaller(p1, p2);
            Ordering::Greater
        } else if p1_outgoing && p2_incoming {
            self.update_bigger_smaller(p2, p1);
            Ordering::Less
        } else if let (Some(p1_order), Some(p2_order)) = (
            self.graph.layerless_nodes[self.node].ports[p1].model_order,
            self.graph.layerless_nodes[self.node].ports[p2].model_order,
        ) {
            self.compare_port_orders(p1, p2, p1_order, p2_order)
        } else {
            self.update_bigger_smaller(p2, p1);
            Ordering::Less
        }
    }

    fn check_port_model_order(&self, p1: PortId, p2: PortId) -> Ordering {
        match (
            self.graph.layerless_nodes[self.node].ports[p1].model_order,
            self.graph.layerless_nodes[self.node].ports[p2].model_order,
        ) {
            (Some(p1_order), Some(p2_order)) => p1_order.cmp(&p2_order),
            _ => Ordering::Equal,
        }
    }

    fn incoming_source_node(&self, port: PortId) -> Option<NodeId> {
        self.graph.layerless_nodes[self.node].ports[port]
            .incoming_edges
            .first()
            .map(|edge| self.graph.edges[*edge].source.node)
    }

    fn incoming_edge_order(&self, port: PortId) -> usize {
        self.graph.layerless_nodes[self.node].ports[port]
            .incoming_edges
            .first()
            .and_then(|edge| self.graph.edges[*edge].model_order)
            .unwrap_or(0)
    }

    fn outgoing_edge_order(&self, port: PortId) -> usize {
        self.graph.layerless_nodes[self.node].ports[port]
            .outgoing_edges
            .first()
            .and_then(|edge| self.graph.edges[*edge].model_order)
            .unwrap_or(0)
    }

    fn source_outgoing_edge_order_for_second_port(
        &self,
        first_port: PortId,
        second_port: PortId,
    ) -> usize {
        // This asymmetric lookup is source behavior: ELK checks whether p2 has model order, then
        // reads p1's value. It looks like a typo, but correcting it changes stable-sort decisions.
        if self.graph.layerless_nodes[self.node].ports[second_port]
            .outgoing_edges
            .first()
            .and_then(|edge| self.graph.edges[*edge].model_order)
            .is_some()
        {
            self.outgoing_edge_order(first_port)
        } else {
            0
        }
    }

    fn outgoing_edge_reversed(&self, port: PortId) -> bool {
        self.graph.layerless_nodes[self.node].ports[port]
            .outgoing_edges
            .first()
            .map(|edge| self.graph.edges[*edge].reversed)
            .unwrap_or(false)
    }

    fn compare_port_orders(
        &mut self,
        p1: PortId,
        p2: PortId,
        p1_order: usize,
        p2_order: usize,
    ) -> Ordering {
        if p1_order > p2_order {
            self.update_bigger_smaller(p1, p2);
        } else {
            self.update_bigger_smaller(p2, p1);
        }
        p1_order.cmp(&p2_order)
    }

    fn update_from_result(&mut self, p1: PortId, p2: PortId, result: Ordering) {
        if result == Ordering::Less {
            self.update_bigger_smaller(p2, p1);
        } else if result == Ordering::Greater {
            self.update_bigger_smaller(p1, p2);
        }
    }

    fn cached_ordering(&mut self, first: PortId, second: PortId) -> Option<Ordering> {
        self.ensure_port(first);
        self.ensure_port(second);
        if self.bigger_than[&first].contains(&second) {
            return Some(Ordering::Greater);
        }
        if self.bigger_than[&second].contains(&first) {
            return Some(Ordering::Less);
        }
        if self.smaller_than[&first].contains(&second) {
            return Some(Ordering::Less);
        }
        if self.smaller_than[&second].contains(&first) {
            return Some(Ordering::Greater);
        }
        None
    }

    fn ensure_port(&mut self, port: PortId) {
        self.bigger_than.entry(port).or_default();
        self.smaller_than.entry(port).or_default();
    }

    fn update_bigger_smaller(&mut self, bigger: PortId, smaller: PortId) {
        update_bigger_and_smaller_associations(
            bigger,
            smaller,
            &mut self.bigger_than,
            &mut self.smaller_than,
        );
    }
}

fn last_previous_layer_source_port(graph: &LGraph, node: NodeId) -> Option<PortRef> {
    let mut selected = None;
    for port in &graph.layerless_nodes[node].ports {
        if let Some(edge) = port.incoming_edges.first() {
            let source = graph.edges[*edge].source;
            if graph.layerless_nodes[source.node].layer_index
                != graph.layerless_nodes[node].layer_index
            {
                // Upstream deliberately keeps scanning, so the last qualifying incoming port is
                // the comparator's source descriptor. Returning early changes model-order ties.
                selected = Some(source);
            }
        }
    }
    selected
}

fn update_bigger_and_smaller_associations<T>(
    bigger: T,
    smaller: T,
    bigger_than: &mut HashMap<T, HashSet<T>>,
    smaller_than: &mut HashMap<T, HashSet<T>>,
) where
    T: Copy + Eq + std::hash::Hash,
{
    bigger_than.entry(bigger).or_default();
    bigger_than.entry(smaller).or_default();
    smaller_than.entry(bigger).or_default();
    smaller_than.entry(smaller).or_default();

    let smaller_bigger_than = bigger_than[&smaller].clone();
    let bigger_smaller_than = smaller_than[&bigger].clone();

    bigger_than.get_mut(&bigger).unwrap().insert(smaller);
    smaller_than.get_mut(&smaller).unwrap().insert(bigger);

    for very_small in smaller_bigger_than {
        bigger_than.get_mut(&bigger).unwrap().insert(very_small);
        smaller_than.entry(very_small).or_default().insert(bigger);
        let bigger_smaller_than = smaller_than[&bigger].clone();
        smaller_than
            .get_mut(&very_small)
            .unwrap()
            .extend(bigger_smaller_than);
    }

    for very_big in bigger_smaller_than {
        smaller_than.get_mut(&smaller).unwrap().insert(very_big);
        bigger_than.entry(very_big).or_default().insert(smaller);
        let smaller_bigger_than = bigger_than[&smaller].clone();
        bigger_than
            .get_mut(&very_big)
            .unwrap()
            .extend(smaller_bigger_than);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphNodeRef, LNode, LNodeKind, LPoint, LayeredEdge, PortType};
    use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
    use crate::intermediate::split_long_edges;
    use crate::options::{ElkDirection, LayeredOptions};
    use crate::p2layers::layer_network_simplex;

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

    fn graph(nodes: Vec<ElkInputNode>, edges: Vec<ElkInputEdge>) -> crate::graph::GraphArena {
        import_graph(&ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes,
            edges,
        })
        .unwrap()
    }

    fn layered_edge(id: &str, source: PortRef, target: PortRef) -> LayeredEdge {
        LayeredEdge {
            id: id.to_string(),
            source,
            target,
            source_node_id: format!("node-{}", source.node.index()),
            target_node_id: format!("node-{}", target.node.index()),
            labels: Vec::new(),
            minlen: 1,
            reversed: false,
            bend_points: Vec::new(),
            junction_points: Vec::new(),
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
            thickness: 0.0,
            original_opposite_port: None,
            compound_segment: None,
        }
    }

    #[test]
    fn model_order_sort_matches_pinned_worker_comparison_schedule() {
        // Extracted insertionSort/mergeSort_0/merge_1 from elkjs 0.9.3, including
        // the seven-element merge boundary and the already-ordered fast path.
        type SortCase = (
            &'static [usize],
            &'static [usize],
            &'static [(usize, usize)],
        );
        let cases: &[SortCase] = &[
            (&[], &[], &[]),
            (&[0], &[0], &[]),
            (&[2, 1, 0], &[0, 1, 2], &[(2, 1), (2, 0), (1, 0)]),
            (
                &[6, 5, 4, 3, 2, 1, 0],
                &[0, 1, 2, 3, 4, 5, 6],
                &[
                    (6, 5),
                    (6, 4),
                    (5, 4),
                    (3, 2),
                    (3, 1),
                    (2, 1),
                    (3, 0),
                    (2, 0),
                    (1, 0),
                    (6, 0),
                    (4, 0),
                    (4, 1),
                    (4, 2),
                    (4, 3),
                ],
            ),
            (
                &[0, 1, 2, 3, 4, 5, 6],
                &[0, 1, 2, 3, 4, 5, 6],
                &[(0, 1), (1, 2), (3, 4), (4, 5), (5, 6), (2, 3)],
            ),
        ];
        for &(input, expected, expected_calls) in cases {
            let mut order = input.to_vec();
            let mut calls = Vec::new();
            sort_model_order(&mut order, |left, right| {
                calls.push((left, right));
                left.cmp(&right)
            });
            assert_eq!(order, expected, "input={input:?}");
            assert_eq!(calls, expected_calls, "input={input:?}");
        }
    }

    #[test]
    fn model_order_sort_preserves_equal_keys_across_recursive_merges() {
        let keys = [2, 1, 2, 0, 1, 0, 2, 1, 2, 0, 1, 0, 2, 1];
        let mut order = (0..keys.len()).collect::<Vec<_>>();
        sort_model_order(&mut order, |left, right| keys[left].cmp(&keys[right]));
        assert_eq!(order, [3, 5, 9, 11, 1, 4, 7, 10, 13, 0, 2, 6, 8, 12]);
    }

    #[test]
    fn model_order_sort_preserves_source_stateful_port_fallback() {
        for length in [2, 6, 7, 14] {
            let mut graph = LGraph::new("root", LayeredOptions::default());
            graph
                .layerless_nodes
                .push(LNode::new("parent", 0.0, 0.0, None));
            for _ in 0..length {
                graph
                    .add_port(NodeId::new(0), PortType::Input, PortSide::West, LPoint::default())
                    .unwrap();
            }
            let target_orders = HashMap::new();
            let mut comparator = ModelOrderPortComparator::new(
                &graph,
                NodeId::new(0),
                &[],
                OrderingStrategy::NodesAndEdges,
                &target_orders,
                false,
            );
            let mut order = (0..length).map(PortId::new).collect::<Vec<_>>();
            sort_model_order(&mut order, |left, right| comparator.compare(left, right));
            assert_eq!(order, (0..length).map(PortId::new).collect::<Vec<_>>());
            // The first source comparison records port 0 < port 1. Reversing
            // the first call (as Rust sort_by can do) records the opposite.
            assert!(comparator.smaller_than[&PortId::new(0)].contains(&PortId::new(1)));
            assert!(comparator.bigger_than[&PortId::new(1)].contains(&PortId::new(0)));
        }
    }

    // Mutation: add a new port at the front of the node's list, not at its end (`NodePorts::push`
    // with `order.insert(0, id)`). The test reads both places with `unwrap`, because a port off
    // the list has no place and `None` sorts before every place.
    #[test]
    fn compound_self_loop_keeps_input_ports_in_source_model_order() {
        let mut parent = node("Active");
        parent.width = 0.0;
        parent.height = 0.0;
        parent.hierarchy_handling = Some(crate::options::HierarchyHandling::IncludeChildren);
        let mut child = node("Idle");
        child.parent = Some("Active".to_string());
        let mut graph = graph(
            vec![parent, child, node("Inactive")],
            vec![
                edge("edge0", "Inactive", "Idle"),
                edge("edge1", "Active", "Active"),
            ],
        );
        crate::pipeline::execute_ported_compound_processors(&mut graph).unwrap();
        let loop_edge = graph.edges.iter().find(|edge| edge.id == "edge1").unwrap();
        let parent_index = loop_edge.target.node;
        let parent = &graph.layerless_nodes[parent_index];
        let cross_edge = graph
            .edges
            .iter()
            .find(|edge| edge.id == "edge0" && edge.target.node == parent_index)
            .unwrap();
        // The graph flows downward, so the finished layout has turned east into south and west
        // into north.
        assert_eq!(parent.ports[loop_edge.source.port].side, PortSide::South);
        assert_eq!(parent.ports[loop_edge.target.port].side, PortSide::North);
        assert_eq!(parent.ports[cross_edge.target.port].side, PortSide::North);
        // Both ports must be in the list: `None` would sort below any place.
        let loop_place = parent.ports.position(loop_edge.target.port).unwrap();
        let cross_place = parent.ports.position(cross_edge.target.port).unwrap();
        assert!(loop_place < cross_place);
        // Reordering must also keep each child external port's origin linked
        // to its corresponding parent port, including detached self-loop ports.
        let child = &graph[parent.nested_graph.unwrap()];
        for (index, port) in parent.ports.iter_with_ids() {
            let dummy = port.port_dummy.as_ref().unwrap();
            let origin = child.layerless_nodes[dummy.node]
                .origin_port
                .as_ref()
                .unwrap();
            assert_eq!(
                origin.port,
                PortRef {
                    node: parent_index,
                    port: index
                }
            );
        }
    }

    #[test]
    fn port_side_processor_assigns_by_net_flow_and_fixes_constraints() {
        let mut graph = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);

        process_port_sides(&mut graph, crate::graph::OtherGraphs::none());

        let a = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "A")
            .unwrap());
        let b = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "B")
            .unwrap());
        assert_eq!(
            graph.layerless_nodes[a].port_constraints,
            PortConstraints::FixedSide
        );
        assert_eq!(
            graph.layerless_nodes[b].port_constraints,
            PortConstraints::FixedSide
        );
        assert_eq!(
            graph.layerless_nodes[a].ports[PortId::new(0)].side,
            PortSide::East
        );
        assert_eq!(
            graph.layerless_nodes[b].ports[PortId::new(0)].side,
            PortSide::West
        );
    }

    #[test]
    fn port_side_processor_uses_external_port_dummy_side_before_net_flow() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        let parent = graph.next_node_id();
        graph
            .layerless_nodes
            .push(LNode::new("parent", 80.0, 40.0, None));
        let dummy = graph.next_node_id();
        let mut external = LNode::new("external:parent", 0.0, 0.0, None);
        external.kind = LNodeKind::ExternalPort;
        external.external_port_side = PortSide::North;
        graph.layerless_nodes.push(external);
        let port = graph
            .add_port(
                parent,
                PortType::Output,
                PortSide::Undefined,
                LPoint::default(),
            )
            .unwrap();
        graph.layerless_nodes[parent].ports[port.port].port_dummy = Some(GraphNodeRef {
            graph_id: GraphId::ROOT,
            node: dummy,
        });

        set_port_side(&mut graph, crate::graph::OtherGraphs::none(), port);

        assert_eq!(
            graph.layerless_nodes[parent].ports[port.port].side,
            PortSide::North
        );
    }

    #[test]
    fn port_list_sorter_uses_clockwise_order_and_rewrites_edge_refs() {
        let mut graph = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);
        let a = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "A")
            .unwrap());
        graph.layerless_nodes[a].port_constraints = PortConstraints::FixedPos;
        let north = graph
            .add_port(
                a,
                PortType::Output,
                PortSide::North,
                LPoint { x: 10.0, y: 0.0 },
            )
            .unwrap();
        let south = graph
            .add_port(
                a,
                PortType::Output,
                PortSide::South,
                LPoint { x: 1.0, y: 0.0 },
            )
            .unwrap();
        graph.layerless_nodes[a].ports[north.port].id = "north".to_string();
        graph.layerless_nodes[a].ports[south.port].id = "south".to_string();
        graph.layerless_nodes[a].ports[PortId::new(0)].set_side(PortSide::West);
        graph.layerless_nodes[a].ports[PortId::new(0)].position = LPoint { x: 0.0, y: 100.0 };
        graph.set_node_layer(a, 0);

        sort_port_lists(&mut graph);

        assert_eq!(
            graph.layerless_nodes[a]
                .ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>(),
            vec!["north", "south", "A:0"]
        );
        assert_eq!(graph.edges[0].source.node, a);
        assert_eq!(
            graph.layerless_nodes[a].ports[graph.edges[0].source.port].id,
            "A:0"
        );
    }

    // Mutation: reverse the south and west ports one port short of the pinned range.
    #[test]
    fn port_list_sorter_matches_pinned_side_ranges_and_preserves_edge_endpoints() {
        use PortSide::{East, North, South, West};

        // Expected permutations come from the pinned elkjs 0.9.3 PortListSorter,
        // including its exclusive last-port bound and sequential south/west passes.
        let cases: &[(&[PortSide], &[usize])] = &[
            (&[], &[]),
            (&[West], &[0]),
            (&[West, West], &[0, 1]),
            (&[West, West, West], &[0, 1, 2]),
            (&[West, West, West, West], &[2, 1, 0, 3]),
            (&[West, West, West, West, West], &[3, 2, 1, 0, 4]),
            (&[South], &[0]),
            (&[South, South], &[0, 1]),
            (&[South, South, South], &[0, 1, 2]),
            (&[South, South, South, South], &[2, 1, 0, 3]),
            (&[South, South, South, South, South], &[3, 2, 1, 0, 4]),
            (&[North, North, East, East, East], &[0, 1, 2, 3, 4]),
            (
                &[North, East, South, South, South, West],
                &[0, 1, 4, 3, 2, 5],
            ),
            (
                &[North, East, South, South, South, West, West],
                &[0, 1, 2, 3, 4, 5, 6],
            ),
            (
                &[North, North, South, South, South, West, West, West, West],
                &[0, 1, 2, 3, 4, 5, 6, 7, 8],
            ),
        ];
        for &(sides, expected) in cases {
            let mut graph = graph(vec![node("A"), node("B")], vec![]);
            graph.layerless_nodes[0].port_constraints = PortConstraints::FixedSide;
            graph.set_node_layer(NodeId::new(0), 0);
            for (index, side) in sides.iter().copied().enumerate() {
                let outgoing = index % 2 == 0;
                let local = graph
                    .add_port(NodeId::new(0),
                        if outgoing {
                            PortType::Output
                        } else {
                            PortType::Input
                        },
                        side,
                        LPoint { x: 0.0, y: 0.0 },
                    )
                    .unwrap();
                let remote = graph
                    .add_port(NodeId::new(1),
                        if outgoing {
                            PortType::Input
                        } else {
                            PortType::Output
                        },
                        side.opposed(),
                        LPoint { x: 0.0, y: 0.0 },
                    )
                    .unwrap();
                graph.layerless_nodes[0].ports[local.port].id = index.to_string();
                let (source, target) = if outgoing {
                    (local, remote)
                } else {
                    (remote, local)
                };
                graph
                    .add_edge(layered_edge(&format!("edge{index}"), source, target))
                    .unwrap();
            }

            sort_port_lists(&mut graph);

            let actual = graph.layerless_nodes[0]
                .ports
                .iter()
                .map(|port| port.id.parse::<usize>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "source range behavior for {sides:?}");
            for (position, &original) in expected.iter().enumerate() {
                let edge = &graph.edges[original];
                let (local, remote) = if original % 2 == 0 {
                    (edge.source, edge.target)
                } else {
                    (edge.target, edge.source)
                };
                // The sort moved the port in the list. The edge still ends at the same port.
                assert_eq!(
                    local,
                    PortRef {
                        node: NodeId::new(0),
                        port: PortId::new(original)
                    }
                );
                assert_eq!(
                    graph.layerless_nodes[0]
                        .ports
                        .position(PortId::new(original)),
                    Some(position)
                );
                assert_eq!(
                    remote,
                    PortRef {
                        node: NodeId::new(1),
                        port: PortId::new(original)
                    }
                );
                let port = &graph.layerless_nodes[0].ports[PortId::new(original)];
                if original % 2 == 0 {
                    assert_eq!(port.outgoing_edges, vec![original]);
                    assert!(port.incoming_edges.is_empty());
                } else {
                    assert_eq!(port.incoming_edges, vec![original]);
                    assert!(port.outgoing_edges.is_empty());
                }
                assert!(graph.edge_source_attached(original));
                assert!(graph.edge_target_attached(original));
            }
        }
    }

    #[test]
    fn west_range_is_recomputed_after_south_temporarily_crosses_sides() {
        let mut graph = graph(vec![node("A")], vec![]);
        for (index, side) in [
            PortSide::North,
            PortSide::East,
            PortSide::South,
            PortSide::South,
            PortSide::South,
            PortSide::West,
            PortSide::West,
        ]
        .into_iter()
        .enumerate()
        {
            let port = graph
                .add_port(NodeId::new(0), PortType::Input, side, LPoint { x: 0.0, y: 0.0 })
                .unwrap();
            graph.layerless_nodes[0].ports[port.port].id = index.to_string();
        }
        reverse_side_range(&mut graph, NodeId::new(0), PortSide::South);
        let after_south = graph.layerless_nodes[0]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(after_south, vec!["0", "1", "5", "4", "3", "2", "6"]);

        reverse_side_range(&mut graph, NodeId::new(0), PortSide::West);
        let after_west = graph.layerless_nodes[0]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(after_west, vec!["0", "1", "2", "3", "4", "5", "6"]);
    }

    #[test]
    fn fixed_order_ports_fall_back_to_position_without_both_indices() {
        let mut graph = graph(vec![node("A")], vec![]);
        let a = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "A")
            .unwrap());
        graph.layerless_nodes[a].port_constraints = PortConstraints::FixedOrder;
        let left = graph
            .add_port(
                a,
                PortType::Output,
                PortSide::North,
                LPoint { x: 1.0, y: 0.0 },
            )
            .unwrap();
        let right = graph
            .add_port(
                a,
                PortType::Output,
                PortSide::North,
                LPoint { x: 9.0, y: 0.0 },
            )
            .unwrap();
        graph.layerless_nodes[a].ports[left.port].id = "left".to_string();
        graph.layerless_nodes[a].ports[right.port].id = "right".to_string();
        graph.layerless_nodes[a].ports[left.port].port_index = Some(99);
        graph.set_node_layer(a, 0);

        sort_port_lists(&mut graph);

        assert_eq!(
            graph.layerless_nodes[a]
                .ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>(),
            vec!["left", "right"]
        );
    }

    #[test]
    fn sort_by_input_model_orders_layers_by_model_order() {
        let mut graph = graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Right", "Top", "Right"),
                edge("Bottom-Left", "Bottom", "Left"),
            ],
        );
        layer_network_simplex(&mut graph).unwrap();
        process_port_sides(&mut graph, crate::graph::OtherGraphs::none());
        sort_port_lists(&mut graph);

        let left = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Left")
            .unwrap());
        let right = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Right")
            .unwrap());
        let target_layer = graph.layerless_nodes[left].layer_index.unwrap();
        graph.layers[target_layer].nodes = vec![right, left];

        sort_by_input_model(&mut graph);

        assert_eq!(graph.layers[target_layer].nodes, vec![left, right]);
    }

    #[test]
    fn model_order_node_source_descriptor_uses_last_cross_layer_port() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        for index in 0..3 {
            graph.layerless_nodes.push(LNode::new(
                format!("node-{index}"),
                10.0,
                10.0,
                Some(index),
            ));
        }
        graph.set_node_layer(NodeId::new(0), 0);
        graph.set_node_layer(NodeId::new(1), 0);
        graph.set_node_layer(NodeId::new(2), 1);

        let first_source = graph
            .add_port(NodeId::new(0), PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let last_source = graph
            .add_port(NodeId::new(1), PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let first_target = graph
            .add_port(NodeId::new(2), PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        let last_target = graph
            .add_port(NodeId::new(2), PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        graph
            .add_edge(layered_edge("first", first_source, first_target))
            .unwrap();
        graph
            .add_edge(layered_edge("last", last_source, last_target))
            .unwrap();

        assert_eq!(
            last_previous_layer_source_port(&graph, NodeId::new(2)),
            Some(last_source)
        );
    }

    #[test]
    fn sort_by_input_model_matches_source_outgoing_second_port_order() {
        let mut graph = LGraph::new(
            "root",
            LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        );
        graph
            .layerless_nodes
            .push(LNode::new("A", 80.0, 40.0, Some(0)));
        graph
            .layerless_nodes
            .push(LNode::new("dummy-1", 0.0, 0.0, None));
        graph
            .layerless_nodes
            .push(LNode::new("dummy-2", 0.0, 0.0, None));
        graph.layerless_nodes[1].kind = LNodeKind::LongEdge;
        graph.layerless_nodes[2].kind = LNodeKind::LongEdge;
        graph.set_node_layer(NodeId::new(0), 0);
        graph.set_node_layer(NodeId::new(1), 1);
        graph.set_node_layer(NodeId::new(2), 1);
        let first = graph
            .add_port(NodeId::new(0), PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let second = graph
            .add_port(NodeId::new(0), PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let first_target = graph
            .add_port(NodeId::new(1), PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        let second_target = graph
            .add_port(NodeId::new(2), PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        graph
            .add_edge(crate::graph::LayeredEdge {
                id: "A-dummy-1".to_string(),
                source: first,
                target: first_target,
                source_node_id: "A".to_string(),
                target_node_id: "dummy-1".to_string(),
                labels: Vec::new(),
                minlen: 1,
                reversed: false,
                bend_points: Vec::new(),
                junction_points: Vec::new(),
                model_order: Some(1),
                priority_direction: 0,
                priority_shortness: 0,
                priority_straightness: 0,
                thickness: 0.0,
                original_opposite_port: None,
                compound_segment: None,
            })
            .unwrap();
        graph
            .add_edge(crate::graph::LayeredEdge {
                id: "A-dummy-2".to_string(),
                source: second,
                target: second_target,
                source_node_id: "A".to_string(),
                target_node_id: "dummy-2".to_string(),
                labels: Vec::new(),
                minlen: 1,
                reversed: false,
                bend_points: Vec::new(),
                junction_points: Vec::new(),
                model_order: Some(0),
                priority_direction: 0,
                priority_shortness: 0,
                priority_straightness: 0,
                thickness: 0.0,
                original_opposite_port: None,
                compound_segment: None,
            })
            .unwrap();

        assert_eq!(
            graph.layerless_nodes[0]
                .ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>(),
            vec!["A:0", "A:1"]
        );

        sort_by_input_model(&mut graph);

        assert_eq!(
            graph.layerless_nodes[0]
                .ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>(),
            vec!["A:0", "A:1"]
        );
    }

    #[test]
    fn long_edge_target_preprocessing_uses_original_target_node() {
        let mut graph = graph(
            vec![node("A"), node("B"), node("C"), node("D")],
            vec![
                edge("A-B", "A", "B"),
                edge("B-C", "B", "C"),
                edge("C-D", "C", "D"),
                edge("A-D", "A", "D"),
            ],
        );
        layer_network_simplex(&mut graph).unwrap();
        split_long_edges(&mut graph);

        let a = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "A")
            .unwrap());
        let d = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "D")
            .unwrap());
        let orders = long_edge_target_node_preprocessing(&mut graph, a);

        assert_eq!(orders.get(&d), Some(&3));
        assert!(
            graph.layerless_nodes[a]
                .ports
                .iter()
                .any(|port| port.long_edge_target_node == Some(d))
        );
    }

    #[test]
    fn target_node_rejects_cyclic_long_edge_chain() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        for index in 0..3 {
            graph.layerless_nodes.push(LNode::new(
                format!("node-{index}"),
                10.0,
                10.0,
                Some(index),
            ));
        }
        graph.layerless_nodes[1].kind = LNodeKind::LongEdge;
        graph.layerless_nodes[2].kind = LNodeKind::LongEdge;

        let source = graph
            .add_port(NodeId::new(0), PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let first_input = graph
            .add_port(NodeId::new(1), PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        let first_output = graph
            .add_port(NodeId::new(1), PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let cycle_input = graph
            .add_port(NodeId::new(1), PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        let second_input = graph
            .add_port(NodeId::new(2), PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        let second_output = graph
            .add_port(NodeId::new(2), PortType::Output, PortSide::East, LPoint::default())
            .unwrap();

        graph
            .add_edge(layered_edge("source", source, first_input))
            .unwrap();
        graph
            .add_edge(layered_edge("forward", first_output, second_input))
            .unwrap();
        graph
            .add_edge(layered_edge("cycle", second_output, cycle_input))
            .unwrap();

        assert_eq!(target_node(&graph, source), None);
    }

    // The two ports share one id string, so only the `PortId` tells them apart.
    //
    // Mutation: leave the ports in the order the sweep put them, by skipping the
    // `reorder_node_ports` call in `SweepCopy::transfer_node_and_port_orders_to_graph`.
    #[test]
    fn sweep_copy_restores_the_port_order_by_port_id_when_id_strings_repeat() {
        let mut graph = graph(vec![node("A")], vec![]);
        let a = NodeId::new(graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "A")
            .unwrap());
        graph.layerless_nodes[a].ports.clear();
        let first = graph
            .add_port(
                a,
                PortType::Output,
                PortSide::East,
                LPoint { x: 0.0, y: 0.0 },
            )
            .unwrap();
        let second = graph
            .add_port(
                a,
                PortType::Output,
                PortSide::East,
                LPoint { x: 0.0, y: 0.0 },
            )
            .unwrap();
        graph.layerless_nodes[a].ports[first.port].id = "shared".to_string();
        graph.layerless_nodes[a].ports[first.port].crossing_minimization_id = Some(7);
        graph.layerless_nodes[a].ports[second.port].id = "shared".to_string();
        graph.layerless_nodes[a].ports[second.port].crossing_minimization_id = Some(8);
        graph.set_node_layer(a, 0);

        let copy = SweepCopy::new(&graph, &[vec![a]]);
        graph.reorder_node_ports(a, crate::graph::port_ids([1, 0]));

        assert!(copy.transfer_node_and_port_orders_to_graph(&mut graph, false));
        assert_eq!(
            graph.layerless_nodes[a]
                .ports
                .iter()
                .map(|port| port.crossing_minimization_id)
                .collect::<Vec<_>>(),
            vec![Some(7), Some(8)]
        );
    }

    // --- Sides of ports whose dummies the minimization placed. ---

    /// A node `a` in layer 1 with a north port and a south port that may switch sides, a
    /// neighbour in layer 0 and the dummies the pre-processor makes. Returns the graph, the node
    /// and the dummies of the north and of the south port.
    fn node_with_movable_ports() -> (LGraph, NodeId, NodeId, NodeId) {
        let mut graph = LGraph::new(
            "g",
            LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
        );
        let neighbour = graph.next_node_id();
        graph
            .layerless_nodes
            .push(LNode::new("n", 10.0, 10.0, None));
        graph.set_node_layer(neighbour, 0);
        for _ in 0..2 {
            graph.add_port(
                neighbour,
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            );
        }
        let a = graph.next_node_id();
        let mut node = LNode::new("a", 40.0, 20.0, None);
        node.port_constraints = PortConstraints::FixedSide;
        graph.layerless_nodes.push(node);
        graph.set_node_layer(a, 1);
        for (index, side) in [PortSide::North, PortSide::South].into_iter().enumerate() {
            graph.add_port(a, PortType::Input, side, LPoint::default());
            graph.layerless_nodes[a].ports[PortId::new(index)]
                .allow_non_flow_ports_to_switch_sides = true;
            graph.add_edge(layered_edge(
                &format!("e{index}"),
                PortRef {
                    node: neighbour,
                    port: PortId::new(index),
                },
                PortRef {
                    node: a,
                    port: PortId::new(index),
                },
            ));
        }
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let dummy = |port: usize| {
            graph.layerless_nodes[a].ports[PortId::new(port)]
                .port_dummy
                .as_ref()
                .unwrap()
                .node
        };
        let (north, south) = (dummy(0), dummy(1));
        (graph, a, north, south)
    }

    fn side_of(graph: &LGraph, node: NodeId, port_id: usize) -> PortSide {
        graph.layerless_nodes[node].ports[PortId::new(port_id)].side
    }

    // Mutation: leave the sides of the ports alone.
    #[test]
    fn a_dummy_on_the_other_side_of_its_node_switches_the_side_of_its_port() {
        let (mut graph, a, north, south) = node_with_movable_ports();
        // The dummies trade places around the node: the south dummy first, the north one last.
        graph.layers[1].nodes = vec![south, a, north];

        correct_north_south_port_sides(&mut graph);

        let sides = graph.layerless_nodes[a]
            .ports
            .iter()
            .map(|port| (port.id.clone(), port.side))
            .collect::<Vec<_>>();
        // The port that was north (port 0) is south now, and the other way round. The list is
        // sorted by side again: north before south.
        assert_eq!(side_of(&graph, a, 0), PortSide::South);
        assert_eq!(side_of(&graph, a, 1), PortSide::North);
        assert_eq!(sides[0].0, "a:1");
        assert_eq!(sides[1].0, "a:0");
    }

    // Mutation: leave the correction out of the transfer of the best sweep to the graph.
    #[test]
    fn the_transfer_of_a_sweep_corrects_the_sides_of_ports_that_switched() {
        let (mut graph, a, north, south) = node_with_movable_ports();
        initialize_crossing_minimization_port_ids(&mut graph);
        let mut order = graph
            .layers
            .iter()
            .map(|layer| layer.nodes.clone())
            .collect::<Vec<_>>();
        order[1] = vec![south, a, north];
        let sweep = SweepCopy::new(&graph, &order);

        assert!(sweep.transfer_node_and_port_orders_to_graph(&mut graph, true));

        assert_eq!(graph.layers[1].nodes, vec![south, a, north]);
        // The northern port is below its dummy and became the southern one: it is last now.
        assert_eq!(graph.layerless_nodes[a].ports.first().unwrap().id, "a:1");
        assert_eq!(side_of(&graph, a, 0), PortSide::South);
        assert_eq!(side_of(&graph, a, 1), PortSide::North);
        assert_eq!(
            graph.layerless_nodes[a].port_constraints,
            PortConstraints::FixedOrder
        );
    }

    // Mutation: switch the sides although the dummies are where the sides say.
    #[test]
    fn dummies_on_the_expected_side_leave_the_sides_of_the_ports_alone() {
        let (mut graph, a, north, south) = node_with_movable_ports();
        assert_eq!(graph.layers[1].nodes, vec![north, a, south]);

        correct_north_south_port_sides(&mut graph);

        assert_eq!(side_of(&graph, a, 0), PortSide::North);
        assert_eq!(side_of(&graph, a, 1), PortSide::South);
        assert_eq!(graph.layerless_nodes[a].ports[PortId::new(0)].id, "a:0");
    }

    // Mutation: switch the side of the south port only (or of the north port only).
    #[test]
    fn a_north_and_a_south_port_switch_independently() {
        let (mut graph, a, north, south) = node_with_movable_ports();
        // Both dummies above the node: the south port becomes a north port.
        graph.layers[1].nodes = vec![north, south, a];

        correct_north_south_port_sides(&mut graph);

        assert_eq!(side_of(&graph, a, 0), PortSide::North);
        assert_eq!(side_of(&graph, a, 1), PortSide::North);

        let (mut graph, a, north, south) = node_with_movable_ports();
        // Both below: the north port becomes a south port.
        graph.layers[1].nodes = vec![a, north, south];

        correct_north_south_port_sides(&mut graph);

        assert_eq!(side_of(&graph, a, 0), PortSide::South);
        assert_eq!(side_of(&graph, a, 1), PortSide::South);
    }

    // Mutation: leave the ports of the dummies in the order they were made.
    #[test]
    fn the_ports_of_a_dummy_are_sorted_by_side_again() {
        let mut graph = LGraph::new(
            "g",
            LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
        );
        let neighbour = graph.next_node_id();
        graph
            .layerless_nodes
            .push(LNode::new("n", 10.0, 10.0, None));
        graph.set_node_layer(neighbour, 0);
        graph.add_port(
            neighbour,
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        );
        graph.add_port(
            neighbour,
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        );
        let a = graph.next_node_id();
        let mut node = LNode::new("a", 40.0, 20.0, None);
        node.port_constraints = PortConstraints::FixedSide;
        graph.layerless_nodes.push(node);
        graph.set_node_layer(a, 1);
        graph.add_port(a, PortType::Input, PortSide::North, LPoint::default());
        graph.add_edge(layered_edge(
            "in",
            PortRef {
                node: neighbour,
                port: PortId::new(0),
            },
            PortRef {
                node: a,
                port: PortId::new(0),
            },
        ));
        graph.add_edge(layered_edge(
            "out",
            PortRef {
                node: a,
                port: PortId::new(0),
            },
            PortRef {
                node: neighbour,
                port: PortId::new(1),
            },
        ));
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let dummy = graph.layerless_nodes[a].ports[PortId::new(0)]
            .port_dummy
            .as_ref()
            .unwrap()
            .node;
        // The dummy has a west port first and an east port second.
        let before = graph.layerless_nodes[dummy]
            .ports
            .iter()
            .map(|port| port.side)
            .collect::<Vec<_>>();
        assert_eq!(before, [PortSide::West, PortSide::East]);

        correct_north_south_port_sides(&mut graph);

        // East comes before west, as in `PortListSorter`. The edges stayed at their ports.
        let after = graph.layerless_nodes[dummy]
            .ports
            .iter()
            .map(|port| {
                (
                    port.side,
                    port.incoming_edges.len(),
                    port.outgoing_edges.len(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(after, [(PortSide::East, 0, 1), (PortSide::West, 1, 0)]);
        // The edges name the same ports as before: the west port is port 0, the east port 1.
        assert_eq!(
            graph.edges[0].target,
            PortRef {
                node: dummy,
                port: PortId::new(0)
            }
        );
        assert_eq!(
            graph.edges[1].source,
            PortRef {
                node: dummy,
                port: PortId::new(1)
            }
        );
    }
}
