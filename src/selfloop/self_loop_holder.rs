//! Port of ELK's `SelfLoopHolder`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/SelfLoopHolder.java

use std::collections::VecDeque;

use crate::common::cells::grow_label_cell;
use crate::graph::{
    LGraph, LPoint, LSize, NodeId, PortId, SelfHyperLoop, SelfHyperLoopLabels, SelfLoopEdge,
    SelfLoopHolder, SelfLoopLabelRef, SelfLoopPort,
};

pub(crate) fn node_has_self_loop(graph: &LGraph, node: NodeId) -> bool {
    graph.layerless_nodes[node].ports.iter().any(|port| {
        port.outgoing_edges()
            .iter()
            .any(|edge| graph.edges[*edge].source().node == graph.edges[*edge].target().node)
    })
}

pub(crate) fn install_self_loop_holder(graph: &LGraph, node: NodeId) -> SelfLoopHolder {
    let self_loop_edges = graph.layerless_nodes[node]
        .ports
        .iter()
        .flat_map(|port| port.outgoing_edges().iter().copied())
        .filter(|edge| graph.edges[*edge].source().node == graph.edges[*edge].target().node)
        .collect::<Vec<_>>();

    let mut ports = Vec::<SelfLoopPort>::new();
    let mut edges = Vec::<SelfLoopEdge>::new();

    for edge in self_loop_edges {
        let source_port = graph.edges[edge].source().port;
        let target_port = graph.edges[edge].target().port;
        ensure_self_loop_port(graph, node, source_port, &mut ports);
        ensure_self_loop_port(graph, node, target_port, &mut ports);
        edges.push(SelfLoopEdge {
            edge,
            source_port,
            target_port,
        });
    }

    let hyper_loops = initialize_hyper_loops(graph, ports, edges);

    SelfLoopHolder {
        node,
        hyper_loops,
        ports_hidden: false,
        hidden_ports: Vec::new(),
        original_port_constraints: graph.layerless_nodes[node].port_constraints,
    }
}

fn ensure_self_loop_port(
    graph: &LGraph,
    node: NodeId,
    port: PortId,
    ports: &mut Vec<SelfLoopPort>,
) {
    if ports.iter().any(|candidate| candidate.port == port) {
        return;
    }

    ports.push(SelfLoopPort {
        port,
        had_only_self_loops: port_had_only_self_loops(graph, node, port),
        hidden: false,
    });
}

fn port_had_only_self_loops(graph: &LGraph, node: NodeId, port: PortId) -> bool {
    let port_data = &graph.layerless_nodes[node].ports[port];
    port_data
        .incoming_edges()
        .iter()
        .chain(port_data.outgoing_edges().iter())
        .all(|edge| graph.edges[*edge].source().node == graph.edges[*edge].target().node)
}

fn initialize_hyper_loops(
    graph: &LGraph,
    ports: Vec<SelfLoopPort>,
    edges: Vec<SelfLoopEdge>,
) -> Vec<SelfHyperLoop> {
    let mut visited = vec![false; ports.len()];
    let mut hyper_loops = Vec::new();

    for start in 0..ports.len() {
        if visited[start] {
            continue;
        }

        let mut queue = VecDeque::from([start]);
        let mut hyper_ports = Vec::new();
        let mut hyper_edges = Vec::new();

        while let Some(port_index) = queue.pop_front() {
            if visited[port_index] {
                continue;
            }
            visited[port_index] = true;

            let port = ports[port_index].port;
            hyper_ports.push(ports[port_index].clone());

            for edge in &edges {
                if edge.source_port != port && edge.target_port != port {
                    continue;
                }

                if !hyper_edges
                    .iter()
                    .any(|candidate: &SelfLoopEdge| candidate.edge == edge.edge)
                {
                    hyper_edges.push(edge.clone());
                }

                let opposite_port = if edge.source_port == port {
                    edge.target_port
                } else {
                    edge.source_port
                };
                if let Some(opposite_index) = ports
                    .iter()
                    .position(|candidate| candidate.port == opposite_port)
                    && !visited[opposite_index]
                {
                    queue.push_back(opposite_index);
                }
            }
        }

        hyper_loops.push(SelfHyperLoop {
            ports: hyper_ports,
            edges: hyper_edges,
            labels: None,
            self_loop_type: None,
            leftmost_port: None,
            rightmost_port: None,
            occupied_sides: Vec::new(),
            routing_slots: [0; 5],
        });
    }

    for hyper_loop in &mut hyper_loops {
        initialize_self_hyper_loop_labels(graph, hyper_loop);
    }

    hyper_loops
}

fn initialize_self_hyper_loop_labels(graph: &LGraph, hyper_loop: &mut SelfHyperLoop) {
    let mut label_refs = Vec::new();
    let mut size = LSize::default();
    let label_spacing = graph.options.spacing.label_label;
    let horizontal_layout = !graph.options.direction.is_vertical();

    for sl_edge in &hyper_loop.edges {
        for (label_index, label) in graph.edges[sl_edge.edge].labels.iter().enumerate() {
            label_refs.push(SelfLoopLabelRef {
                edge: sl_edge.edge,
                label: label_index,
            });
            // `SelfHyperLoopLabels.updateSize`, which repeats the arithmetic of `LabelCell.addLabel`.
            grow_label_cell(
                &mut size,
                label.size,
                label_refs.len(),
                label_spacing,
                horizontal_layout,
            );
        }
    }

    if !label_refs.is_empty() {
        hyper_loop.labels = Some(SelfHyperLoopLabels {
            id: None,
            label_refs,
            size,
            position: LPoint::default(),
            side: None,
            alignment: None,
            alignment_reference_port: None,
        });
    }
}
