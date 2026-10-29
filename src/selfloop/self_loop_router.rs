//! Port of ELK's `SelfLoopRouter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/SelfLoopRouter.java

use crate::graph::LGraph;
use crate::selfloop::routing::label_placer::place_self_loop_labels;
use crate::selfloop::routing::orthogonal_self_loop_router::route_self_loop_holder;
use crate::selfloop::routing::routing_director::{PortPlaces, determine_loop_routes};
use crate::selfloop::routing::routing_slot_assigner::assign_routing_slots;

pub fn route_self_loops(graph: &mut LGraph) {
    let mut holders = std::mem::take(&mut graph.self_loop_holders);
    for holder in &mut holders {
        // None of the four steps lists, unlists or reorders the node's ports (they write bend
        // points, label positions, slots and margins), so one table serves them all.
        let places = PortPlaces::new(&graph.layerless_nodes[holder.node].ports);
        determine_loop_routes(graph, holder, &places);
        place_self_loop_labels(graph, holder);
        assign_routing_slots(graph, holder, &places);
        route_self_loop_holder(graph, holder, &places);
    }
    graph.self_loop_holders = holders;
}
