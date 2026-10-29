//! Port of ELK's `BarycenterHeuristic`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/BarycenterHeuristic.java

use std::collections::HashMap;

use crate::graph::{LGraph, LNodeKind, NodeId, PortRef, PortType};
use crate::intermediate::preserveorder::model_order_node_comparator::update_bigger_and_smaller_associations;
use crate::phase::ordering::abstract_barycenter_port_distributor::BarycenterPortDistributor;
use crate::phase::ordering::forster_constraint_resolver::resolve_forster_constraints;
use crate::phase::ordering::{first_index, is_first_layer};
use crate::random::JavaRandom;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BarycenterState {
    pub(crate) summed_weight: f64,
    pub(crate) degree: usize,
    pub(crate) barycenter: Option<f64>,
    pub(crate) visited: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct BarycenterHeuristic {
    states: HashMap<NodeId, BarycenterState>,
    pub(crate) random: JavaRandom,
    pub(crate) port_distributor: BarycenterPortDistributor,
    force_node_model_order: bool,
    /// `ForsterConstraintResolver.layoutUnits`: the nodes of each layout unit, in the order the
    /// layers had when the resolver was made. The order of the layers changes later.
    pub(crate) layout_units: HashMap<NodeId, Vec<NodeId>>,
    bigger_than: HashMap<NodeId, crate::gwt::LinkedSet<NodeId>>,
    smaller_than: HashMap<NodeId, crate::gwt::LinkedSet<NodeId>>,
}

impl BarycenterHeuristic {
    const RANDOM_AMOUNT: f64 = 0.07;

    pub(crate) fn new(
        graph: &LGraph,
        random: JavaRandom,
        port_distributor: BarycenterPortDistributor,
    ) -> Self {
        let states = graph
            .layers()
            .iter()
            .flat_map(|layer| layer.nodes().iter().copied())
            .map(|node| (node, BarycenterState::default()))
            .collect();
        let mut layout_units: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for node in graph
            .layers()
            .iter()
            .flat_map(|layer| layer.nodes().iter().copied())
        {
            if let Some(unit) = graph.in_layer_layout_unit(node) {
                layout_units.entry(unit).or_default().push(node);
            }
        }
        Self {
            states,
            random,
            port_distributor,
            force_node_model_order: graph.options.force_node_model_order,
            layout_units,
            bigger_than: HashMap::new(),
            smaller_than: HashMap::new(),
        }
    }

    pub(crate) fn set_first_layer_order(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        forward: bool,
    ) {
        let Some(layer) = order.get_mut(first_index(forward, order.len())) else {
            return;
        };
        self.minimize_layer(graph, layer, false, true, forward);
    }

    pub(crate) fn minimize_crossings(
        &mut self,
        graph: &LGraph,
        order: &mut [Vec<NodeId>],
        free_layer_index: usize,
        forward: bool,
        is_first_sweep: bool,
    ) {
        if !is_first_layer(order, free_layer_index, forward) {
            let fixed_layer_index = if forward {
                free_layer_index - 1
            } else {
                free_layer_index + 1
            };
            let port_type = if forward {
                PortType::Output
            } else {
                PortType::Input
            };
            self.port_distributor
                .calculate_port_ranks(graph, &order[fixed_layer_index], port_type);
        }

        let pre_ordered = !is_first_sweep
            || order[free_layer_index]
                .first()
                .is_some_and(|node| graph.layerless_nodes[*node].kind() == LNodeKind::ExternalPort);
        let mut nodes = order[free_layer_index].clone();
        self.minimize_layer(graph, &mut nodes, pre_ordered, false, forward);
        order[free_layer_index] = nodes;
    }

    fn minimize_layer(
        &mut self,
        graph: &LGraph,
        layer: &mut [NodeId],
        pre_ordered: bool,
        randomize: bool,
        forward: bool,
    ) {
        if randomize {
            self.randomize_barycenters(layer);
        } else {
            self.calculate_barycenters(graph, layer, forward);
            self.fill_in_unknown_barycenters(layer, pre_ordered);
        }

        if layer.len() > 1 {
            if self.force_node_model_order {
                self.insertion_sort_by_model_order(graph, layer);
            } else {
                layer.sort_by(|left, right| {
                    let left = self.states.get(left).and_then(|state| state.barycenter);
                    let right = self.states.get(right).and_then(|state| state.barycenter);
                    compare_barycenters(left, right)
                });
                resolve_forster_constraints(graph, layer, &mut self.states, &self.layout_units);
            }
        }
    }

    pub(crate) fn insertion_sort_by_model_order(&mut self, graph: &LGraph, layer: &mut [NodeId]) {
        for index in 1..layer.len() {
            let node = layer[index];
            let mut insert_at = index;
            while insert_at > 0
                && self.compare_model_order_nodes(graph, layer[insert_at - 1], node)
                    == std::cmp::Ordering::Greater
            {
                layer[insert_at] = layer[insert_at - 1];
                insert_at -= 1;
            }
            layer[insert_at] = node;
        }
        self.bigger_than.clear();
        self.smaller_than.clear();
    }

    fn compare_model_order_nodes(
        &mut self,
        graph: &LGraph,
        first: NodeId,
        second: NodeId,
    ) -> std::cmp::Ordering {
        if let Some(ordering) = self.transitive_ordering(first, second) {
            return ordering;
        }

        if let (Some(first_order), Some(second_order)) = (
            graph.layerless_nodes[first].model_order,
            graph.layerless_nodes[second].model_order,
        ) {
            let ordering = first_order.cmp(&second_order);
            self.update_model_order_associations(first, second, ordering);
            return ordering;
        }

        let first_barycenter = self.states.get(&first).and_then(|state| state.barycenter);
        let second_barycenter = self.states.get(&second).and_then(|state| state.barycenter);
        let ordering = compare_barycenters(first_barycenter, second_barycenter);
        self.update_model_order_associations(first, second, ordering);
        ordering
    }

    fn transitive_ordering(&mut self, first: NodeId, second: NodeId) -> Option<std::cmp::Ordering> {
        self.bigger_than.entry(first).or_default();
        self.bigger_than.entry(second).or_default();
        self.smaller_than.entry(first).or_default();
        self.smaller_than.entry(second).or_default();

        if self.bigger_than[&first].contains(&second) || self.smaller_than[&second].contains(&first)
        {
            Some(std::cmp::Ordering::Greater)
        } else if self.bigger_than[&second].contains(&first)
            || self.smaller_than[&first].contains(&second)
        {
            Some(std::cmp::Ordering::Less)
        } else {
            None
        }
    }

    fn update_model_order_associations(
        &mut self,
        first: NodeId,
        second: NodeId,
        ordering: std::cmp::Ordering,
    ) {
        match ordering {
            std::cmp::Ordering::Less => update_bigger_and_smaller_associations(
                first,
                second,
                &mut self.bigger_than,
                &mut self.smaller_than,
            ),
            std::cmp::Ordering::Greater => update_bigger_and_smaller_associations(
                second,
                first,
                &mut self.bigger_than,
                &mut self.smaller_than,
            ),
            std::cmp::Ordering::Equal => {}
        }
    }

    fn randomize_barycenters(&mut self, nodes: &[NodeId]) {
        for node in nodes {
            let value = self.random.next_double();
            let state = self.state_mut(*node);
            state.barycenter = Some(value);
            state.summed_weight = value;
            state.degree = 1;
            state.visited = false;
        }
    }

    fn calculate_barycenters(&mut self, graph: &LGraph, nodes: &[NodeId], forward: bool) {
        for node in nodes {
            self.state_mut(*node).visited = false;
        }

        for node in nodes {
            self.calculate_barycenter(graph, *node, forward);
        }
    }

    fn calculate_barycenter(&mut self, graph: &LGraph, node: NodeId, forward: bool) {
        if self.state_mut(node).visited {
            return;
        }
        {
            let state = self.state_mut(node);
            state.visited = true;
            state.degree = 0;
            state.summed_weight = 0.0;
            state.barycenter = None;
        }

        for port in graph.layerless_nodes[node].ports.ids() {
            let free_port = PortRef { node, port };
            for fixed_port in connected_ports_for_sweep(graph, free_port, forward) {
                if graph.layerless_nodes[fixed_port.node].layer_index()
                    == graph.layerless_nodes[node].layer_index()
                {
                    if fixed_port.node != node {
                        self.calculate_barycenter(graph, fixed_port.node, forward);
                        let fixed_state = self.state(fixed_port.node).clone();
                        let state = self.state_mut(node);
                        state.degree += fixed_state.degree;
                        state.summed_weight += fixed_state.summed_weight;
                    }
                } else {
                    let rank = self.port_distributor.rank_of(graph, fixed_port);
                    let state = self.state_mut(node);
                    state.summed_weight += rank;
                    state.degree += 1;
                }
            }
        }

        for associate in graph.barycenter_associates(node).to_vec() {
            if graph.layerless_nodes[associate].layer_index()
                == graph.layerless_nodes[node].layer_index()
            {
                self.calculate_barycenter(graph, associate, forward);
                let associate_state = self.state(associate).clone();
                let state = self.state_mut(node);
                state.degree += associate_state.degree;
                state.summed_weight += associate_state.summed_weight;
            }
        }

        let degree = self.state(node).degree;
        if degree > 0 {
            let perturbation =
                self.random.next_float() as f64 * Self::RANDOM_AMOUNT - Self::RANDOM_AMOUNT / 2.0;
            let state = self.state_mut(node);
            state.summed_weight += perturbation;
            state.barycenter = Some(state.summed_weight / degree as f64);
        }
    }

    fn fill_in_unknown_barycenters(&mut self, nodes: &[NodeId], pre_ordered: bool) {
        if pre_ordered {
            let mut last_value = -1.0;
            for (index, node) in nodes.iter().copied().enumerate() {
                let mut value = self.state(node).barycenter;
                if value.is_none() {
                    let next_value = nodes
                        .iter()
                        .skip(index + 1)
                        .find_map(|next| self.state(*next).barycenter)
                        .unwrap_or(last_value + 1.0);
                    value = Some((last_value + next_value) / 2.0);
                    let state = self.state_mut(node);
                    state.barycenter = value;
                    state.summed_weight = value.unwrap_or(0.0);
                    state.degree = 1;
                }
                last_value = value.unwrap_or(last_value);
            }
        } else {
            let max_barycenter = nodes
                .iter()
                .filter_map(|node| self.state(*node).barycenter)
                .fold(0.0, f64::max)
                + 2.0;
            for node in nodes {
                if self.state(*node).barycenter.is_none() {
                    let value = self.random.next_float() as f64 * max_barycenter - 1.0;
                    let state = self.state_mut(*node);
                    state.barycenter = Some(value);
                    state.summed_weight = value;
                    state.degree = 1;
                }
            }
        }
    }

    fn state(&self, node: NodeId) -> &BarycenterState {
        self.states
            .get(&node)
            .expect("barycenter state initialized for every layered node")
    }

    pub(crate) fn state_mut(&mut self, node: NodeId) -> &mut BarycenterState {
        self.states.entry(node).or_default()
    }
}

fn compare_barycenters(first: Option<f64>, second: Option<f64>) -> std::cmp::Ordering {
    match (first, second) {
        (Some(first), Some(second)) => first.total_cmp(&second),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn connected_ports_for_sweep(graph: &LGraph, port: PortRef, forward: bool) -> Vec<PortRef> {
    let edge_indices = if forward {
        graph.layerless_nodes[port.node].ports[port.port].incoming_edges()
    } else {
        graph.layerless_nodes[port.node].ports[port.port].outgoing_edges()
    };

    edge_indices
        .iter()
        .filter_map(|edge| {
            if forward {
                graph.edges.get(*edge).map(|edge| edge.source())
            } else {
                graph.edges.get(*edge).map(|edge| edge.target())
            }
        })
        .collect()
}
