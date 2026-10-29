//! Port of ELK's `ModelOrderPortComparator`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/preserveorder/ModelOrderPortComparator.java

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::graph::{LGraph, NodeId, PortId, PortSide};
use crate::intermediate::port_list_sorter::port_side_order;
use crate::intermediate::preserveorder::model_order_node_comparator::update_bigger_and_smaller_associations;
use crate::options::OrderingStrategy;

pub(crate) struct ModelOrderPortComparator<'a> {
    graph: &'a LGraph,
    node: NodeId,
    previous_layer: &'a [NodeId],
    strategy: OrderingStrategy,
    target_node_model_order: &'a HashMap<NodeId, usize>,
    port_model_order: bool,
    pub(crate) bigger_than: HashMap<PortId, crate::gwt::LinkedSet<PortId>>,
    pub(crate) smaller_than: HashMap<PortId, crate::gwt::LinkedSet<PortId>>,
}

impl<'a> ModelOrderPortComparator<'a> {
    pub(crate) fn new(
        graph: &'a LGraph,
        node: NodeId,
        previous_layer: &'a [NodeId],
        strategy: OrderingStrategy,
        target_node_model_order: &'a HashMap<NodeId, usize>,
        port_model_order: bool,
    ) -> Self {
        Self {
            graph,
            node,
            previous_layer,
            strategy,
            target_node_model_order,
            port_model_order,
            bigger_than: HashMap::new(),
            smaller_than: HashMap::new(),
        }
    }

    pub(crate) fn compare(&mut self, original_p1: PortId, original_p2: PortId) -> Ordering {
        let mut p1 = original_p1;
        let mut p2 = original_p2;
        let p1_side = self.graph.layerless_nodes[self.node].ports[p1].side;
        let p2_side = self.graph.layerless_nodes[self.node].ports[p2].side;

        if self.port_model_order && p1_side == PortSide::West && p2_side == PortSide::West {
            std::mem::swap(&mut p1, &mut p2);
        }

        if let Some(ordering) = self.cached_ordering(p1, p2) {
            return ordering;
        }

        if self.graph.layerless_nodes[self.node].ports[p1].side
            != self.graph.layerless_nodes[self.node].ports[p2].side
        {
            let result = port_side_order(self.graph.layerless_nodes[self.node].ports[p1].side).cmp(
                &port_side_order(self.graph.layerless_nodes[self.node].ports[p2].side),
            );
            if result == Ordering::Less {
                self.update_bigger_smaller(p2, p1);
            } else {
                self.update_bigger_smaller(p1, p2);
            }
            return result;
        }

        if !self.graph.layerless_nodes[self.node].ports[p1]
            .incoming_edges()
            .is_empty()
            && !self.graph.layerless_nodes[self.node].ports[p2]
                .incoming_edges()
                .is_empty()
        {
            if self.port_model_order {
                let result = self.check_port_model_order(p1, p2);
                if result != Ordering::Equal {
                    self.update_from_result(p1, p2, result);
                    return result;
                }
            }

            let p1_node = self.incoming_source_node(p1);
            let p2_node = self.incoming_source_node(p2);
            if p1_node == p2_node {
                let p1_order = self.incoming_edge_order(p1);
                let p2_order = self.incoming_edge_order(p2);
                return self.compare_port_orders(p1, p2, p1_order, p2_order);
            }

            for previous_node in self.previous_layer {
                if Some(*previous_node) == p1_node {
                    self.update_bigger_smaller(p1, p2);
                    return Ordering::Greater;
                }
                if Some(*previous_node) == p2_node {
                    self.update_bigger_smaller(p2, p1);
                    return Ordering::Less;
                }
            }
        }

        if !self.graph.layerless_nodes[self.node].ports[p1]
            .outgoing_edges()
            .is_empty()
            && !self.graph.layerless_nodes[self.node].ports[p2]
                .outgoing_edges()
                .is_empty()
        {
            let p1_target = self.graph.layerless_nodes[self.node].ports[p1].long_edge_target_node;
            let p2_target = self.graph.layerless_nodes[self.node].ports[p2].long_edge_target_node;

            if self.strategy == OrderingStrategy::PreferNodes
                && let (Some(p1_target), Some(p2_target)) = (p1_target, p2_target)
                && let (Some(p1_order), Some(p2_order)) = (
                    self.graph.layerless_nodes[p1_target].model_order,
                    self.graph.layerless_nodes[p2_target].model_order,
                )
            {
                return self.compare_port_orders(p1, p2, p1_order, p2_order);
            }

            if self.port_model_order {
                let result = self.check_port_model_order(p1, p2);
                if result != Ordering::Equal {
                    self.update_from_result(p1, p2, result);
                    return result;
                }
            }

            let mut p1_order = self.outgoing_edge_order(p1);
            let mut p2_order = self.source_outgoing_edge_order_for_second_port(p1, p2);

            if p1_target.is_some() && p1_target == p2_target {
                let p1_reversed = self.outgoing_edge_reversed(p1);
                let p2_reversed = self.outgoing_edge_reversed(p2);
                if p1_reversed && !p2_reversed {
                    self.update_bigger_smaller(p1, p2);
                    return Ordering::Greater;
                }
                if !p1_reversed && p2_reversed {
                    self.update_bigger_smaller(p2, p1);
                    return Ordering::Less;
                }
                return self.compare_port_orders(p1, p2, p1_order, p2_order);
            }

            if let Some(target) = p1_target
                && let Some(order) = self.target_node_model_order.get(&target)
            {
                p1_order = *order;
            }
            if let Some(target) = p2_target
                && let Some(order) = self.target_node_model_order.get(&target)
            {
                p2_order = *order;
            }
            return self.compare_port_orders(p1, p2, p1_order, p2_order);
        }

        let p1_incoming = !self.graph.layerless_nodes[self.node].ports[p1]
            .incoming_edges()
            .is_empty();
        let p1_outgoing = !self.graph.layerless_nodes[self.node].ports[p1]
            .outgoing_edges()
            .is_empty();
        let p2_incoming = !self.graph.layerless_nodes[self.node].ports[p2]
            .incoming_edges()
            .is_empty();
        let p2_outgoing = !self.graph.layerless_nodes[self.node].ports[p2]
            .outgoing_edges()
            .is_empty();

        if p1_incoming && p2_outgoing {
            self.update_bigger_smaller(p1, p2);
            Ordering::Greater
        } else if p1_outgoing && p2_incoming {
            self.update_bigger_smaller(p2, p1);
            Ordering::Less
        } else if let (Some(p1_order), Some(p2_order)) = (
            self.graph.layerless_nodes[self.node].ports[p1].model_order,
            self.graph.layerless_nodes[self.node].ports[p2].model_order,
        ) {
            self.compare_port_orders(p1, p2, p1_order, p2_order)
        } else {
            self.update_bigger_smaller(p2, p1);
            Ordering::Less
        }
    }

    fn check_port_model_order(&self, p1: PortId, p2: PortId) -> Ordering {
        match (
            self.graph.layerless_nodes[self.node].ports[p1].model_order,
            self.graph.layerless_nodes[self.node].ports[p2].model_order,
        ) {
            (Some(p1_order), Some(p2_order)) => p1_order.cmp(&p2_order),
            _ => Ordering::Equal,
        }
    }

    fn incoming_source_node(&self, port: PortId) -> Option<NodeId> {
        self.graph.layerless_nodes[self.node].ports[port]
            .incoming_edges()
            .first()
            .map(|edge| self.graph.edges[*edge].source().node)
    }

    fn incoming_edge_order(&self, port: PortId) -> usize {
        self.graph.layerless_nodes[self.node].ports[port]
            .incoming_edges()
            .first()
            .and_then(|edge| self.graph.edges[*edge].model_order)
            .unwrap_or(0)
    }

    fn outgoing_edge_order(&self, port: PortId) -> usize {
        self.graph.layerless_nodes[self.node].ports[port]
            .outgoing_edges()
            .first()
            .and_then(|edge| self.graph.edges[*edge].model_order)
            .unwrap_or(0)
    }

    fn source_outgoing_edge_order_for_second_port(
        &self,
        first_port: PortId,
        second_port: PortId,
    ) -> usize {
        // This asymmetric lookup is source behavior: ELK checks whether p2 has model order, then
        // reads p1's value. It looks like a typo, but correcting it changes stable-sort decisions.
        if self.graph.layerless_nodes[self.node].ports[second_port]
            .outgoing_edges()
            .first()
            .and_then(|edge| self.graph.edges[*edge].model_order)
            .is_some()
        {
            self.outgoing_edge_order(first_port)
        } else {
            0
        }
    }

    fn outgoing_edge_reversed(&self, port: PortId) -> bool {
        self.graph.layerless_nodes[self.node].ports[port]
            .outgoing_edges()
            .first()
            .map(|edge| self.graph.edges[*edge].reversed)
            .unwrap_or(false)
    }

    fn compare_port_orders(
        &mut self,
        p1: PortId,
        p2: PortId,
        p1_order: usize,
        p2_order: usize,
    ) -> Ordering {
        if p1_order > p2_order {
            self.update_bigger_smaller(p1, p2);
        } else {
            self.update_bigger_smaller(p2, p1);
        }
        p1_order.cmp(&p2_order)
    }

    fn update_from_result(&mut self, p1: PortId, p2: PortId, result: Ordering) {
        if result == Ordering::Less {
            self.update_bigger_smaller(p2, p1);
        } else if result == Ordering::Greater {
            self.update_bigger_smaller(p1, p2);
        }
    }

    fn cached_ordering(&mut self, first: PortId, second: PortId) -> Option<Ordering> {
        self.ensure_port(first);
        self.ensure_port(second);
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

    fn ensure_port(&mut self, port: PortId) {
        self.bigger_than.entry(port).or_default();
        self.smaller_than.entry(port).or_default();
    }

    fn update_bigger_smaller(&mut self, bigger: PortId, smaller: PortId) {
        update_bigger_and_smaller_associations(
            bigger,
            smaller,
            &mut self.bigger_than,
            &mut self.smaller_than,
        );
    }
}
