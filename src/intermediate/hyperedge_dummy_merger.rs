//! Port of ELK's `HyperedgeDummyMerger`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HyperedgeDummyMerger.java

use std::collections::HashMap;

use crate::graph::{IdVec, LGraph, LNode, LNodeKind, NodeId, PortRef, PortSide};

pub fn merge_hyperedge_dummies(graph: &mut LGraph) {
    let hyperedge_ids = identify_hyperedges(graph);

    for layer_index in 0..graph.layers().len() {
        let mut node_position = 1usize;
        while node_position < graph.layers()[layer_index].nodes().len() {
            let current = graph.layers()[layer_index].nodes()[node_position];
            let previous = graph.layers()[layer_index].nodes()[node_position - 1];

            if graph.layerless_nodes[current].kind() == LNodeKind::LongEdge
                && graph.layerless_nodes[previous].kind() == LNodeKind::LongEdge
            {
                let state = check_hyperedge_merge_allowed(graph, current, previous, &hyperedge_ids);
                if state.allow_merge {
                    merge_hyperedge_dummy_nodes(
                        graph,
                        current,
                        previous,
                        state.same_source,
                        state.same_target,
                    );
                    graph.remove_node_at(layer_index, node_position);
                    continue;
                }
            }

            node_position += 1;
        }
    }
}

#[derive(Clone, Copy)]
struct HyperedgeMergeState {
    allow_merge: bool,
    same_source: bool,
    same_target: bool,
}

/// Whether the node is a long edge dummy of an edge that has label dummies. Any other node is not.
pub(crate) fn has_label_dummies(node: &LNode) -> bool {
    node.long_edge()
        .is_some_and(|long_edge| long_edge.has_label_dummies)
}

fn check_hyperedge_merge_allowed(
    graph: &LGraph,
    current: NodeId,
    previous: NodeId,
    hyperedge_ids: &HashMap<PortRef, i32>,
) -> HyperedgeMergeState {
    let current_node = &graph.layerless_nodes[current];
    let previous_node = &graph.layerless_nodes[previous];
    let current_ends = current_node.long_edge_ends().unwrap_or_default();
    let previous_ends = previous_node.long_edge_ends().unwrap_or_default();
    let same_source = current_ends.source.is_some() && current_ends.source == previous_ends.source;
    let same_target = current_ends.target.is_some() && current_ends.target == previous_ends.target;
    let current_has_label_dummies = has_label_dummies(current_node);
    let previous_has_label_dummies = has_label_dummies(previous_node);

    if !current_has_label_dummies && !previous_has_label_dummies {
        let current_first_port = current_node.ports.id_at(0).map(|port| PortRef {
            node: current,
            port,
        });
        let previous_first_port = previous_node.ports.id_at(0).map(|port| PortRef {
            node: previous,
            port,
        });
        let allow_merge = match (current_first_port, previous_first_port) {
            (Some(current_port), Some(previous_port)) => {
                let current_id = hyperedge_ids.get(&current_port);
                let previous_id = hyperedge_ids.get(&previous_port);
                current_id.is_some() && current_id == previous_id
            }
            _ => false,
        };
        return HyperedgeMergeState {
            allow_merge,
            same_source,
            same_target,
        };
    }

    let current_before_label = is_before_label_dummy(graph, current);
    let previous_before_label = is_before_label_dummy(graph, previous);
    let eligible_for_source_merging = (!current_has_label_dummies || current_before_label)
        && (!previous_has_label_dummies || previous_before_label);
    let eligible_for_target_merging = (!current_has_label_dummies || !current_before_label)
        && (!previous_has_label_dummies || !previous_before_label);

    HyperedgeMergeState {
        allow_merge: (same_source && eligible_for_source_merging)
            || (same_target && eligible_for_target_merging),
        same_source,
        same_target,
    }
}

fn merge_hyperedge_dummy_nodes(
    graph: &mut LGraph,
    merge_source: NodeId,
    merge_target: NodeId,
    keep_source_port: bool,
    keep_target_port: bool,
) {
    let Some(merge_target_input_port) = find_port_by_side(graph, merge_target, PortSide::West)
    else {
        return;
    };
    let Some(merge_target_output_port) = find_port_by_side(graph, merge_target, PortSide::East)
    else {
        return;
    };

    // Moving an edge to another port never changes the source's list.
    for position in 0..graph.layerless_nodes[merge_source].ports.len() {
        let Some(port_index) = graph.layerless_nodes[merge_source].ports.id_at(position) else {
            continue;
        };
        while let Some(edge_index) = graph.layerless_nodes[merge_source].ports[port_index]
            .incoming_edges()
            .first()
            .copied()
        {
            if !graph.set_edge_target(edge_index, merge_target_input_port) {
                break;
            }
        }

        while let Some(edge_index) = graph.layerless_nodes[merge_source].ports[port_index]
            .outgoing_edges()
            .first()
            .copied()
        {
            if !graph.set_edge_source(edge_index, merge_target_output_port) {
                break;
            }
        }
    }

    if !keep_source_port
        && let Some(ends) = graph.layerless_nodes[merge_target].long_edge_ends_mut()
    {
        ends.source = None;
    }
    if !keep_target_port
        && let Some(ends) = graph.layerless_nodes[merge_target].long_edge_ends_mut()
    {
        ends.target = None;
    }
}

fn identify_hyperedges(graph: &LGraph) -> HashMap<PortRef, i32> {
    let ports = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .flat_map(|node| {
            graph.layerless_nodes[node]
                .ports
                .ids()
                .map(move |port| PortRef { node, port })
        })
        .collect::<Vec<_>>();
    let mut component_ids = HashMap::new();
    let mut next_id = 0i32;

    for port in ports {
        if component_ids.contains_key(&port) {
            continue;
        }
        mark_hyperedge_component(graph, port, next_id, &mut component_ids);
        next_id += 1;
    }

    component_ids
}

fn mark_hyperedge_component(
    graph: &LGraph,
    start: PortRef,
    component_id: i32,
    component_ids: &mut HashMap<PortRef, i32>,
) {
    let mut stack = vec![start];
    while let Some(port_ref) = stack.pop() {
        if !port_exists_in_graph(graph, port_ref) || component_ids.contains_key(&port_ref) {
            continue;
        }
        component_ids.insert(port_ref, component_id);

        for connected in connected_ports(graph, port_ref) {
            if !component_ids.contains_key(&connected) {
                stack.push(connected);
            }
        }

        if graph.layerless_nodes[port_ref.node].kind() == LNodeKind::LongEdge {
            for sibling_port in graph.layerless_nodes[port_ref.node].ports.ids() {
                let sibling = PortRef {
                    node: port_ref.node,
                    port: sibling_port,
                };
                if sibling != port_ref && !component_ids.contains_key(&sibling) {
                    stack.push(sibling);
                }
            }
        }
    }
}

pub(crate) fn connected_ports(graph: &LGraph, port_ref: PortRef) -> Vec<PortRef> {
    let Some(port) = graph
        .layerless_nodes
        .get(port_ref.node)
        .and_then(|node| node.ports.get(port_ref.port))
    else {
        return Vec::new();
    };

    port.outgoing_edges()
        .iter()
        .filter_map(|edge| graph.edges.get(*edge).map(|edge| edge.target()))
        .chain(
            port.incoming_edges()
                .iter()
                .filter_map(|edge| graph.edges.get(*edge).map(|edge| edge.source())),
        )
        .collect()
}

pub(crate) fn is_before_label_dummy(graph: &LGraph, node: NodeId) -> bool {
    let mut current = node;
    let mut visited = IdVec::filled(graph.layerless_nodes.len(), false);

    loop {
        if visited.get(current).copied().unwrap_or(true) {
            return false;
        }
        visited[current] = true;

        let Some(outgoing) = graph.node_outgoing_edges(current).into_iter().next() else {
            return false;
        };
        let target = graph.edges[outgoing].target().node;
        match graph.layerless_nodes[target].kind() {
            LNodeKind::Label => return true,
            LNodeKind::LongEdge => current = target,
            _ => return false,
        }
    }
}

fn find_port_by_side(graph: &LGraph, node: NodeId, side: PortSide) -> Option<PortRef> {
    graph
        .layerless_nodes
        .get(node)?
        .ports
        .iter_with_ids()
        .find(|(_, port)| port.side == side)
        .map(|(port, _)| PortRef { node, port })
}

fn port_exists_in_graph(graph: &LGraph, port_ref: PortRef) -> bool {
    graph
        .layerless_nodes
        .get(port_ref.node)
        .and_then(|node| node.ports.get(port_ref.port))
        .is_some()
}
