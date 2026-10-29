//! Port of ELK's `RoutingDirector`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/routing/RoutingDirector.java

use crate::graph::{
    LGraph, NodeId, NodePorts, PortId, PortSide, SelfHyperLoop, SelfLoopHolder, SelfLoopType,
};
use crate::selfloop::ordering::port_restorer::sorted_two_side_loop_port_sides;
use crate::selfloop::self_hyper_loop::loop_sides;

const UNCONNECTED_PORT_PENALTY: usize = 1;

const CONNECTED_PORT_PENALTY: usize = 3;

/// Where each port stands in its node's port list, read once so that a lookup is not a scan.
///
/// A port the node does not list has no place. Callers decide what that means for them, and
/// none panics: a self-loop port is back in the list once the ports are restored, but this table
/// does not rely on it.
pub(crate) struct PortPlaces {
    /// The place of each port, indexed by `PortId::index`.
    places: Vec<Option<usize>>,
    /// How many ports the list holds.
    pub(crate) listed: usize,
}

impl PortPlaces {
    pub(crate) fn new(ports: &NodePorts) -> Self {
        let mut places = vec![None; ports.slot_count()];
        for (place, id) in ports.ids().enumerate() {
            places[id.index()] = Some(place);
        }
        Self {
            places,
            listed: ports.len(),
        }
    }

    /// The place of this port in the list, or `None` if the list does not hold it.
    pub(crate) fn of(&self, port: PortId) -> Option<usize> {
        self.places.get(port.index()).copied().flatten()
    }
}

pub(crate) fn determine_loop_routes(
    graph: &LGraph,
    holder: &mut SelfLoopHolder,
    places: &PortPlaces,
) {
    let port_penalties = compute_port_penalties(graph, holder.node);

    for hyper_loop in &mut holder.hyper_loops {
        // Java's `slPortsBySide.keySet()` lists the sides in the order the loop's ports were
        // added, so read it before the ports are sorted by id.
        let sides_in_port_order = loop_sides_in_port_order(graph, holder.node, hyper_loop);
        hyper_loop.ports.sort_by_key(|port| places.of(port.port));

        let Some(loop_type) = hyper_loop.self_loop_type else {
            continue;
        };

        match loop_type {
            SelfLoopType::OneSide => {
                let side = graph.layerless_nodes[holder.node].ports[hyper_loop.ports[0].port].side;
                assign_leftmost_rightmost_ports(graph, places, holder.node, hyper_loop, side, side);
            }
            SelfLoopType::TwoSidesCorner => {
                if let Some((left, right)) =
                    sorted_two_side_loop_port_sides(graph, holder.node, hyper_loop)
                {
                    assign_leftmost_rightmost_ports(
                        graph,
                        places,
                        holder.node,
                        hyper_loop,
                        left,
                        right,
                    );
                }
            }
            SelfLoopType::TwoSidesOpposing => {
                determine_two_side_opposing_loop_route(
                    graph,
                    places,
                    holder.node,
                    hyper_loop,
                    &sides_in_port_order,
                    &port_penalties,
                );
            }
            SelfLoopType::ThreeSides => {
                if let Some((left, right)) = three_side_route_sides(graph, holder.node, hyper_loop)
                {
                    assign_leftmost_rightmost_ports(
                        graph,
                        places,
                        holder.node,
                        hyper_loop,
                        left,
                        right,
                    );
                }
            }
            SelfLoopType::FourSides => {
                determine_four_side_loop_route(places, hyper_loop, &port_penalties);
            }
        }

        compute_occupied_sides(graph, holder.node, hyper_loop);
    }
}

// Java: RoutingDirector.determineTwoSideOpposingLoopRoutes. Java takes `sides` from
// `slLoop.getSLPortsBySide().keySet().toArray(...)`, a multimap over a hash map. elkjs (GWT)
// iterates that map in insertion order, so `sides` keeps the order of the loop's ports and is not
// sorted. A tie between the two options goes to option 1, so this order picks the loop's route.
fn determine_two_side_opposing_loop_route(
    graph: &LGraph,
    places: &PortPlaces,
    node: NodeId,
    hyper_loop: &mut SelfHyperLoop,
    sides: &[PortSide],
    port_penalties: &[usize],
) {
    if sides.len() != 2 {
        return;
    }

    let Some(option1_leftmost_port) =
        lowest_port_on_side(graph, places, node, hyper_loop, sides[0])
    else {
        return;
    };
    let Some(option1_rightmost_port) =
        highest_port_on_side(graph, places, node, hyper_loop, sides[1])
    else {
        return;
    };
    let option1_penalty = compute_edge_penalty(
        places,
        option1_leftmost_port,
        option1_rightmost_port,
        port_penalties,
    );

    let Some(option2_leftmost_port) =
        lowest_port_on_side(graph, places, node, hyper_loop, sides[1])
    else {
        return;
    };
    let Some(option2_rightmost_port) =
        highest_port_on_side(graph, places, node, hyper_loop, sides[0])
    else {
        return;
    };
    let option2_penalty = compute_edge_penalty(
        places,
        option2_leftmost_port,
        option2_rightmost_port,
        port_penalties,
    );

    if option1_penalty <= option2_penalty {
        hyper_loop.leftmost_port = Some(option1_leftmost_port);
        hyper_loop.rightmost_port = Some(option1_rightmost_port);
    } else {
        hyper_loop.leftmost_port = Some(option2_leftmost_port);
        hyper_loop.rightmost_port = Some(option2_rightmost_port);
    }
}

fn determine_four_side_loop_route(
    places: &PortPlaces,
    hyper_loop: &mut SelfHyperLoop,
    port_penalties: &[usize],
) {
    let Some(mut worst_left_port) = hyper_loop.ports.last().map(|port| port.port) else {
        return;
    };
    let Some(mut worst_right_port) = hyper_loop.ports.first().map(|port| port.port) else {
        return;
    };
    let mut worst_penalty =
        compute_edge_penalty(places, worst_left_port, worst_right_port, port_penalties);

    for ports in hyper_loop.ports.windows(2) {
        let current_left_port = ports[0].port;
        let current_right_port = ports[1].port;
        let current_penalty = compute_edge_penalty(
            places,
            current_left_port,
            current_right_port,
            port_penalties,
        );

        if current_penalty > worst_penalty {
            worst_left_port = current_left_port;
            worst_right_port = current_right_port;
            worst_penalty = current_penalty;
        }
    }

    hyper_loop.leftmost_port = Some(worst_right_port);
    hyper_loop.rightmost_port = Some(worst_left_port);
}

fn three_side_route_sides(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
) -> Option<(PortSide, PortSide)> {
    let sides = loop_sides(graph, node, hyper_loop);
    if sides.len() != 3 {
        return None;
    }

    if !sides.contains(&PortSide::North) {
        Some((PortSide::East, PortSide::West))
    } else if !sides.contains(&PortSide::East) {
        Some((PortSide::South, PortSide::North))
    } else if !sides.contains(&PortSide::South) {
        Some((PortSide::West, PortSide::East))
    } else if !sides.contains(&PortSide::West) {
        Some((PortSide::North, PortSide::South))
    } else {
        None
    }
}

/// The distinct sides of the loop's ports, each at its first port. Java: the key order of
/// `SelfHyperLoop.slPortsBySide` as elkjs iterates it.
fn loop_sides_in_port_order(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
) -> Vec<PortSide> {
    let mut sides = Vec::new();
    for port in &hyper_loop.ports {
        let side = graph.layerless_nodes[node].ports[port.port].side;
        if side != PortSide::Undefined && !sides.contains(&side) {
            sides.push(side);
        }
    }
    sides
}

fn assign_leftmost_rightmost_ports(
    graph: &LGraph,
    places: &PortPlaces,
    node: NodeId,
    hyper_loop: &mut SelfHyperLoop,
    leftmost_side: PortSide,
    rightmost_side: PortSide,
) {
    hyper_loop.leftmost_port = hyper_loop
        .ports
        .iter()
        .filter(|port| graph.layerless_nodes[node].ports[port.port].side == leftmost_side)
        .map(|port| port.port)
        .min_by_key(|port| places.of(*port));
    hyper_loop.rightmost_port = hyper_loop
        .ports
        .iter()
        .filter(|port| graph.layerless_nodes[node].ports[port.port].side == rightmost_side)
        .map(|port| port.port)
        .max_by_key(|port| places.of(*port));
}

fn lowest_port_on_side(
    graph: &LGraph,
    places: &PortPlaces,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
    side: PortSide,
) -> Option<PortId> {
    hyper_loop
        .ports
        .iter()
        .filter(|port| graph.layerless_nodes[node].ports[port.port].side == side)
        .map(|port| port.port)
        .min_by_key(|port| places.of(*port))
}

fn highest_port_on_side(
    graph: &LGraph,
    places: &PortPlaces,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
    side: PortSide,
) -> Option<PortId> {
    hyper_loop
        .ports
        .iter()
        .filter(|port| graph.layerless_nodes[node].ports[port.port].side == side)
        .map(|port| port.port)
        .max_by_key(|port| places.of(*port))
}

fn compute_port_penalties(graph: &LGraph, node: NodeId) -> Vec<usize> {
    let mut penalty_sum = 0usize;
    graph.layerless_nodes[node]
        .ports
        .iter()
        .map(|port| {
            if port.incoming_edges().is_empty() && port.outgoing_edges().is_empty() {
                penalty_sum += UNCONNECTED_PORT_PENALTY;
            } else {
                penalty_sum += CONNECTED_PORT_PENALTY;
            }
            penalty_sum
        })
        .collect()
}

pub(crate) fn compute_edge_penalty(
    places: &PortPlaces,
    leftmost_port: PortId,
    rightmost_port: PortId,
    port_penalties: &[usize],
) -> usize {
    let port_count = places.listed;
    // A port off the list has no place, and so no penalty. A place is below `port_count` and
    // `port_count` is above 0 whenever there is a place, so no other range check is needed.
    let (Some(leftmost_port), Some(rightmost_port)) =
        (places.of(leftmost_port), places.of(rightmost_port))
    else {
        return 0;
    };
    if port_penalties.len() != port_count {
        return 0;
    }

    let left_of_rightmost_port = if rightmost_port == 0 {
        port_count - 1
    } else {
        rightmost_port - 1
    };

    if leftmost_port <= left_of_rightmost_port {
        port_penalties[left_of_rightmost_port] - port_penalties[leftmost_port]
    } else {
        port_penalties[port_count - 1] - port_penalties[leftmost_port]
            + port_penalties[left_of_rightmost_port]
    }
}

fn compute_occupied_sides(graph: &LGraph, node: NodeId, hyper_loop: &mut SelfHyperLoop) {
    hyper_loop.occupied_sides.clear();

    let (Some(leftmost_port), Some(rightmost_port)) =
        (hyper_loop.leftmost_port, hyper_loop.rightmost_port)
    else {
        return;
    };

    let mut current = graph.layerless_nodes[node].ports[leftmost_port].side;
    let target = graph.layerless_nodes[node].ports[rightmost_port].side;
    if current == PortSide::Undefined || target == PortSide::Undefined {
        return;
    }

    loop {
        if !hyper_loop.occupied_sides.contains(&current) {
            hyper_loop.occupied_sides.push(current);
        }
        if current == target {
            break;
        }
        current = current.right();
    }
}
