//! Port of ELK's `GraphInfoHolder`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/GraphInfoHolder.java

use crate::graph::{GraphArena, LGraph, NodeId};
use crate::phase::ordering::sweep_copy::SweepCopy;

pub(crate) fn initialize_crossing_minimization_port_ids(graph: &mut LGraph) {
    clear_crossing_minimization_port_ids(graph);
    let order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    assign_crossing_minimization_port_ids(graph, &order);
}

pub(crate) fn initialize_crossing_minimization_port_ids_hierarchy(graphs: &mut GraphArena) {
    // The ids are numbered inside each graph, so the order of the graphs does not matter.
    for graph in graphs.iter_mut() {
        initialize_crossing_minimization_port_ids(graph);
    }
}

fn clear_crossing_minimization_port_ids(graph: &mut LGraph) {
    for node in &mut graph.layerless_nodes {
        for port in node.ports.slots_mut() {
            port.crossing_minimization_id = None;
        }
    }
}

fn assign_crossing_minimization_port_ids(graph: &mut LGraph, order: &[Vec<NodeId>]) {
    let mut next_port_id = 0usize;
    for layer in order {
        for node in layer {
            let Some(node_data) = graph.layerless_nodes.get_mut(*node) else {
                continue;
            };
            node_data.ports.for_each_listed_mut(|port| {
                port.crossing_minimization_id = Some(next_port_id);
                next_port_id += 1;
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphInfoHolder {
    pub current_node_order: Vec<Vec<NodeId>>,
    pub currently_best_node_and_port_order: Option<SweepCopy>,
    pub best_node_and_port_order: Option<SweepCopy>,
    /// `GraphInfoHolder.crossingsCounter`, whose port positions outlive each count.
    pub crossings_counter: crate::phase::ordering::counting::CrossingsCounter,
}

impl GraphInfoHolder {
    pub fn new(graph: &LGraph) -> Self {
        Self {
            current_node_order: graph
                .layers()
                .iter()
                .map(|layer| layer.nodes().to_vec())
                .collect(),
            currently_best_node_and_port_order: None,
            best_node_and_port_order: None,
            crossings_counter: Default::default(),
        }
    }

    pub fn set_currently_best_node_and_port_order(&mut self, graph: &LGraph) {
        self.currently_best_node_and_port_order =
            Some(SweepCopy::new(graph, &self.current_node_order));
    }

    pub fn set_best_node_and_port_order(&mut self, copy: SweepCopy) {
        self.best_node_and_port_order = Some(copy);
    }

    pub fn get_best_sweep(&self) -> Option<&SweepCopy> {
        self.best_node_and_port_order
            .as_ref()
            .or(self.currently_best_node_and_port_order.as_ref())
    }
}
