//! Port of ELK's `PortListSorter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/PortListSorter.java

use std::cmp::Ordering;

use crate::graph::{LGraph, NodeId, PortId, PortSide};
use crate::options::{PortConstraints, PortSortingStrategy};

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

pub fn sort_port_lists(graph: &mut LGraph) {
    let node_indices = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
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

fn reorder_by_keys<K: Ord>(graph: &mut LGraph, node: NodeId, mut keys: Vec<(K, PortId)>) {
    keys.sort_by(|left, right| left.0.cmp(&right.0));
    graph.reorder_node_ports(node, keys.into_iter().map(|(_, port)| port));
}

pub(crate) fn compare_combined_ports(
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

pub(crate) fn reverse_side_range(graph: &mut LGraph, node: NodeId, side: PortSide) {
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
        .incoming_edges()
        .iter()
        .filter(|edge| !graph.edges[**edge].reversed)
        .count()
}

fn real_out_degree(graph: &LGraph, node: NodeId, port: PortId) -> usize {
    graph.layerless_nodes[node].ports[port]
        .outgoing_edges()
        .iter()
        .filter(|edge| !graph.edges[**edge].reversed)
        .count()
}

pub(crate) fn port_side_order(side: PortSide) -> u8 {
    match side {
        PortSide::Undefined => 0,
        PortSide::North => 1,
        PortSide::East => 2,
        PortSide::South => 3,
        PortSide::West => 4,
    }
}
