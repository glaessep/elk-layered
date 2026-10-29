//! Port of ELK's `ModelOrderNodeComparator`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/preserveorder/ModelOrderNodeComparator.java

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::graph::{LGraph, NodeId, PortRef};
use crate::options::OrderingStrategy;

pub(crate) struct ModelOrderNodeComparator<'a> {
    graph: &'a LGraph,
    previous_layer: &'a [NodeId],
    ordering_strategy: OrderingStrategy,
    bigger_than: HashMap<NodeId, crate::gwt::LinkedSet<NodeId>>,
    smaller_than: HashMap<NodeId, crate::gwt::LinkedSet<NodeId>>,
}

impl<'a> ModelOrderNodeComparator<'a> {
    pub(crate) fn new(
        graph: &'a LGraph,
        previous_layer: &'a [NodeId],
        ordering_strategy: OrderingStrategy,
    ) -> Self {
        Self {
            graph,
            previous_layer,
            ordering_strategy,
            bigger_than: HashMap::new(),
            smaller_than: HashMap::new(),
        }
    }

    pub(crate) fn compare(&mut self, n1: NodeId, n2: NodeId) -> Ordering {
        if let Some(ordering) = self.cached_ordering(n1, n2) {
            return ordering;
        }

        if self.ordering_strategy == OrderingStrategy::PreferEdges
            || self.graph.layerless_nodes[n1].model_order.is_none()
            || self.graph.layerless_nodes[n2].model_order.is_none()
        {
            let p1_source_port = last_previous_layer_source_port(self.graph, n1);
            let p2_source_port = last_previous_layer_source_port(self.graph, n2);

            if let (Some(p1), Some(p2)) = (p1_source_port, p2_source_port) {
                if p1.node == p2.node {
                    for port_index in self.graph.layerless_nodes[p1.node].ports.ids() {
                        if port_index == p1.port {
                            self.update_bigger_smaller(n2, n1);
                            return Ordering::Less;
                        }
                        if port_index == p2.port {
                            self.update_bigger_smaller(n1, n2);
                            return Ordering::Greater;
                        }
                    }
                }

                for previous_node in self.previous_layer {
                    if *previous_node == p1.node {
                        self.update_bigger_smaller(n2, n1);
                        return Ordering::Less;
                    }
                    if *previous_node == p2.node {
                        self.update_bigger_smaller(n1, n2);
                        return Ordering::Greater;
                    }
                }
            }

            if self.graph.layerless_nodes[n1].model_order.is_none()
                || self.graph.layerless_nodes[n2].model_order.is_none()
            {
                return self.compare_node_model_orders(
                    n1,
                    n2,
                    self.model_order_from_connected_edges(n1),
                    self.model_order_from_connected_edges(n2),
                );
            }
        }

        self.compare_node_model_orders(
            n1,
            n2,
            self.graph.layerless_nodes[n1].model_order.unwrap_or(0) as i64,
            self.graph.layerless_nodes[n2].model_order.unwrap_or(0) as i64,
        )
    }

    fn compare_node_model_orders(
        &mut self,
        n1: NodeId,
        n2: NodeId,
        n1_order: i64,
        n2_order: i64,
    ) -> Ordering {
        if n1_order > n2_order {
            self.update_bigger_smaller(n1, n2);
        } else {
            self.update_bigger_smaller(n2, n1);
        }
        n1_order.cmp(&n2_order)
    }

    fn model_order_from_connected_edges(&self, node: NodeId) -> i64 {
        for port in &self.graph.layerless_nodes[node].ports {
            if let Some(edge) = port.incoming_edges().first() {
                return self.graph.edges[*edge].model_order.unwrap_or(0) as i64;
            }
        }
        self.graph
            .options
            .consider_model_order_long_edge_strategy
            .return_value()
    }

    fn cached_ordering(&mut self, first: NodeId, second: NodeId) -> Option<Ordering> {
        self.ensure_node(first);
        self.ensure_node(second);
        if self.bigger_than[&first].contains(&second) {
            return Some(Ordering::Greater);
        }
        if self.bigger_than[&second].contains(&first) {
            return Some(Ordering::Less);
        }
        if self.smaller_than[&first].contains(&second) {
            return Some(Ordering::Less);
        }
        if self.smaller_than[&second].contains(&first) {
            return Some(Ordering::Greater);
        }
        None
    }

    fn ensure_node(&mut self, node: NodeId) {
        self.bigger_than.entry(node).or_default();
        self.smaller_than.entry(node).or_default();
    }

    fn update_bigger_smaller(&mut self, bigger: NodeId, smaller: NodeId) {
        update_bigger_and_smaller_associations(
            bigger,
            smaller,
            &mut self.bigger_than,
            &mut self.smaller_than,
        );
    }
}

pub(crate) fn last_previous_layer_source_port(graph: &LGraph, node: NodeId) -> Option<PortRef> {
    let mut selected = None;
    for port in &graph.layerless_nodes[node].ports {
        if let Some(edge) = port.incoming_edges().first() {
            let source = graph.edges[*edge].source();
            if graph.layerless_nodes[source.node].layer_index()
                != graph.layerless_nodes[node].layer_index()
            {
                // Upstream deliberately keeps scanning, so the last qualifying incoming port is
                // the comparator's source descriptor. Returning early changes model-order ties.
                selected = Some(source);
            }
        }
    }
    selected
}

pub(crate) fn update_bigger_and_smaller_associations<T>(
    bigger: T,
    smaller: T,
    bigger_than: &mut HashMap<T, crate::gwt::LinkedSet<T>>,
    smaller_than: &mut HashMap<T, crate::gwt::LinkedSet<T>>,
) where
    T: Copy + Eq + std::hash::Hash,
{
    bigger_than.entry(bigger).or_default();
    bigger_than.entry(smaller).or_default();
    smaller_than.entry(bigger).or_default();
    smaller_than.entry(smaller).or_default();

    // The sets iterate as elkjs's do, in insertion order: see `crate::gwt::LinkedSet`.
    let smaller_bigger_than = bigger_than[&smaller].to_vec();
    let bigger_smaller_than = smaller_than[&bigger].to_vec();

    bigger_than.get_mut(&bigger).unwrap().insert(smaller);
    smaller_than.get_mut(&smaller).unwrap().insert(bigger);

    for very_small in smaller_bigger_than {
        bigger_than.get_mut(&bigger).unwrap().insert(very_small);
        smaller_than.entry(very_small).or_default().insert(bigger);
        let bigger_smaller_than = smaller_than[&bigger].to_vec();
        smaller_than
            .get_mut(&very_small)
            .unwrap()
            .extend(bigger_smaller_than);
    }

    for very_big in bigger_smaller_than {
        smaller_than.get_mut(&smaller).unwrap().insert(very_big);
        bigger_than.entry(very_big).or_default().insert(smaller);
        let smaller_bigger_than = bigger_than[&smaller].to_vec();
        bigger_than
            .get_mut(&very_big)
            .unwrap()
            .extend(smaller_bigger_than);
    }
}
