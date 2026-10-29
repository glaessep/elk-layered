//! Port of ELK's `PortSideAssigner`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/ordering/PortSideAssigner.java

use crate::graph::{LGraph, PortSide, SelfLoopHolder};
use crate::options::SelfLoopDistributionStrategy;
use crate::selfloop::self_loop_port::self_loop_port_net_flow;

pub(crate) fn assign_hidden_port_sides(graph: &mut LGraph, holder: &SelfLoopHolder) {
    match graph
        .tables
        .self_loop_options
        .get_or_default(holder.node)
        .distribution
    {
        SelfLoopDistributionStrategy::North => assign_hidden_ports_to_north(graph, holder),
        SelfLoopDistributionStrategy::NorthSouth => {
            assign_hidden_ports_to_north_or_south(graph, holder)
        }
        SelfLoopDistributionStrategy::Equally => assign_hidden_ports_equally(graph, holder),
    }
}

fn assign_hidden_ports_to_north(graph: &mut LGraph, holder: &SelfLoopHolder) {
    for hyper_loop in &holder.hyper_loops {
        for port in hyper_loop.ports.iter().filter(|port| port.hidden) {
            graph.layerless_nodes[holder.node].ports[port.port].set_side(PortSide::North);
        }
    }
}

fn assign_hidden_ports_to_north_or_south(graph: &mut LGraph, holder: &SelfLoopHolder) {
    let mut north_ports = 0usize;
    let mut south_ports = 0usize;

    for hyper_loop in &holder.hyper_loops {
        let hidden_ports = hyper_loop
            .ports
            .iter()
            .filter(|port| port.hidden)
            .collect::<Vec<_>>();
        let side = if north_ports <= south_ports {
            north_ports += hidden_ports.len();
            PortSide::North
        } else {
            south_ports += hidden_ports.len();
            PortSide::South
        };

        for port in hidden_ports {
            graph.layerless_nodes[holder.node].ports[port.port].set_side(side);
        }
    }
}

fn assign_hidden_ports_equally(graph: &mut LGraph, holder: &SelfLoopHolder) {
    let mut sorted_loop_indices = (0..holder.hyper_loops.len()).collect::<Vec<_>>();
    sorted_loop_indices.sort_by_key(|loop_index| {
        (
            std::cmp::Reverse(holder.hyper_loops[*loop_index].ports.len()),
            *loop_index,
        )
    });

    for (assignment_index, loop_index) in sorted_loop_indices.into_iter().enumerate() {
        let hyper_loop = &holder.hyper_loops[loop_index];
        let target = EqualDistributionTarget::VALUES
            [assignment_index % EqualDistributionTarget::VALUES.len()];
        let mut hidden_ports = hyper_loop
            .ports
            .iter()
            .filter(|port| port.hidden)
            .collect::<Vec<_>>();

        if hidden_ports.is_empty() {
            continue;
        }

        if target.is_corner() {
            hidden_ports.sort_by_key(|port| self_loop_port_net_flow(hyper_loop, port.port));
        }

        let second_half_start = hidden_ports.len() / 2;
        for (index, port) in hidden_ports.into_iter().enumerate() {
            let side = if index < second_half_start {
                target.first_side
            } else {
                target.second_side
            };
            graph.layerless_nodes[holder.node].ports[port.port].set_side(side);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EqualDistributionTarget {
    first_side: PortSide,
    second_side: PortSide,
}

impl EqualDistributionTarget {
    const VALUES: [Self; 8] = [
        Self::new(PortSide::North, PortSide::North),
        Self::new(PortSide::South, PortSide::South),
        Self::new(PortSide::East, PortSide::East),
        Self::new(PortSide::West, PortSide::West),
        Self::new(PortSide::West, PortSide::North),
        Self::new(PortSide::North, PortSide::East),
        Self::new(PortSide::South, PortSide::West),
        Self::new(PortSide::East, PortSide::South),
    ];

    const fn new(first_side: PortSide, second_side: PortSide) -> Self {
        Self {
            first_side,
            second_side,
        }
    }

    fn is_corner(self) -> bool {
        self.first_side != self.second_side
    }
}
