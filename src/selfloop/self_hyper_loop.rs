//! Port of ELK's `SelfHyperLoop`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/SelfHyperLoop.java

use crate::graph::{LGraph, NodeId, PortSide, SelfHyperLoop};

pub(crate) fn loop_sides(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
) -> Vec<PortSide> {
    let mut sides = hyper_loop
        .ports
        .iter()
        .map(|port| graph.layerless_nodes[node].ports[port.port].side)
        .filter(|side| *side != PortSide::Undefined)
        .collect::<Vec<_>>();
    sides.sort_by_key(|side| side.ordinal());
    sides.dedup();
    sides
}
