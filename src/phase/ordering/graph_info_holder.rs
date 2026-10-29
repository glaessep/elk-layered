//! Port of ELK's `GraphInfoHolder`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/GraphInfoHolder.java

use crate::graph::GraphArena;
use crate::graph::{GraphId, GraphParent, LGraph, NodeId};
use crate::phase::ordering::barycenter_heuristic::BarycenterHeuristic;
use crate::phase::ordering::counting::crossings_counter::CrossingsCounter;
use crate::phase::ordering::layer_sweep_crossing_minimizer::CrossMinType;
use crate::phase::ordering::sweep_copy::SweepCopy;
use crate::phase::ordering::sweep_port_distributor::PortDistributor;
use crate::random::JavaRandom;

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

#[derive(Debug, Clone)]
pub(crate) struct GraphInfoHolder {
    pub(crate) graph: GraphId,
    pub(crate) current_node_order: Vec<Vec<NodeId>>,
    pub(crate) currently_best_node_and_port_order: Option<SweepCopy>,
    pub(crate) best_node_and_port_order: Option<SweepCopy>,
    pub(crate) barycenter_heuristic: Option<BarycenterHeuristic>,
    /// `GraphInfoHolder.crossingsCounter`, whose port positions outlive each count.
    pub(crate) crossings_counter: CrossingsCounter,
    /// `GraphInfoHolder.portDistributor`.
    pub(crate) port_distributor: PortDistributor,
    pub(crate) random: JavaRandom,
    pub(crate) child_graphs: Vec<GraphId>,
    /// The node this graph lies in, copied from the graph once. `None` for the root.
    pub(crate) parent: Option<GraphParent>,
    pub(crate) has_external_ports: bool,
    pub(crate) use_bottom_up: bool,
}

impl GraphInfoHolder {
    pub(crate) fn new(graph: &LGraph, cross_min_type: CrossMinType) -> Self {
        let mut random = graph.random.clone();
        let current_node_order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        let mut port_distributor =
            PortDistributor::create(cross_min_type, &mut random, &current_node_order);
        // The barycenter type keeps its distributor in its heuristic. The greedy switch types
        // keep theirs here.
        let barycenter_heuristic = (cross_min_type == CrossMinType::Barycenter).then(|| {
            BarycenterHeuristic::new(graph, random.clone(), port_distributor.hand_to_heuristic())
        });
        Self {
            graph: graph.id,
            current_node_order,
            currently_best_node_and_port_order: None,
            best_node_and_port_order: None,
            barycenter_heuristic,
            crossings_counter: CrossingsCounter::default(),
            port_distributor,
            random,
            child_graphs: Vec::new(),
            parent: graph.parent,
            has_external_ports: graph.graph_properties.external_ports,
            use_bottom_up: true,
        }
    }

    pub(crate) fn set_currently_best_node_and_port_order(&mut self, graph: &LGraph) {
        self.currently_best_node_and_port_order =
            Some(SweepCopy::new(graph, &self.current_node_order));
    }

    pub(crate) fn set_best_node_and_port_order(&mut self, copy: SweepCopy) {
        self.best_node_and_port_order = Some(copy);
    }

    pub(crate) fn get_best_sweep(&self, cross_min_type: CrossMinType) -> Option<&SweepCopy> {
        if cross_min_type.is_deterministic() {
            self.currently_best_node_and_port_order.as_ref()
        } else {
            self.best_node_and_port_order.as_ref()
        }
    }
}
