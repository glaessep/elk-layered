//! Port of ELK's `EndLabelSorter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/EndLabelSorter.java

use crate::graph::{EdgeId, IdVec, LGraph, LNodeKind, NodeId};

/// Ports `EndLabelSorter.process`, `initialize` and the label group comparator at commit
/// `62d5909`.
///
/// `initialize` gives every node and every port one ascending id, layer by layer, node by node
/// and port by port. The comparator compares those ids, so the order of the layers and of the
/// nodes in them decides, not the order in which the importer made the nodes or the index of a
/// port inside its own node.
pub fn sort_end_labels(graph: &mut LGraph) {
    let (node_ids, port_ids) = end_label_sorter_ids(graph);
    let edge_sort_keys = graph
        .edge_ids()
        .map(|edge| end_label_sort_key(graph, edge, &node_ids, &port_ids))
        .collect::<IdVec<EdgeId, _>>();

    for node in graph.node_ids() {
        // `EndLabelSorter.process` walks the layers and keeps only normal nodes, so the end
        // labels at an external port dummy keep the order they were gathered in.
        if graph.layerless_nodes[node].kind() != LNodeKind::Normal
            || graph.layerless_nodes[node].layer_index().is_none()
        {
            continue;
        }
        graph.layerless_nodes[node]
            .ports
            .for_each_listed_mut(|port| {
                if port.end_labels.len() < 2 {
                    return;
                }
                port.end_labels.sort_by(|a, b| {
                    let a_key = a
                        .end_label_edge
                        .and_then(|edge| edge_sort_keys.get(edge).copied())
                        .unwrap_or((usize::MAX, usize::MAX, usize::MAX));
                    let b_key = b
                        .end_label_edge
                        .and_then(|edge| edge_sort_keys.get(edge).copied())
                        .unwrap_or((usize::MAX, usize::MAX, usize::MAX));
                    a_key.cmp(&b_key)
                });
            });
    }
}

/// `EndLabelSorter.initialize`: ascending ids over the layers, their nodes and those nodes' ports.
/// A node outside every layer keeps `usize::MAX`.
fn end_label_sorter_ids(graph: &LGraph) -> (IdVec<NodeId, usize>, IdVec<NodeId, Vec<usize>>) {
    let mut node_ids = IdVec::filled(graph.layerless_nodes.len(), usize::MAX);
    let mut port_ids = graph
        .layerless_nodes
        .iter()
        .map(|node| vec![usize::MAX; node.ports.slot_count()])
        .collect::<IdVec<NodeId, _>>();
    let mut next_element_id = 0;
    for layer in graph.layers() {
        for &node in layer.nodes() {
            node_ids[node] = next_element_id;
            next_element_id += 1;
            for port in graph.layerless_nodes[node].ports.ids() {
                port_ids[node][port.index()] = next_element_id;
                next_element_id += 1;
            }
        }
    }
    (node_ids, port_ids)
}

fn end_label_sort_key(
    graph: &LGraph,
    edge: EdgeId,
    node_ids: &IdVec<NodeId, usize>,
    port_ids: &IdVec<NodeId, Vec<usize>>,
) -> (usize, usize, usize) {
    let edge = &graph.edges[edge];
    let source_port = port_ids[edge.source().node][edge.source().port.index()];
    let target_node = node_ids[edge.target().node];
    // Java compares the target ports backwards: western ports run bottom to top.
    let target_port =
        usize::MAX.saturating_sub(port_ids[edge.target().node][edge.target().port.index()]);
    (source_port, target_node, target_port)
}
