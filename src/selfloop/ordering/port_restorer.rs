//! Port of ELK's `PortRestorer`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/ordering/PortRestorer.java

use crate::graph::{
    GraphId, LGraph, NodeId, OtherGraphs, PortId, PortSide, SelfHyperLoop, SelfLoopHolder,
    SelfLoopPort, SelfLoopType,
};
use crate::options::SelfLoopOrderingStrategy;
use crate::selfloop::self_hyper_loop::loop_sides;
use crate::selfloop::self_loop_port::self_loop_port_net_flow;

pub(crate) fn restore_hidden_ports(
    graph: &mut LGraph,
    others: OtherGraphs<'_>,
    holder: &mut SelfLoopHolder,
) {
    let mut target_areas = PortRestoreTargetAreas::new();
    collect_restore_target_areas(graph, holder, &mut target_areas);

    let new_order = restored_port_order(graph, others, holder.node, &target_areas);
    if !graph.reorder_node_ports(holder.node, new_order) {
        return;
    }

    holder.ports_hidden = false;
    for hyper_loop in &mut holder.hyper_loops {
        for port in &mut hyper_loop.ports {
            port.hidden = false;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortSideArea {
    Start,
    Middle,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddMode {
    Prepend,
    Append,
}

#[derive(Debug, Clone)]
pub(crate) struct PortRestoreTargetAreas {
    areas: [[Vec<PortId>; 3]; 5],
}

impl PortRestoreTargetAreas {
    pub(crate) fn new() -> Self {
        Self {
            areas: std::array::from_fn(|_| std::array::from_fn(|_| Vec::new())),
        }
    }

    fn add_ports(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        hyper_loop: &SelfHyperLoop,
        side: PortSide,
        area: PortSideArea,
        mode: AddMode,
    ) {
        let ports = hyper_loop
            .ports
            .iter()
            .filter(|port| port.hidden && graph.layerless_nodes[node].ports[port.port].side == side)
            .map(|port| port.port)
            .collect::<Vec<_>>();
        self.add_port_list(ports, side, area, mode);
    }

    pub(crate) fn add_port_list(
        &mut self,
        mut ports: Vec<PortId>,
        side: PortSide,
        area: PortSideArea,
        mode: AddMode,
    ) {
        ports.reverse();
        let target = &mut self.areas[side.ordinal()][area.ordinal()];
        match mode {
            AddMode::Prepend => {
                target.splice(0..0, ports);
            }
            AddMode::Append => target.extend(ports),
        }
    }

    pub(crate) fn get(&self, side: PortSide, area: PortSideArea) -> &[PortId] {
        &self.areas[side.ordinal()][area.ordinal()]
    }

    fn ports(&self) -> Vec<PortId> {
        self.areas
            .iter()
            .flat_map(|side_areas| side_areas.iter())
            .flat_map(|ports| ports.iter().copied())
            .collect()
    }
}

impl PortSideArea {
    fn ordinal(self) -> usize {
        match self {
            Self::Start => 0,
            Self::Middle => 1,
            Self::End => 2,
        }
    }
}

fn collect_restore_target_areas(
    graph: &LGraph,
    holder: &SelfLoopHolder,
    target_areas: &mut PortRestoreTargetAreas,
) {
    process_one_side_loops(graph, holder, target_areas);
    process_two_side_corner_loops(graph, holder, target_areas);
    process_three_side_loops(graph, holder, target_areas);
    process_four_side_loops(graph, holder, target_areas);
    process_two_side_opposing_loops(graph, holder, target_areas);
}

fn process_one_side_loops(
    graph: &LGraph,
    holder: &SelfLoopHolder,
    target_areas: &mut PortRestoreTargetAreas,
) {
    let mut one_side_loops = holder
        .hyper_loops
        .iter()
        .filter(|hyper_loop| hyper_loop.self_loop_type == Some(SelfLoopType::OneSide))
        .collect::<Vec<_>>();
    if graph
        .tables
        .self_loop_options
        .get_or_default(holder.node)
        .ordering
        == SelfLoopOrderingStrategy::ReverseStacked
    {
        one_side_loops.reverse();
    }

    for hyper_loop in one_side_loops {
        let Some(side) = hyper_loop
            .ports
            .first()
            .map(|port| graph.layerless_nodes[holder.node].ports[port.port].side)
        else {
            continue;
        };

        let mut sorted_ports = hyper_loop.ports.clone();
        sorted_ports.sort_by_key(|port| self_loop_port_net_flow(hyper_loop, port.port));
        match graph
            .tables
            .self_loop_options
            .get_or_default(holder.node)
            .ordering
        {
            SelfLoopOrderingStrategy::Sequenced => {
                target_areas.add_port_list(
                    sorted_ports
                        .iter()
                        .filter(|port| port.hidden)
                        .map(|port| port.port)
                        .collect(),
                    side,
                    PortSideArea::Middle,
                    AddMode::Append,
                );
            }
            SelfLoopOrderingStrategy::Stacked | SelfLoopOrderingStrategy::ReverseStacked => {
                let split_index = compute_port_list_split_index(hyper_loop, &sorted_ports);
                target_areas.add_port_list(
                    sorted_ports[..split_index]
                        .iter()
                        .filter(|port| port.hidden)
                        .map(|port| port.port)
                        .collect(),
                    side,
                    PortSideArea::Middle,
                    AddMode::Prepend,
                );
                target_areas.add_port_list(
                    sorted_ports[split_index..]
                        .iter()
                        .filter(|port| port.hidden)
                        .map(|port| port.port)
                        .collect(),
                    side,
                    PortSideArea::Middle,
                    AddMode::Append,
                );
            }
        }
    }
}

fn process_two_side_corner_loops(
    graph: &LGraph,
    holder: &SelfLoopHolder,
    target_areas: &mut PortRestoreTargetAreas,
) {
    for hyper_loop in holder
        .hyper_loops
        .iter()
        .filter(|hyper_loop| hyper_loop.self_loop_type == Some(SelfLoopType::TwoSidesCorner))
    {
        if let Some((start, end)) = sorted_two_side_loop_port_sides(graph, holder.node, hyper_loop)
        {
            target_areas.add_ports(
                graph,
                holder.node,
                hyper_loop,
                start,
                PortSideArea::End,
                AddMode::Prepend,
            );
            target_areas.add_ports(
                graph,
                holder.node,
                hyper_loop,
                end,
                PortSideArea::Start,
                AddMode::Append,
            );
        }
    }
}

fn process_two_side_opposing_loops(
    graph: &LGraph,
    holder: &SelfLoopHolder,
    target_areas: &mut PortRestoreTargetAreas,
) {
    for hyper_loop in holder
        .hyper_loops
        .iter()
        .filter(|hyper_loop| hyper_loop.self_loop_type == Some(SelfLoopType::TwoSidesOpposing))
    {
        if let Some((start, end)) = sorted_two_side_loop_port_sides(graph, holder.node, hyper_loop)
        {
            target_areas.add_ports(
                graph,
                holder.node,
                hyper_loop,
                start,
                PortSideArea::End,
                AddMode::Prepend,
            );
            target_areas.add_ports(
                graph,
                holder.node,
                hyper_loop,
                end,
                PortSideArea::Start,
                AddMode::Append,
            );
        }
    }
}

fn process_three_side_loops(
    graph: &LGraph,
    holder: &SelfLoopHolder,
    target_areas: &mut PortRestoreTargetAreas,
) {
    for hyper_loop in holder
        .hyper_loops
        .iter()
        .filter(|hyper_loop| hyper_loop.self_loop_type == Some(SelfLoopType::ThreeSides))
    {
        let Some((start, middle, end)) = three_side_restore_sides(graph, holder.node, hyper_loop)
        else {
            continue;
        };
        target_areas.add_ports(
            graph,
            holder.node,
            hyper_loop,
            start,
            PortSideArea::End,
            AddMode::Prepend,
        );
        target_areas.add_ports(
            graph,
            holder.node,
            hyper_loop,
            middle,
            PortSideArea::Middle,
            AddMode::Append,
        );
        target_areas.add_ports(
            graph,
            holder.node,
            hyper_loop,
            end,
            PortSideArea::Start,
            AddMode::Append,
        );
    }
}

fn process_four_side_loops(
    graph: &LGraph,
    holder: &SelfLoopHolder,
    target_areas: &mut PortRestoreTargetAreas,
) {
    for hyper_loop in holder
        .hyper_loops
        .iter()
        .filter(|hyper_loop| hyper_loop.self_loop_type == Some(SelfLoopType::FourSides))
    {
        for side in loop_sides(graph, holder.node, hyper_loop) {
            target_areas.add_ports(
                graph,
                holder.node,
                hyper_loop,
                side,
                PortSideArea::Middle,
                AddMode::Append,
            );
        }
    }
}

fn compute_port_list_split_index(
    hyper_loop: &SelfHyperLoop,
    sorted_ports: &[SelfLoopPort],
) -> usize {
    if sorted_ports.is_empty() {
        return 0;
    }

    let positive_net_flow_index = sorted_ports
        .iter()
        .position(|port| self_loop_port_net_flow(hyper_loop, port.port) > 0)
        .unwrap_or(sorted_ports.len());
    if positive_net_flow_index > 0 && positive_net_flow_index < sorted_ports.len() - 1 {
        return positive_net_flow_index;
    }

    let non_negative_net_flow_index = sorted_ports
        .iter()
        .position(|port| self_loop_port_net_flow(hyper_loop, port.port) > 0)
        .unwrap_or(sorted_ports.len());
    if non_negative_net_flow_index > 0 && non_negative_net_flow_index < sorted_ports.len() - 1 {
        return non_negative_net_flow_index;
    }

    sorted_ports.len() / 2
}

fn three_side_restore_sides(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
) -> Option<(PortSide, PortSide, PortSide)> {
    let sides = loop_sides(graph, node, hyper_loop);
    if sides.len() != 3 {
        return None;
    }

    if !sides.contains(&PortSide::North) {
        Some((PortSide::East, PortSide::South, PortSide::West))
    } else if !sides.contains(&PortSide::East) {
        Some((PortSide::South, PortSide::West, PortSide::North))
    } else if !sides.contains(&PortSide::South) {
        Some((PortSide::West, PortSide::North, PortSide::East))
    } else if !sides.contains(&PortSide::West) {
        Some((PortSide::North, PortSide::East, PortSide::South))
    } else {
        None
    }
}

pub(crate) fn restored_port_order(
    graph: &LGraph,
    others: OtherGraphs<'_>,
    node: NodeId,
    target_areas: &PortRestoreTargetAreas,
) -> Vec<PortId> {
    let hidden_ports = target_areas.ports();
    let old_ports = graph.layerless_nodes[node]
        .ports
        .ids()
        .filter(|port| !hidden_ports.contains(port))
        .collect::<Vec<_>>();
    let mut next_old_port_index = 0usize;
    let mut new_order = Vec::with_capacity(old_ports.len());

    add_target_area(
        target_areas,
        PortSide::North,
        PortSideArea::Start,
        &mut new_order,
    );
    next_old_port_index = add_all_that(
        graph,
        node,
        &old_ports,
        next_old_port_index,
        &mut new_order,
        |id, port| {
            port.side == PortSide::North
                && is_north_south_port_with_west_or_west_east_connections(graph, others, id, port)
        },
    );
    add_target_area(
        target_areas,
        PortSide::North,
        PortSideArea::Middle,
        &mut new_order,
    );
    next_old_port_index = add_all_that(
        graph,
        node,
        &old_ports,
        next_old_port_index,
        &mut new_order,
        |_, port| port.side == PortSide::North,
    );
    add_target_area(
        target_areas,
        PortSide::North,
        PortSideArea::End,
        &mut new_order,
    );

    add_target_area(
        target_areas,
        PortSide::East,
        PortSideArea::Start,
        &mut new_order,
    );
    add_target_area(
        target_areas,
        PortSide::East,
        PortSideArea::Middle,
        &mut new_order,
    );
    next_old_port_index = add_all_that(
        graph,
        node,
        &old_ports,
        next_old_port_index,
        &mut new_order,
        |_, port| port.side == PortSide::East,
    );
    add_target_area(
        target_areas,
        PortSide::East,
        PortSideArea::End,
        &mut new_order,
    );

    add_target_area(
        target_areas,
        PortSide::South,
        PortSideArea::Start,
        &mut new_order,
    );
    next_old_port_index = add_all_that(
        graph,
        node,
        &old_ports,
        next_old_port_index,
        &mut new_order,
        |id, port| {
            port.side == PortSide::South
                && is_north_south_port_with_east_connections(graph, others, id, port)
        },
    );
    add_target_area(
        target_areas,
        PortSide::South,
        PortSideArea::Middle,
        &mut new_order,
    );
    next_old_port_index = add_all_that(
        graph,
        node,
        &old_ports,
        next_old_port_index,
        &mut new_order,
        |_, port| port.side == PortSide::South,
    );
    add_target_area(
        target_areas,
        PortSide::South,
        PortSideArea::End,
        &mut new_order,
    );

    add_target_area(
        target_areas,
        PortSide::West,
        PortSideArea::Start,
        &mut new_order,
    );
    let _ = add_all_that(
        graph,
        node,
        &old_ports,
        next_old_port_index,
        &mut new_order,
        |_, port| port.side == PortSide::West,
    );
    add_target_area(
        target_areas,
        PortSide::West,
        PortSideArea::Middle,
        &mut new_order,
    );
    add_target_area(
        target_areas,
        PortSide::West,
        PortSideArea::End,
        &mut new_order,
    );

    for old_port in old_ports {
        if !new_order.contains(&old_port) {
            new_order.push(old_port);
        }
    }

    new_order
}

fn add_target_area(
    target_areas: &PortRestoreTargetAreas,
    side: PortSide,
    area: PortSideArea,
    new_order: &mut Vec<PortId>,
) {
    for port in target_areas.get(side, area) {
        if !new_order.contains(port) {
            new_order.push(*port);
        }
    }
}

fn add_all_that(
    graph: &LGraph,
    node: NodeId,
    old_ports: &[PortId],
    from_index: usize,
    new_order: &mut Vec<PortId>,
    condition: impl Fn(PortId, &crate::graph::LPort) -> bool,
) -> usize {
    for (index, old_port) in old_ports.iter().enumerate().skip(from_index) {
        let port = &graph.layerless_nodes[node].ports[*old_port];
        if !condition(*old_port, port) {
            return index;
        }
        if !new_order.contains(old_port) {
            new_order.push(*old_port);
        }
    }

    old_ports.len()
}

fn is_north_south_port_with_west_or_west_east_connections(
    graph: &LGraph,
    others: OtherGraphs<'_>,
    id: PortId,
    port: &crate::graph::LPort,
) -> bool {
    let connections = north_south_port_connection_sides(graph, others, id, port);
    connections.contains(&PortSide::West)
}

fn is_north_south_port_with_east_connections(
    graph: &LGraph,
    others: OtherGraphs<'_>,
    id: PortId,
    port: &crate::graph::LPort,
) -> bool {
    north_south_port_connection_sides(graph, others, id, port).contains(&PortSide::East)
}

fn north_south_port_connection_sides(
    graph: &LGraph,
    others: OtherGraphs<'_>,
    id: PortId,
    port: &crate::graph::LPort,
) -> Vec<PortSide> {
    let Some(dummy_ref) = port.port_dummy.as_ref() else {
        return Vec::new();
    };
    let Some(dummy_graph) = graph_by_id(graph, others, dummy_ref.graph_id) else {
        return Vec::new();
    };
    let Some(dummy_node) = dummy_graph.layerless_nodes.get(dummy_ref.node) else {
        return Vec::new();
    };

    // Only the ports of the dummy that were made for this port count:
    // `dummyLPort.getProperty(ORIGIN) == lPort` in Java.
    let origin = crate::graph::PortRef {
        node: port.node(),
        port: id,
    };
    let mut sides = dummy_node
        .ports
        .iter()
        .filter(|dummy_port| dummy_port.origin_port == Some(origin))
        .filter(|dummy_port| {
            !dummy_port.incoming_edges().is_empty() || !dummy_port.outgoing_edges().is_empty()
        })
        .map(|dummy_port| dummy_port.side)
        .collect::<Vec<_>>();
    sides.sort_by_key(|side| side.ordinal());
    sides.dedup();
    sides
}

/// The graph a dummy lives in: `graph` itself, or one of the graphs inside its nodes.
fn graph_by_id<'a>(graph: &'a LGraph, others: OtherGraphs<'a>, id: GraphId) -> Option<&'a LGraph> {
    if graph.id == id {
        Some(graph)
    } else {
        others.get(id)
    }
}

pub(crate) fn sorted_two_side_loop_port_sides(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
) -> Option<(PortSide, PortSide)> {
    let mut sides = loop_sides(graph, node, hyper_loop);
    if sides.len() != 2 {
        return None;
    }
    sides.sort_by_key(|side| side.ordinal());
    if sides[0] == PortSide::North && sides[1] == PortSide::West {
        Some((PortSide::West, PortSide::North))
    } else {
        Some((sides[0], sides[1]))
    }
}
