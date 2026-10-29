//! Port of ELK's `SortByInputModelProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/SortByInputModelProcessor.java

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::graph::{LGraph, LNodeKind, NodeId, PortId, PortRef};
use crate::intermediate::preserveorder::model_order_node_comparator::ModelOrderNodeComparator;
use crate::intermediate::preserveorder::model_order_port_comparator::ModelOrderPortComparator;
use crate::options::PortConstraints;

pub fn sort_by_input_model(graph: &mut LGraph) {
    let mut layer_index = 0usize;
    while layer_index < graph.layers().len() {
        let previous_layer_index = if layer_index == 0 { 0 } else { layer_index - 1 };
        let previous_layer = graph.layers()[previous_layer_index].nodes().to_vec();
        let layer_nodes = graph.layers()[layer_index].nodes().to_vec();

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

        let node_order = sorted_nodes_by_model_order(
            graph,
            graph.layers()[layer_index].nodes(),
            &previous_layer,
        );
        graph.set_layer_nodes(layer_index, node_order);
        layer_index += 1;
    }
}

// elkjs 0.9.3's GWT Arrays sort uses insertion sort below seven elements, then
// alternating merge buffers. Model-order comparators cache relations, including a
// fallback that returns Less for an unseen pair in either direction. Preserve the
// exact comparison schedule for both node and port sorting; Rust's sort_by is not
// interchangeable here. Source: elk-worker.js insertionSort/mergeSort_0/merge_1.
pub(crate) fn sort_model_order<T: Copy>(
    order: &mut [T],
    mut compare: impl FnMut(T, T) -> Ordering,
) {
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
            .outgoing_edges()
            .is_empty()
        {
            continue;
        }

        let Some(target_node) = target_node(graph, PortRef { node, port }) else {
            continue;
        };
        graph.layerless_nodes[node].ports[port].long_edge_target_node = Some(target_node);
        let edge_index = graph.layerless_nodes[node].ports[port].outgoing_edges()[0];
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
        .get(port.node)
        .and_then(|node| node.ports.get(port.port))
        .and_then(|port| port.outgoing_edges().first())
        .copied()?;
    let mut remaining_hops = graph.edges.len().saturating_add(1);
    while remaining_hops > 0 {
        remaining_hops -= 1;
        let edge_data = graph.edges.get(edge)?;
        let node = edge_data.target().node;
        if let Some(target) = graph.layerless_nodes[node]
            .long_edge_ends()
            .and_then(|ends| ends.target)
            .map(|port_ref| port_ref.node)
        {
            return Some(target);
        }
        if graph.layerless_nodes[node].kind() == LNodeKind::Normal {
            return Some(node);
        }
        edge = graph.layerless_nodes[node]
            .ports
            .iter()
            .find_map(|port| port.outgoing_edges().first().copied())?;
    }
    None
}
