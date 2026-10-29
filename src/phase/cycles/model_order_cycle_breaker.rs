//! Port of ELK's `ModelOrderCycleBreaker`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/ModelOrderCycleBreaker.java

use crate::graph::{EdgeId, LGraph, NodeId};
use crate::options::LayerConstraint;
use crate::phase::cycles::reverse_feedback_edges;

pub fn break_cycles_model_order(graph: &mut LGraph) {
    let rev_edges = model_order_feedback_edges(graph);
    reverse_feedback_edges(graph, rev_edges, true);
}

fn model_order_feedback_edges(graph: &LGraph) -> Vec<EdgeId> {
    // Keep this aligned with Mermaid's pinned elkjs 0.9.3 worker: the runtime seeds the offset
    // from the node count, raises it for explicit model order, and consumes FIRST_SEPARATE and
    // LAST_SEPARATE counters in edge traversal order. Caching one score per node changes behavior.
    let offset = graph
        .layerless_nodes
        .iter()
        .filter_map(|node| node.model_order.map(|order| order + 1))
        .max()
        .unwrap_or(graph.layerless_nodes.len())
        .max(graph.layerless_nodes.len()) as i64;
    let mut context = ModelOrderContext::default();
    let mut feedback_edges = Vec::new();

    for source in graph.node_ids() {
        let model_order_source = context.compute_constraint_model_order(graph, source, offset);
        for port in &graph.layerless_nodes[source].ports {
            for &edge_index in port.outgoing_edges() {
                let target = graph.edges[edge_index].target().node;
                if target == source {
                    continue;
                }
                let model_order_target =
                    context.compute_constraint_model_order(graph, target, offset);
                if model_order_target < model_order_source {
                    feedback_edges.push(edge_index);
                }
            }
        }
    }
    feedback_edges
}

#[derive(Debug, Default)]
struct ModelOrderContext {
    first_separate_model_order: i64,
    last_separate_model_order: i64,
}

impl ModelOrderContext {
    fn compute_constraint_model_order(
        &mut self,
        graph: &LGraph,
        node_index: NodeId,
        offset: i64,
    ) -> i64 {
        let node = &graph.layerless_nodes[node_index];
        let mut model_order = match node.layer_constraint {
            LayerConstraint::FirstSeparate => {
                let order = -2 * offset + self.first_separate_model_order;
                self.first_separate_model_order += 1;
                order
            }
            LayerConstraint::First => -offset,
            LayerConstraint::Last => offset,
            LayerConstraint::LastSeparate => {
                let order = 2 * offset + self.last_separate_model_order;
                self.last_separate_model_order += 1;
                order
            }
            LayerConstraint::None => 0,
        };
        model_order += node.model_order.unwrap_or_default() as i64;
        model_order
    }
}
