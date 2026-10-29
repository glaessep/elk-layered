//! Port of ELK's `LayerSweepTypeDecider`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/LayerSweepTypeDecider.java

use std::collections::HashMap;

use crate::graph::{LGraph, LNodeKind, NodeId, PortRef, PortSide};
use crate::phase::ordering::counting::crossings_counter::ports_in_north_south_east_west_order;
use crate::phase::ordering::{connected_edges, is_north_south_node};

#[derive(Debug, Default)]
pub(crate) struct LayerSweepTypeDecider {
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
    pub(crate) fn decide(
        &mut self,
        graph: &LGraph,
        parent_graph: Option<&LGraph>,
        boundary: f64,
    ) -> bool {
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

fn has_connected_ports_on_side(graph: &LGraph, node: NodeId, side: PortSide) -> bool {
    ports_in_north_south_east_west_order(graph, node, side)
        .into_iter()
        .any(|port| connected_edges(graph, port).next().is_some())
}

pub(crate) fn external_dummy_origin_side(
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
