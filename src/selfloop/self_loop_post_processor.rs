//! Port of ELK's `SelfLoopPostProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/SelfLoopPostProcessor.java

use crate::graph::{EdgeId, LGraph, LPoint, NodeId, PortRef, SelfHyperLoopLabels, SelfLoopEdge};

pub fn postprocess_self_loops(graph: &mut LGraph) {
    let holders = std::mem::take(&mut graph.self_loop_holders);
    for holder in &holders {
        let node_position = graph.layerless_nodes[holder.node].position;
        for hyper_loop in &holder.hyper_loops {
            for sl_edge in &hyper_loop.edges {
                reattach_self_loop_edge(graph, holder.node, sl_edge);
                offset_edge_bend_points(graph, sl_edge.edge, node_position);
            }
            if let Some(labels) = &hyper_loop.labels {
                offset_self_loop_label_positions(graph, labels, node_position);
            }
        }
    }
}

fn reattach_self_loop_edge(graph: &mut LGraph, node: NodeId, sl_edge: &SelfLoopEdge) {
    let source = PortRef {
        node,
        port: sl_edge.source_port,
    };
    let target = PortRef {
        node,
        port: sl_edge.target_port,
    };

    // An edge that is still attached sits at these ports already; moving it would change its
    // place in the port's edge list.
    if !graph.edge_source_attached(sl_edge.edge) {
        graph.set_edge_source(sl_edge.edge, source);
    }
    if !graph.edge_target_attached(sl_edge.edge) {
        graph.set_edge_target(sl_edge.edge, target);
    }
}

fn offset_edge_bend_points(graph: &mut LGraph, edge: EdgeId, offset: LPoint) {
    for point in &mut graph.edges[edge].bend_points {
        point.x += offset.x;
        point.y += offset.y;
    }
}

fn offset_self_loop_label_positions(
    graph: &mut LGraph,
    labels: &SelfHyperLoopLabels,
    offset: LPoint,
) {
    for label_ref in &labels.label_refs {
        if let Some(label) = graph
            .edges
            .get_mut(label_ref.edge)
            .and_then(|edge| edge.labels.get_mut(label_ref.label))
        {
            label.position.x += offset.x;
            label.position.y += offset.y;
        }
    }
}
