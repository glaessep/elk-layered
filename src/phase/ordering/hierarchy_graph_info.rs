//! Port of ELK's `GraphInfoHolder`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/GraphInfoHolder.java
//!
//! This is the holder the hierarchical sweep uses for each graph it sweeps.

use crate::graph::{GraphId, GraphParent, LGraph, NodeId};
use crate::phase::ordering::barycenter_heuristic::BarycenterHeuristic;
use crate::phase::ordering::counting::crossings_counter::CrossingsCounter;
use crate::phase::ordering::layer_sweep_crossing_minimizer::CrossMinType;
use crate::phase::ordering::sweep_copy::SweepCopy;
use crate::phase::ordering::sweep_port_distributor::PortDistributor;
use crate::random::JavaRandom;

#[derive(Debug, Clone)]
pub(crate) struct HierarchyGraphInfo {
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

impl HierarchyGraphInfo {
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
