//! Port of ELK's `HyperEdgeSegment`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/HyperEdgeSegment.java

use std::collections::HashMap;

use crate::graph::{LGraph, PortRef};
use crate::phase::routing::orthogonal::direction::routing_direction::RoutingDirection;
use crate::phase::routing::orthogonal::hyper_edge_segment_dependency::HyperEdgeGraph;

#[derive(Debug, Clone, PartialEq)]
pub struct HyperEdgeSegment {
    pub ports: Vec<PortRef>,
    pub mark: i32,
    pub routing_slot: i32,
    start_position: f64,
    end_position: f64,
    pub incoming_connection_coordinates: Vec<f64>,
    pub outgoing_connection_coordinates: Vec<f64>,
    pub outgoing_segment_dependencies: Vec<usize>,
    pub out_dep_weight: i32,
    pub critical_out_dep_weight: i32,
    pub incoming_segment_dependencies: Vec<usize>,
    pub in_dep_weight: i32,
    pub critical_in_dep_weight: i32,
    pub split_partner: Option<usize>,
    pub split_by: Option<usize>,
}

impl HyperEdgeSegment {
    pub fn new() -> Self {
        Self {
            ports: Vec::new(),
            mark: 0,
            routing_slot: 0,
            start_position: f64::NAN,
            end_position: f64::NAN,
            incoming_connection_coordinates: Vec::new(),
            outgoing_connection_coordinates: Vec::new(),
            outgoing_segment_dependencies: Vec::new(),
            out_dep_weight: 0,
            critical_out_dep_weight: 0,
            incoming_segment_dependencies: Vec::new(),
            in_dep_weight: 0,
            critical_in_dep_weight: 0,
            split_partner: None,
            split_by: None,
        }
    }

    #[cfg(test)]
    pub fn with_incoming_outgoing(
        incoming: impl Into<Vec<f64>>,
        outgoing: impl Into<Vec<f64>>,
    ) -> Self {
        let mut segment = Self::new();
        segment.incoming_connection_coordinates = incoming.into();
        segment.outgoing_connection_coordinates = outgoing.into();
        sort_dedup_f64(&mut segment.incoming_connection_coordinates);
        sort_dedup_f64(&mut segment.outgoing_connection_coordinates);
        segment.recompute_extent();
        segment
    }

    pub fn start_coordinate(&self) -> f64 {
        self.start_position
    }

    pub fn end_coordinate(&self) -> f64 {
        self.end_position
    }

    pub fn length(&self) -> f64 {
        self.end_coordinate() - self.start_coordinate()
    }

    pub fn represents_hyperedge(&self) -> bool {
        self.incoming_connection_coordinates.len() + self.outgoing_connection_coordinates.len() > 2
    }

    pub fn is_dummy(&self) -> bool {
        self.split_partner.is_some() && self.split_by.is_none()
    }

    pub fn simulate_split(&self) -> (Self, Self) {
        let mut split_segment = Self::new();
        split_segment.incoming_connection_coordinates =
            self.incoming_connection_coordinates.clone();
        split_segment.split_by = self.split_by;
        split_segment.split_partner = Some(1);
        split_segment.recompute_extent();

        let mut split_partner = Self::new();
        split_partner.outgoing_connection_coordinates =
            self.outgoing_connection_coordinates.clone();
        split_partner.split_partner = Some(0);
        split_partner.recompute_extent();

        (split_segment, split_partner)
    }

    pub fn insert_incoming(&mut self, value: f64) {
        insert_sorted_unique(&mut self.incoming_connection_coordinates, value);
        self.recompute_extent();
    }

    pub fn insert_outgoing(&mut self, value: f64) {
        insert_sorted_unique(&mut self.outgoing_connection_coordinates, value);
        self.recompute_extent();
    }

    pub fn recompute_extent(&mut self) {
        self.start_position = f64::NAN;
        self.end_position = f64::NAN;
        self.recompute_extent_for(true);
        self.recompute_extent_for(false);
    }

    fn recompute_extent_for(&mut self, incoming: bool) {
        let positions = if incoming {
            &self.incoming_connection_coordinates
        } else {
            &self.outgoing_connection_coordinates
        };
        let Some(first) = positions.first().copied() else {
            return;
        };
        let last = positions.last().copied().unwrap_or(first);

        if self.start_position.is_nan() {
            self.start_position = first;
        } else {
            self.start_position = self.start_position.min(first);
        }

        if self.end_position.is_nan() {
            self.end_position = last;
        } else {
            self.end_position = self.end_position.max(last);
        }
    }
}

impl Default for HyperEdgeSegment {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn add_port_positions(
    graph: &LGraph,
    direction: RoutingDirection,
    port_ref: PortRef,
    hyper_graph: &mut HyperEdgeGraph,
    segment: usize,
    port_to_segment: &mut HashMap<PortRef, usize>,
) {
    port_to_segment.insert(port_ref, segment);
    hyper_graph.segments[segment].ports.push(port_ref);
    let port_position = direction.port_position_on_hypernode(graph, port_ref);
    let port = &graph.layerless_nodes[port_ref.node].ports[port_ref.port];

    if port.side == direction.source_port_side() {
        hyper_graph.segments[segment].insert_incoming(port_position);
    } else {
        hyper_graph.segments[segment].insert_outgoing(port_position);
    }

    for other_port in connected_ports(graph, port_ref) {
        if !port_to_segment.contains_key(&other_port) {
            add_port_positions(
                graph,
                direction,
                other_port,
                hyper_graph,
                segment,
                port_to_segment,
            );
        }
    }
}

fn connected_ports(graph: &LGraph, port_ref: PortRef) -> Vec<PortRef> {
    let Some(node) = graph.layerless_nodes.get(port_ref.node) else {
        return Vec::new();
    };
    let Some(port) = node.ports.get(port_ref.port) else {
        return Vec::new();
    };

    port.incoming_edges()
        .iter()
        .filter_map(|edge| graph.edges.get(*edge).map(|edge| edge.source()))
        .chain(
            port.outgoing_edges()
                .iter()
                .filter_map(|edge| graph.edges.get(*edge).map(|edge| edge.target())),
        )
        .filter(|other| *other != port_ref)
        .collect()
}

fn insert_sorted_unique(list: &mut Vec<f64>, value: f64) {
    match list.binary_search_by(|probe| probe.total_cmp(&value)) {
        Ok(_) => {}
        Err(index) => list.insert(index, value),
    }
}

pub(crate) fn sort_dedup_f64(values: &mut Vec<f64>) {
    values.sort_by(|left, right| left.total_cmp(right));
    values.dedup_by(|left, right| left == right);
}

// `HyperEdgeSegment.splitAt`. Rust keeps segments in `HyperEdgeGraph`, so the split is a method
// of the graph, which owns the segment and its partner.
impl HyperEdgeGraph {
    pub fn split_segment_at(&mut self, segment: usize, split_position: f64) -> usize {
        let split_partner = self.segments.len();
        let mut partner = HyperEdgeSegment::new();
        partner.split_partner = Some(segment);
        partner.outgoing_connection_coordinates =
            std::mem::take(&mut self.segments[segment].outgoing_connection_coordinates);
        partner.incoming_connection_coordinates.push(split_position);
        self.segments[segment]
            .outgoing_connection_coordinates
            .push(split_position);
        self.segments[segment].split_partner = Some(split_partner);

        self.segments[segment].recompute_extent();
        partner.recompute_extent();
        self.segments.push(partner);

        let incoming = self.segments[segment].incoming_segment_dependencies.clone();
        let outgoing = self.segments[segment].outgoing_segment_dependencies.clone();

        for dependency in incoming.into_iter().chain(outgoing) {
            self.remove_dependency(dependency);
        }

        split_partner
    }
}
