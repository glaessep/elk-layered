//! Port of ELK's `LongEdgeSplitter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LongEdgeSplitter.java

use crate::graph::{
    DummyData, EdgeId, EdgeLabelPlacement, LGraph, LNode, LNodeKind, LPoint, LayeredEdge,
    LongEdgeData, LongEdgeEnds, NodeId, PortSide, PortType,
};
use crate::options::PortConstraints;

pub fn split_long_edges(graph: &mut LGraph) {
    if graph.layers().len() <= 2 {
        return;
    }

    let mut layer_index = 0usize;
    while layer_index + 1 < graph.layers().len() {
        let next_layer_index = layer_index + 1;
        let mut node_position = 0usize;

        while node_position < graph.layers()[layer_index].nodes().len() {
            let node_index = graph.layers()[layer_index].nodes()[node_position];
            let outgoing_edges = graph.node_outgoing_edges(node_index);

            for edge_index in outgoing_edges {
                let target_node = graph.edges[edge_index].target().node;
                let Some(target_layer_index) = graph.layerless_nodes[target_node].layer_index()
                else {
                    continue;
                };

                if target_layer_index != layer_index && target_layer_index != next_layer_index {
                    let dummy = create_long_edge_dummy_node(graph, next_layer_index, edge_index);
                    split_edge(graph, edge_index, dummy);
                }
            }

            node_position += 1;
        }

        layer_index += 1;
    }
}

fn create_long_edge_dummy_node(
    graph: &mut LGraph,
    target_layer_index: usize,
    edge_to_split: EdgeId,
) -> NodeId {
    let dummy_index = graph.push_node_with(|_| {
        let mut dummy = LNode::new(
            format!("longEdge:{}:{target_layer_index}", edge_to_split.index()),
            0.0,
            0.0,
            None,
        );
        dummy.set_dummy(DummyData::LongEdge(LongEdgeData {
            origin_edge: Some(edge_to_split),
            ..LongEdgeData::default()
        }));
        dummy.port_constraints = PortConstraints::FixedPos;
        dummy
    });
    graph.set_node_layer(dummy_index, target_layer_index);
    dummy_index
}

pub fn split_edge(graph: &mut LGraph, edge_index: EdgeId, dummy_node: NodeId) -> Option<EdgeId> {
    let old_edge_target = graph.edges.get(edge_index)?.target();
    let thickness = graph.edges[edge_index].thickness.max(0.0);
    graph.edges[edge_index].thickness = thickness;
    graph.layerless_nodes[dummy_node].size.height = thickness;
    let port_position = LPoint {
        x: 0.0,
        y: (thickness / 2.0).floor(),
    };

    let dummy_input = graph.add_port(dummy_node, PortType::Input, PortSide::West, port_position)?;
    let dummy_output =
        graph.add_port(dummy_node, PortType::Output, PortSide::East, port_position)?;

    if !graph.set_edge_target(edge_index, dummy_input) {
        return None;
    }

    // The old edge holds the clamped thickness already.
    let dummy_edge =
        LayeredEdge::segment_of(&graph.edges[edge_index], dummy_output, old_edge_target);
    let dummy_edge_index = graph.add_edge(dummy_edge)?;

    set_dummy_node_properties(graph, dummy_node, edge_index, dummy_edge_index);
    move_head_labels(graph, edge_index, dummy_edge_index);

    Some(dummy_edge_index)
}

fn set_dummy_node_properties(
    graph: &mut LGraph,
    dummy_node: NodeId,
    in_edge: EdgeId,
    out_edge: EdgeId,
) {
    let in_edge_source_node = graph.edges[in_edge].source().node;
    let out_edge_target_node = graph.edges[out_edge].target().node;

    // The ends and the flag the new dummy takes, from the dummy before it or after it.
    let (ends, has_label_dummies) =
        if graph.layerless_nodes[in_edge_source_node].kind() == LNodeKind::LongEdge {
            (
                graph.layerless_nodes[in_edge_source_node].long_edge_ends(),
                graph.layerless_nodes[in_edge_source_node]
                    .long_edge()
                    .map(|long_edge| long_edge.has_label_dummies),
            )
        } else if graph.layerless_nodes[in_edge_source_node].kind() == LNodeKind::Label {
            (
                graph.layerless_nodes[in_edge_source_node].long_edge_ends(),
                Some(true),
            )
        } else if graph.layerless_nodes[out_edge_target_node].kind() == LNodeKind::Label {
            (
                graph.layerless_nodes[out_edge_target_node].long_edge_ends(),
                Some(true),
            )
        } else {
            (
                Some(LongEdgeEnds {
                    source: Some(graph.edges[in_edge].source()),
                    target: Some(graph.edges[out_edge].target()),
                }),
                None,
            )
        };
    if let Some(long_edge) = graph.layerless_nodes[dummy_node].long_edge_mut() {
        long_edge.ends = ends.unwrap_or_default();
        if let Some(has_label_dummies) = has_label_dummies {
            long_edge.has_label_dummies = has_label_dummies;
        }
    }
}

pub(crate) fn move_head_labels(graph: &mut LGraph, old_edge: EdgeId, new_edge: EdgeId) {
    let labels = std::mem::take(&mut graph.edges[old_edge].labels);
    let mut retained = Vec::with_capacity(labels.len());
    let mut moved = Vec::new();
    for mut label in labels {
        if label.placement == EdgeLabelPlacement::Head {
            if label.end_label_edge.is_none() {
                label.end_label_edge = Some(old_edge);
            }
            moved.push(label);
        } else {
            retained.push(label);
        }
    }
    graph.edges[old_edge].labels = retained;
    graph.edges[new_edge].labels.extend(moved);
}
