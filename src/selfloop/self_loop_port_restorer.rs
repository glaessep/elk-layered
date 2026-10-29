//! Port of ELK's `SelfLoopPortRestorer`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/SelfLoopPortRestorer.java

use crate::graph::{LGraph, OtherGraphs, PortSide, SelfLoopHolder};
use crate::selfloop::ordering::port_restorer::restore_hidden_ports;
use crate::selfloop::ordering::port_side_assigner::assign_hidden_port_sides;
use crate::selfloop::self_loop_pre_processor::unhide_ports;
use crate::selfloop::self_loop_type::self_loop_type_from_sides;

pub fn restore_self_loop_ports(graph: &mut LGraph, others: OtherGraphs<'_>) {
    let mut holders = std::mem::take(&mut graph.self_loop_holders);
    for holder in &mut holders {
        unhide_ports(graph, holder);
        if holder.ports_hidden && !holder.original_port_constraints.is_side_fixed() {
            assign_hidden_port_sides(graph, holder);
        }
        compute_self_loop_types(graph, holder);
        if holder.ports_hidden {
            restore_hidden_ports(graph, others, holder);
        }
    }
    graph.self_loop_holders = holders;
}

fn compute_self_loop_types(graph: &LGraph, holder: &mut SelfLoopHolder) {
    for hyper_loop in &mut holder.hyper_loops {
        let mut sides = hyper_loop
            .ports
            .iter()
            .map(|port| graph.layerless_nodes[holder.node].ports[port.port].side)
            .filter(|side| *side != PortSide::Undefined)
            .collect::<Vec<_>>();
        sides.sort_by_key(|side| side.ordinal());
        sides.dedup();

        hyper_loop.self_loop_type = self_loop_type_from_sides(&sides);
    }
}
