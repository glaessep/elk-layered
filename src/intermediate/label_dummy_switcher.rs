//! Port of ELK's `LabelDummySwitcher`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LabelDummySwitcher.java

use crate::graph::{LGraph, LNodeKind, NodeId, PortRef, PortSide};
use crate::intermediate::long_edge_joiner::first_port_on_side;

pub fn switch_label_dummies(graph: &mut LGraph) {
    let label_dummies = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .filter(|node| graph.layerless_nodes[*node].kind() == LNodeKind::Label)
        .collect::<Vec<_>>();

    for label_dummy in label_dummies {
        if let Some(info) = LabelDummyInfo::new(graph, label_dummy) {
            let target_layer = find_median_layer_target_id(&info);
            assign_label_dummy_layer(graph, &info, target_layer);
            // Java: LabelDummySwitcher.updateLongEdgeSourceLabelDummyInfo sets
            // LONG_EDGE_BEFORE_LABEL_DUMMY only. It leaves LONG_EDGE_HAS_LABEL_DUMMIES alone:
            // that flag comes from LongEdgeSplitter. The merger reads "before the label dummy"
            // from the graph in `is_before_label_dummy`.
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LabelDummyInfo {
    pub(crate) label_dummy: NodeId,
    pub(crate) left_long_edge_dummies: Vec<NodeId>,
    pub(crate) right_long_edge_dummies: Vec<NodeId>,
    pub(crate) leftmost_layer_id: usize,
    pub(crate) rightmost_layer_id: usize,
}

impl LabelDummyInfo {
    fn new(graph: &LGraph, label_dummy: NodeId) -> Option<Self> {
        let left_long_edge_dummies = gather_left_long_edge_dummies(graph, label_dummy)?;
        let right_long_edge_dummies = gather_right_long_edge_dummies(graph, label_dummy)?;
        let leftmost_layer_id = left_long_edge_dummies
            .first()
            .copied()
            .unwrap_or(label_dummy);
        let leftmost_layer_id = graph.layerless_nodes[leftmost_layer_id].layer_index()?;
        let rightmost_layer_id = right_long_edge_dummies
            .last()
            .copied()
            .unwrap_or(label_dummy);
        let rightmost_layer_id = graph.layerless_nodes[rightmost_layer_id].layer_index()?;

        Some(Self {
            label_dummy,
            left_long_edge_dummies,
            right_long_edge_dummies,
            leftmost_layer_id,
            rightmost_layer_id,
        })
    }

    fn dummy_at_offset(&self, offset: usize) -> Option<NodeId> {
        if offset < self.left_long_edge_dummies.len() {
            self.left_long_edge_dummies.get(offset).copied()
        } else if offset == self.left_long_edge_dummies.len() {
            Some(self.label_dummy)
        } else {
            self.right_long_edge_dummies
                .get(offset - self.left_long_edge_dummies.len() - 1)
                .copied()
        }
    }
}

fn gather_left_long_edge_dummies(graph: &LGraph, label_dummy: NodeId) -> Option<Vec<NodeId>> {
    let mut out = Vec::new();
    let mut current = label_dummy;

    loop {
        let incoming = graph.node_incoming_edges(current).into_iter().next()?;
        let source = graph.edges[incoming].source().node;
        if graph.layerless_nodes[source].kind() == LNodeKind::LongEdge {
            out.push(source);
            current = source;
        } else {
            break;
        }
    }

    out.reverse();
    Some(out)
}

fn gather_right_long_edge_dummies(graph: &LGraph, label_dummy: NodeId) -> Option<Vec<NodeId>> {
    let mut out = Vec::new();
    let mut current = label_dummy;

    loop {
        let outgoing = graph.node_outgoing_edges(current).into_iter().next()?;
        let target = graph.edges[outgoing].target().node;
        if graph.layerless_nodes[target].kind() == LNodeKind::LongEdge {
            out.push(target);
            current = target;
        } else {
            break;
        }
    }

    Some(out)
}

pub(crate) fn find_median_layer_target_id(info: &LabelDummyInfo) -> usize {
    // Layered.melk defaults center-edge labels to MEDIAN_LAYER. Java's LabelDummySwitcher uses
    // the lower median when an edge spans an even number of eligible dummy layers.
    let total = info.rightmost_layer_id - info.leftmost_layer_id + 1;
    info.leftmost_layer_id + (total - 1) / 2
}

fn assign_label_dummy_layer(graph: &mut LGraph, info: &LabelDummyInfo, target_layer: usize) {
    let current_layer = graph.layerless_nodes[info.label_dummy]
        .layer_index()
        .unwrap_or(target_layer);
    if current_layer == target_layer {
        return;
    }

    let Some(target_dummy) = info.dummy_at_offset(target_layer - info.leftmost_layer_id) else {
        return;
    };
    if graph.layerless_nodes[target_dummy].kind() != LNodeKind::LongEdge {
        return;
    }
    swap_label_and_long_edge_dummies(graph, info.label_dummy, target_dummy);
}

fn swap_label_and_long_edge_dummies(
    graph: &mut LGraph,
    label_dummy: NodeId,
    long_edge_dummy: NodeId,
) {
    let Some(label_layer) = graph.layerless_nodes[label_dummy].layer_index() else {
        return;
    };
    let Some(long_edge_layer) = graph.layerless_nodes[long_edge_dummy].layer_index() else {
        return;
    };
    if !graph.layers()[label_layer].nodes().contains(&label_dummy)
        || !graph.layers()[long_edge_layer]
            .nodes()
            .contains(&long_edge_dummy)
    {
        return;
    }

    let Some(label_input) = first_port_on_side(graph, label_dummy, PortSide::West) else {
        return;
    };
    let Some(label_output) = first_port_on_side(graph, label_dummy, PortSide::East) else {
        return;
    };
    let Some(long_input) = first_port_on_side(graph, long_edge_dummy, PortSide::West) else {
        return;
    };
    let Some(long_output) = first_port_on_side(graph, long_edge_dummy, PortSide::East) else {
        return;
    };

    let label_input_ref = PortRef {
        node: label_dummy,
        port: label_input,
    };
    let label_output_ref = PortRef {
        node: label_dummy,
        port: label_output,
    };
    let long_input_ref = PortRef {
        node: long_edge_dummy,
        port: long_input,
    };
    let long_output_ref = PortRef {
        node: long_edge_dummy,
        port: long_output,
    };
    let label_incoming = graph.layerless_nodes[label_dummy].ports[label_input]
        .incoming_edges()
        .to_vec();
    let label_outgoing = graph.layerless_nodes[label_dummy].ports[label_output]
        .outgoing_edges()
        .to_vec();
    let long_incoming = graph.layerless_nodes[long_edge_dummy].ports[long_input]
        .incoming_edges()
        .to_vec();
    let long_outgoing = graph.layerless_nodes[long_edge_dummy].ports[long_output]
        .outgoing_edges()
        .to_vec();

    graph.swap_layer_places(label_dummy, long_edge_dummy);
    for edge in long_incoming {
        graph.set_edge_target(edge, label_input_ref);
    }
    for edge in long_outgoing {
        graph.set_edge_source(edge, label_output_ref);
    }

    for edge in label_incoming {
        graph.set_edge_target(edge, long_input_ref);
    }
    for edge in label_outgoing {
        graph.set_edge_source(edge, long_output_ref);
    }
}
