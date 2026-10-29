//! Port of ELK's `GreedyCycleBreaker`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/GreedyCycleBreaker.java

use std::collections::VecDeque;

use crate::graph::{IdVec, LGraph, NodeId, PortRef, reverse_edge};

pub fn break_cycles_greedy(graph: &mut LGraph) {
    break_cycles_greedy_with_choice(graph, GreedyNodeChoice::Random);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GreedyNodeChoice {
    Random,
    MinimumModelOrder,
}

pub(crate) fn break_cycles_greedy_with_choice(graph: &mut LGraph, choice: GreedyNodeChoice) {
    let node_count = graph.layerless_nodes.len();
    let mut indeg = IdVec::filled(node_count, 0i32);
    let mut outdeg = IdVec::filled(node_count, 0i32);
    let mut mark = IdVec::filled(node_count, 0i32);
    let mut sources = VecDeque::new();
    let mut sinks = VecDeque::new();

    for node_index in graph.node_ids() {
        for port in &graph.layerless_nodes[node_index].ports {
            for edge_index in port.incoming_edges() {
                let edge = &graph.edges[*edge_index];
                if edge.source().node == node_index {
                    continue;
                }
                indeg[node_index] += positive_priority_weight(edge.priority_direction);
            }

            for edge_index in port.outgoing_edges() {
                let edge = &graph.edges[*edge_index];
                if edge.target().node == node_index {
                    continue;
                }
                outdeg[node_index] += positive_priority_weight(edge.priority_direction);
            }
        }

        if outdeg[node_index] == 0 {
            sinks.push_back(node_index);
        } else if indeg[node_index] == 0 {
            sources.push_back(node_index);
        }
    }

    let mut unprocessed_node_count = node_count;
    let mut next_right = -1;
    let mut next_left = 1;

    while unprocessed_node_count > 0 {
        while let Some(sink) = sinks.pop_front() {
            mark[sink] = next_right;
            next_right -= 1;
            update_neighbors(
                graph,
                sink,
                &mut indeg,
                &mut outdeg,
                &mark,
                &mut sources,
                &mut sinks,
            );
            unprocessed_node_count -= 1;
        }

        while let Some(source) = sources.pop_front() {
            mark[source] = next_left;
            next_left += 1;
            update_neighbors(
                graph,
                source,
                &mut indeg,
                &mut outdeg,
                &mark,
                &mut sources,
                &mut sinks,
            );
            unprocessed_node_count -= 1;
        }

        if unprocessed_node_count > 0 {
            let mut max_outflow = i32::MIN;
            let mut max_nodes = Vec::new();

            for node_index in graph.node_ids() {
                if mark[node_index] == 0 {
                    let outflow = outdeg[node_index] - indeg[node_index];
                    if outflow >= max_outflow {
                        if outflow > max_outflow {
                            max_nodes.clear();
                            max_outflow = outflow;
                        }
                        max_nodes.push(node_index);
                    }
                }
            }

            if let Some(max_node) = choose_node_with_max_outflow(graph, &max_nodes, choice) {
                mark[max_node] = next_left;
                next_left += 1;
                update_neighbors(
                    graph,
                    max_node,
                    &mut indeg,
                    &mut outdeg,
                    &mark,
                    &mut sources,
                    &mut sinks,
                );
                unprocessed_node_count -= 1;
            } else {
                break;
            }
        }
    }

    let shift_base = node_count as i32 + 1;
    for node_mark in &mut mark {
        if *node_mark < 0 {
            *node_mark += shift_base;
        }
    }

    for node_index in graph.node_ids() {
        // `reverse_edge` with `adapt_ports` can add a collector port to the node's list, so the
        // loop walks a copy of the ids.
        let port_ids = graph.layerless_nodes[node_index]
            .ports
            .ids()
            .collect::<Vec<_>>();
        for port_index in port_ids {
            let outgoing_edges = graph.layerless_nodes[node_index].ports[port_index]
                .outgoing_edges()
                .to_vec();
            for edge_index in outgoing_edges {
                let target_index = graph.edges[edge_index].target().node;
                if mark[node_index] > mark[target_index] && reverse_edge(graph, edge_index, true) {
                    graph.cyclic = true;
                }
            }
        }
    }
}

fn choose_node_with_max_outflow(
    graph: &mut LGraph,
    nodes: &[NodeId],
    choice: GreedyNodeChoice,
) -> Option<NodeId> {
    if choice == GreedyNodeChoice::MinimumModelOrder
        && let Some(node) = nodes
            .iter()
            .copied()
            .filter(|node| graph.layerless_nodes[*node].model_order.is_some())
            .min_by_key(|node| {
                graph.layerless_nodes[*node]
                    .model_order
                    .unwrap_or(usize::MAX)
            })
    {
        return Some(node);
    }

    let random_index = graph.random.next_int(nodes.len())?;
    nodes.get(random_index).copied()
}

fn update_neighbors(
    graph: &LGraph,
    node_index: NodeId,
    indeg: &mut IdVec<NodeId, i32>,
    outdeg: &mut IdVec<NodeId, i32>,
    mark: &IdVec<NodeId, i32>,
    sources: &mut VecDeque<NodeId>,
    sinks: &mut VecDeque<NodeId>,
) {
    for (port_index, port) in graph.layerless_nodes[node_index].ports.iter_with_ids() {
        let port_ref = PortRef {
            node: node_index,
            port: port_index,
        };
        let connected_edges = port
            .incoming_edges()
            .iter()
            .chain(port.outgoing_edges().iter())
            .copied()
            .collect::<Vec<_>>();

        for edge_index in connected_edges {
            let edge = &graph.edges[edge_index];
            let connected_port = if edge.source() == port_ref {
                edge.target()
            } else {
                edge.source()
            };
            let endpoint_index = connected_port.node;

            if node_index == endpoint_index {
                continue;
            }

            if mark[endpoint_index] == 0 {
                let weight = non_negative_priority_weight(edge.priority_direction);
                if edge.target() == connected_port {
                    indeg[endpoint_index] -= weight;
                    if indeg[endpoint_index] <= 0 && outdeg[endpoint_index] > 0 {
                        sources.push_back(endpoint_index);
                    }
                } else {
                    outdeg[endpoint_index] -= weight;
                    if outdeg[endpoint_index] <= 0 && indeg[endpoint_index] > 0 {
                        sinks.push_back(endpoint_index);
                    }
                }
            }
        }
    }
}

fn positive_priority_weight(priority: i32) -> i32 {
    if priority > 0 { priority + 1 } else { 1 }
}

fn non_negative_priority_weight(priority: i32) -> i32 {
    priority.max(0) + 1
}
