//! Phase 4: node placement, and the spacing between two nodes that its strategies share.

pub mod bk;
pub mod network_simplex;
pub mod simple;

use crate::graph::{LGraph, LNodeKind, NodeId};

pub use bk::place_nodes_brandes_koepf;
pub use network_simplex::place_nodes_network_simplex;
pub use simple::place_nodes_simple;

pub(crate) fn vertical_spacing(graph: &LGraph, first: NodeId, second: NodeId) -> f64 {
    use LNodeKind::*;

    match (
        graph.layerless_nodes[first].kind(),
        graph.layerless_nodes[second].kind(),
    ) {
        (Normal, Normal) | (Normal, Label) | (Label, Normal) => graph.options.spacing.node_node,
        (Normal, LongEdge)
        | (LongEdge, Normal)
        | (LongEdge, Label)
        | (Label, LongEdge)
        | (BreakingPoint, Normal)
        | (Normal, BreakingPoint)
        | (BreakingPoint, Label)
        | (Label, BreakingPoint)
        | (BreakingPoint, LongEdge)
        | (LongEdge, BreakingPoint) => graph.options.spacing.edge_node,
        (LongEdge, LongEdge)
        | (LongEdge, NorthSouthPort)
        | (NorthSouthPort, LongEdge)
        | (LongEdge, ExternalPort)
        | (ExternalPort, LongEdge)
        | (Label, Label)
        | (BreakingPoint, BreakingPoint) => graph.options.spacing.edge_edge,
        (Normal, NorthSouthPort)
        | (NorthSouthPort, Normal)
        | (Normal, ExternalPort)
        | (ExternalPort, Normal) => graph.options.spacing.edge_node,
        (NorthSouthPort, NorthSouthPort)
        | (NorthSouthPort, ExternalPort)
        | (ExternalPort, NorthSouthPort) => graph.options.spacing.edge_edge,
        (NorthSouthPort, Label) | (Label, NorthSouthPort) => graph.options.spacing.label_node,
        (ExternalPort, ExternalPort) => graph.options.spacing.port_port,
        (ExternalPort, Label) | (Label, ExternalPort) => graph.options.spacing.label_port_vertical,
        (BreakingPoint, ExternalPort)
        | (ExternalPort, BreakingPoint)
        | (BreakingPoint, NorthSouthPort)
        | (NorthSouthPort, BreakingPoint) => graph.options.spacing.edge_edge,
    }
}
