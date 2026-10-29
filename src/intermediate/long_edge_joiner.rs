//! Port of ELK's `LongEdgeJoiner`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LongEdgeJoiner.java

use std::collections::{HashMap, HashSet};

use crate::graph::{EdgeId, LGraph, LNodeKind, LPoint, NodeId, PortId, PortRef, PortSide};

fn index_all_incoming_edges(graph: &LGraph) -> HashMap<EdgeId, (PortRef, usize)> {
    let mut positions = HashMap::with_capacity(graph.edges.len());
    for (node_index, node) in graph.nodes_with_ids() {
        for (port_index, port) in node.ports.iter_with_ids() {
            let target = PortRef {
                node: node_index,
                port: port_index,
            };
            for (position, edge) in port.incoming_edges().iter().copied().enumerate() {
                positions.insert(edge, (target, position));
            }
        }
    }
    positions
}

pub fn join_long_edges(graph: &mut LGraph) {
    let add_unnecessary_bendpoints = graph.options.unnecessary_bendpoints;
    let mut has_long_edge_dummy = false;
    'preflight: for layer in graph.layers() {
        for &node_index in layer.nodes() {
            if graph.layerless_nodes[node_index].kind() == LNodeKind::LongEdge {
                has_long_edge_dummy = true;
                break 'preflight;
            }
        }
    }
    if !has_long_edge_dummy {
        return;
    }

    let mut incoming_edge_positions = index_all_incoming_edges(graph);

    for layer_index in 0..graph.layers().len() {
        let mut layer_nodes = graph.layers()[layer_index].nodes().to_vec();
        let mut retained_count = 0usize;

        for node_position in 0..layer_nodes.len() {
            let node_index = layer_nodes[node_position];
            if graph.layerless_nodes[node_index].kind() == LNodeKind::LongEdge {
                join_long_edge_at_indexed(
                    graph,
                    node_index,
                    add_unnecessary_bendpoints,
                    Some(&mut incoming_edge_positions),
                );
            } else {
                layer_nodes[retained_count] = node_index;
                retained_count += 1;
            }
        }

        layer_nodes.truncate(retained_count);
        graph.set_layer_nodes(layer_index, layer_nodes);
    }
}

pub fn join_long_edge_at(
    graph: &mut LGraph,
    long_edge_dummy: NodeId,
    add_unnecessary_bendpoints: bool,
) {
    join_long_edge_at_indexed(graph, long_edge_dummy, add_unnecessary_bendpoints, None);
}

#[derive(Debug, Clone, Copy)]
struct LongEdgeJoinPair {
    surviving_edge: EdgeId,
    dropped_edge: EdgeId,
    dropped_target: PortRef,
}

fn index_join_pair_targets(
    graph: &LGraph,
    pairs: &[LongEdgeJoinPair],
) -> HashMap<EdgeId, (PortRef, usize)> {
    let dropped_edges = pairs
        .iter()
        .map(|pair| pair.dropped_edge)
        .collect::<HashSet<_>>();
    let mut seen_target_ports = HashSet::with_capacity(pairs.len());
    let mut positions = HashMap::with_capacity(pairs.len());

    for target in pairs.iter().map(|pair| pair.dropped_target) {
        if !seen_target_ports.insert(target) {
            continue;
        }
        for (position, edge) in graph.layerless_nodes[target.node].ports[target.port]
            .incoming_edges()
            .iter()
            .copied()
            .enumerate()
        {
            if dropped_edges.contains(&edge) {
                positions.insert(edge, (target, position));
            }
        }
    }
    positions
}

pub(crate) fn join_long_edge_at_indexed(
    graph: &mut LGraph,
    long_edge_dummy: NodeId,
    add_unnecessary_bendpoints: bool,
    incoming_edge_positions: Option<&mut HashMap<EdgeId, (PortRef, usize)>>,
) {
    let Some(input_port) = first_port_on_side(graph, long_edge_dummy, PortSide::West) else {
        return;
    };
    let Some(output_port) = first_port_on_side(graph, long_edge_dummy, PortSide::East) else {
        return;
    };

    let input_port_ref = PortRef {
        node: long_edge_dummy,
        port: input_port,
    };
    let output_port_ref = PortRef {
        node: long_edge_dummy,
        port: output_port,
    };
    let pair_count = graph.layerless_nodes[long_edge_dummy].ports[input_port]
        .incoming_edges()
        .len()
        .min(
            graph.layerless_nodes[long_edge_dummy].ports[output_port]
                .outgoing_edges()
                .len(),
        );
    if pair_count == 0 {
        return;
    }

    // ELK zips the existing input/output lists by index and keeps the input edge as the survivor.
    // Reordering either list, or choosing the output edge as survivor, changes labels and geometry.
    let mut pairs = Vec::with_capacity(pair_count);
    for (&surviving_edge, &dropped_edge) in graph.layerless_nodes[long_edge_dummy].ports[input_port]
        .incoming_edges()
        .iter()
        .take(pair_count)
        .zip(
            graph.layerless_nodes[long_edge_dummy].ports[output_port]
                .outgoing_edges()
                .iter()
                .take(pair_count),
        )
    {
        pairs.push(LongEdgeJoinPair {
            surviving_edge,
            dropped_edge,
            dropped_target: graph.edges[dropped_edge].target(),
        });
    }

    let mut local_incoming_edge_positions = None;
    let incoming_edge_positions = match incoming_edge_positions {
        Some(incoming_edge_positions) => incoming_edge_positions,
        None => local_incoming_edge_positions.insert(index_join_pair_targets(graph, &pairs)),
    };

    // Replacing the dropped edge in place preserves its exact incoming-edge slot without the
    // repeated shifts caused by insert-then-remove on the same target port.
    for pair in &pairs {
        let target_incoming = graph.layerless_nodes[pair.dropped_target.node].ports
            [pair.dropped_target.port]
            .incoming_edges();
        let dropped_target_position = incoming_edge_positions
            .get(&pair.dropped_edge)
            .and_then(|indexed| {
                let (target, position) = *indexed;
                (target == pair.dropped_target
                    && target_incoming.get(position) == Some(&pair.dropped_edge))
                .then_some(position)
            })
            .or_else(|| {
                target_incoming
                    .iter()
                    .position(|edge| *edge == pair.dropped_edge)
            })
            .expect("long-edge output must remain attached to its recorded target port");
        // ELK's ListIterator replacement keeps the surviving input edge in the exact slot of the
        // paired output edge. The linear fallback repairs a stale internal index without silently
        // appending and leaving the dropped edge attached.
        graph.replace_incoming_edge_at(
            pair.dropped_target,
            dropped_target_position,
            pair.surviving_edge,
        );
        let surviving_position = dropped_target_position;
        incoming_edge_positions.remove(&pair.dropped_edge);
        incoming_edge_positions.insert(
            pair.surviving_edge,
            (pair.dropped_target, surviving_position),
        );
    }

    graph.remove_leading_incoming_edges(input_port_ref, pair_count);
    for (position, edge) in graph.layerless_nodes[input_port_ref.node].ports[input_port_ref.port]
        .incoming_edges()
        .iter()
        .copied()
        .enumerate()
    {
        incoming_edge_positions.insert(edge, (input_port_ref, position));
    }
    graph.remove_leading_outgoing_edges(output_port_ref, pair_count);

    let unnecessary_bendpoint =
        add_unnecessary_bendpoints.then(|| long_edge_dummy_anchor(graph, long_edge_dummy));
    for pair in pairs {
        join_long_edge_pair(
            graph,
            pair.surviving_edge,
            pair.dropped_edge,
            unnecessary_bendpoint,
        );
    }
}

pub(crate) fn first_port_on_side(graph: &LGraph, node: NodeId, side: PortSide) -> Option<PortId> {
    graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .find(|(_, port)| port.side == side)
        .map(|(id, _)| id)
}

fn join_long_edge_pair(
    graph: &mut LGraph,
    surviving_edge: EdgeId,
    dropped_edge: EdgeId,
    unnecessary_bendpoint: Option<LPoint>,
) {
    let dropped_bend_points = graph.edges[dropped_edge].bend_points.clone();
    let dropped_junction_points = graph.edges[dropped_edge].junction_points.clone();
    let dropped_labels = std::mem::take(&mut graph.edges[dropped_edge].labels);

    if let Some(unnecessary_bendpoint) = unnecessary_bendpoint {
        graph.edges[surviving_edge]
            .bend_points
            .push(unnecessary_bendpoint);
    }
    graph.edges[surviving_edge]
        .bend_points
        .extend(dropped_bend_points);
    graph.edges[surviving_edge].labels.extend(dropped_labels);
    // `LongEdgeJoiner.joinAt` joins the junction points too.
    graph.edges[surviving_edge]
        .junction_points
        .extend(dropped_junction_points);
}

fn long_edge_dummy_anchor(graph: &LGraph, node: NodeId) -> LPoint {
    let port = graph.layerless_nodes[node]
        .ports
        .first()
        .expect("a long edge dummy has ports");
    LPoint {
        x: graph.layerless_nodes[node].position.x + port.position.x + port.anchor.x,
        y: graph.layerless_nodes[node].position.y + port.position.y + port.anchor.y,
    }
}
