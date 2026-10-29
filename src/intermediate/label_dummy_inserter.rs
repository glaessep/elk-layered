//! Port of ELK's `LabelDummyInserter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LabelDummyInserter.java

use crate::graph::{DummyData, EdgeId, EdgeLabelPlacement, LGraph, LNode, LabelData, NodeId};
use crate::intermediate::long_edge_splitter::split_edge;
use crate::options::PortConstraints;

// Java: LabelDummyInserter.process. It visits the layerless nodes in order, and each node's
// outgoing edges in port order. The dummies are created in that order, so it fixes their node
// order inside a layer before crossing minimisation. Walking the edge list by index gives another
// order whenever a node has an edge on a declared port and one on the node itself.
pub fn insert_label_dummies(graph: &mut LGraph) {
    let edge_indices = graph
        .node_ids()
        .flat_map(|node| graph.node_outgoing_edges(node))
        .filter(|&index| {
            let edge = &graph.edges[index];
            edge.source().node != edge.target().node
                && edge
                    .labels
                    .iter()
                    .any(|label| label.placement == EdgeLabelPlacement::Center)
        })
        .collect::<Vec<_>>();

    for edge_index in edge_indices {
        if !graph.edge_source_attached(edge_index) || !graph.edge_target_attached(edge_index) {
            continue;
        }

        let thickness = graph.edges[edge_index].thickness.max(0.0);
        graph.edges[edge_index].thickness = thickness;
        let original_source = graph.edges[edge_index].source();
        let original_target = graph.edges[edge_index].target();
        let dummy = create_label_dummy_node(graph, edge_index, thickness);
        if split_edge(graph, edge_index, dummy).is_none() {
            continue;
        }

        if let Some(ends) = graph.layerless_nodes[dummy].long_edge_ends_mut() {
            ends.source = Some(original_source);
            ends.target = Some(original_target);
        }
        move_center_labels_to_dummy(graph, edge_index, dummy);
        size_label_dummy(graph, dummy, thickness);
    }
}

pub(crate) fn create_label_dummy_node(
    graph: &mut LGraph,
    edge_index: EdgeId,
    thickness: f64,
) -> NodeId {
    let dummy_index = graph.push_node_with(|id| {
        let mut dummy = LNode::new(
            format!("label:{}:{}", edge_index.index(), id.index()),
            0.0,
            0.0,
            None,
        );
        dummy.set_dummy(DummyData::Label(LabelData {
            origin_edge: Some(edge_index),
            ..LabelData::default()
        }));
        dummy.port_constraints = PortConstraints::FixedPos;
        dummy
    });
    graph.layerless_nodes[dummy_index].size.height = thickness.max(0.0);
    dummy_index
}

fn move_center_labels_to_dummy(graph: &mut LGraph, edge_index: EdgeId, dummy: NodeId) {
    let mut retained = Vec::new();
    let mut moved = Vec::new();

    for label in std::mem::take(&mut graph.edges[edge_index].labels) {
        if label.placement == EdgeLabelPlacement::Center {
            moved.push(label);
        } else {
            retained.push(label);
        }
    }

    graph.edges[edge_index].labels = retained;
    if let Some(label_dummy) = graph.layerless_nodes[dummy].label_dummy_mut() {
        label_dummy.represented_labels = moved;
    }
}

pub(crate) fn size_label_dummy(graph: &mut LGraph, dummy: NodeId, thickness: f64) {
    let edge_label_spacing = graph.options.spacing.edge_label;
    let label_label_spacing = graph.options.spacing.label_label;
    let mut width = 0.0;
    // LongEdgeSplitter initializes the dummy's height to the edge thickness before
    // LabelDummyInserter adds the represented labels and its own thickness allowance.
    let mut height = thickness;

    if graph.options.direction.is_vertical() {
        for label in graph.layerless_nodes[dummy].represented_labels() {
            width += label.size.width + label_label_spacing;
            height = f64::max(height, label.size.height);
        }
        if !graph.layerless_nodes[dummy].represented_labels().is_empty() {
            width -= label_label_spacing;
        }
        height += edge_label_spacing + thickness;
    } else {
        for label in graph.layerless_nodes[dummy].represented_labels() {
            width = f64::max(width, label.size.width);
            height += label.size.height + label_label_spacing;
        }
        if !graph.layerless_nodes[dummy].represented_labels().is_empty() {
            height -= label_label_spacing;
        }
        height += edge_label_spacing + thickness;
    }

    graph.layerless_nodes[dummy].size.width = width.max(0.0);
    graph.layerless_nodes[dummy].size.height = height.max(thickness);
}
