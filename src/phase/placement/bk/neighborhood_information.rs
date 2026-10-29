//! Port of ELK's `NeighborhoodInformation`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/bk/NeighborhoodInformation.java

use crate::graph::{EdgeId, IdVec, LGraph, LayeredEdge, NodeId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Neighbor {
    pub(crate) node: NodeId,
    pub(crate) edge: EdgeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NeighborhoodInformation {
    pub(crate) node_count: usize,
    pub(crate) node_id: IdVec<NodeId, usize>,
    pub(crate) id_to_node: Vec<NodeId>,
    pub(crate) layer_index: Vec<usize>,
    pub(crate) node_index: Vec<usize>,
    pub(crate) left_neighbors: Vec<Vec<Neighbor>>,
    pub(crate) right_neighbors: Vec<Vec<Neighbor>>,
}

impl NeighborhoodInformation {
    pub(crate) fn build_for(graph: &LGraph) -> Self {
        let node_count = graph.layers().iter().map(|layer| layer.nodes().len()).sum();
        let mut node_id = IdVec::filled(graph.layerless_nodes.len(), usize::MAX);
        let mut id_to_node = Vec::with_capacity(node_count);
        let mut layer_index = vec![usize::MAX; graph.layers().len()];
        let mut node_index = vec![usize::MAX; node_count];

        let mut next_id = 0usize;
        for (layer_idx, layer) in graph.layers().iter().enumerate() {
            layer_index[layer_idx] = layer_idx;
            for (node_idx, node) in layer.nodes().iter().copied().enumerate() {
                node_id[node] = next_id;
                id_to_node.push(node);
                node_index[next_id] = node_idx;
                next_id += 1;
            }
        }

        let mut ni = Self {
            node_count,
            node_id,
            id_to_node,
            layer_index,
            node_index,
            left_neighbors: vec![Vec::new(); node_count],
            right_neighbors: vec![Vec::new(); node_count],
        };
        ni.determine_all_left_neighbors(graph);
        ni.determine_all_right_neighbors(graph);
        ni
    }

    pub(crate) fn iter_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.id_to_node.iter().copied()
    }

    fn determine_all_left_neighbors(&mut self, graph: &LGraph) {
        let nodes = self.id_to_node.clone();
        for node in nodes {
            let id = self.node_id[node];
            let mut result = Vec::new();
            let mut max_priority = 0;

            for edge in graph.node_incoming_edges(node) {
                if is_self_loop_or_in_layer_edge(graph, edge) {
                    continue;
                }
                let edge_priority = graph.edges[edge].priority_straightness;
                if edge_priority > max_priority {
                    max_priority = edge_priority;
                    result.clear();
                }
                if edge_priority == max_priority {
                    result.push(Neighbor {
                        node: graph.edges[edge].source().node,
                        edge,
                    });
                }
            }

            result.sort_by_key(|neighbor| self.node_index[self.node_id[neighbor.node]]);
            self.left_neighbors[id] = result;
        }
    }

    fn determine_all_right_neighbors(&mut self, graph: &LGraph) {
        let nodes = self.id_to_node.clone();
        for node in nodes {
            let id = self.node_id[node];
            let mut result = Vec::new();
            let mut max_priority = 0;

            for edge in graph.node_outgoing_edges(node) {
                if is_self_loop_or_in_layer_edge(graph, edge) {
                    continue;
                }
                let edge_priority = graph.edges[edge].priority_straightness;
                if edge_priority > max_priority {
                    max_priority = edge_priority;
                    result.clear();
                }
                if edge_priority == max_priority {
                    result.push(Neighbor {
                        node: graph.edges[edge].target().node,
                        edge,
                    });
                }
            }

            result.sort_by_key(|neighbor| self.node_index[self.node_id[neighbor.node]]);
            self.right_neighbors[id] = result;
        }
    }
}

fn is_self_loop_or_in_layer_edge(graph: &LGraph, edge: EdgeId) -> bool {
    let edge = &graph.edges[edge];
    edge.source().node == edge.target().node || is_in_layer_edge(graph, edge)
}

fn is_in_layer_edge(graph: &LGraph, edge: &LayeredEdge) -> bool {
    graph.layerless_nodes[edge.source().node].layer_index()
        == graph.layerless_nodes[edge.target().node].layer_index()
}

pub(crate) fn is_in_layer_edge_index(graph: &LGraph, edge: EdgeId) -> bool {
    let edge = &graph.edges[edge];
    is_in_layer_edge(graph, edge)
}
