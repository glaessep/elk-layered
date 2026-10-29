//! Port of ELK's `LabelPlacer`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/routing/LabelPlacer.java

use crate::graph::{
    LGraph, NodeId, PortId, PortSide, SelfHyperLoop, SelfLoopHolder, SelfLoopLabelAlignment,
    SelfLoopType,
};

pub(crate) fn place_self_loop_labels(graph: &mut LGraph, holder: &mut SelfLoopHolder) {
    assign_label_side_and_alignment(graph, holder);
    for hyper_loop in &mut holder.hyper_loops {
        compute_label_coordinates(graph, holder.node, hyper_loop);
    }
}

fn assign_label_side_and_alignment(graph: &LGraph, holder: &mut SelfLoopHolder) {
    for hyper_loop in &mut holder.hyper_loops {
        if hyper_loop.labels.is_none() {
            continue;
        }

        let Some(loop_type) = hyper_loop.self_loop_type else {
            continue;
        };

        match loop_type {
            SelfLoopType::OneSide => {
                let Some(loop_side) = hyper_loop.occupied_sides.first().copied().or_else(|| {
                    hyper_loop
                        .ports
                        .first()
                        .map(|port| graph.layerless_nodes[holder.node].ports[port.port].side)
                }) else {
                    continue;
                };
                assign_one_sided_label_side_and_alignment(
                    graph,
                    holder.node,
                    hyper_loop,
                    loop_side,
                );
            }
            SelfLoopType::TwoSidesCorner => {
                assign_two_sides_corner_label_side_and_alignment(graph, holder.node, hyper_loop);
            }
            SelfLoopType::TwoSidesOpposing | SelfLoopType::ThreeSides => {
                assign_two_sides_opposing_and_three_sides_label_side_and_alignment(hyper_loop);
            }
            SelfLoopType::FourSides => {
                assign_four_sides_label_side_and_alignment(graph, holder.node, hyper_loop);
            }
        }
    }
}

fn assign_one_sided_label_side_and_alignment(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &mut SelfHyperLoop,
    loop_side: PortSide,
) {
    match loop_side {
        PortSide::East | PortSide::West => {
            let mut topmost_port = hyper_loop.leftmost_port;
            if let (Some(left), Some(right)) = (hyper_loop.leftmost_port, hyper_loop.rightmost_port)
                && graph.layerless_nodes[node].ports[right].position.y
                    < graph.layerless_nodes[node].ports[left].position.y
            {
                topmost_port = Some(right);
            }
            assign_label_side_and_alignment_to_loop(
                hyper_loop,
                loop_side,
                SelfLoopLabelAlignment::Top,
                topmost_port,
            );
        }
        PortSide::North | PortSide::South => {
            assign_label_side_and_alignment_to_loop(
                hyper_loop,
                loop_side,
                SelfLoopLabelAlignment::Center,
                None,
            );
        }
        PortSide::Undefined => {}
    }
}

fn assign_two_sides_corner_label_side_and_alignment(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &mut SelfHyperLoop,
) {
    let (Some(leftmost_port), Some(rightmost_port)) =
        (hyper_loop.leftmost_port, hyper_loop.rightmost_port)
    else {
        return;
    };
    let leftmost_side = graph.layerless_nodes[node].ports[leftmost_port].side;
    let rightmost_side = graph.layerless_nodes[node].ports[rightmost_port].side;

    if leftmost_side == PortSide::North {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::North,
            SelfLoopLabelAlignment::Left,
            Some(leftmost_port),
        );
    } else if rightmost_side == PortSide::North {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::North,
            SelfLoopLabelAlignment::Right,
            Some(rightmost_port),
        );
    } else if leftmost_side == PortSide::South {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::South,
            SelfLoopLabelAlignment::Right,
            Some(leftmost_port),
        );
    } else if rightmost_side == PortSide::South {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::South,
            SelfLoopLabelAlignment::Left,
            Some(rightmost_port),
        );
    }
}

fn assign_two_sides_opposing_and_three_sides_label_side_and_alignment(
    hyper_loop: &mut SelfHyperLoop,
) {
    if !hyper_loop.occupied_sides.contains(&PortSide::North) {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::South,
            SelfLoopLabelAlignment::Center,
            None,
        );
    } else if !hyper_loop.occupied_sides.contains(&PortSide::South) {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::North,
            SelfLoopLabelAlignment::Center,
            None,
        );
    } else if !hyper_loop.occupied_sides.contains(&PortSide::West) {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::North,
            SelfLoopLabelAlignment::Left,
            hyper_loop.leftmost_port,
        );
    } else if !hyper_loop.occupied_sides.contains(&PortSide::East) {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::North,
            SelfLoopLabelAlignment::Right,
            hyper_loop.rightmost_port,
        );
    }
}

/// Ports `LabelPlacer.assignFourSidesSideAndAlignment` at commit `62d5909`.
///
/// ELK reads the side of the leftmost port twice and never looks at the rightmost port: its
/// `rightmostPortSide` is `slLoop.getLeftmostPort().getLPort().getSide()`. elkjs 0.9.3 does the
/// same, so this does too.
fn assign_four_sides_label_side_and_alignment(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &mut SelfHyperLoop,
) {
    let Some(leftmost_port) = hyper_loop.leftmost_port else {
        return;
    };
    let leftmost_side = graph.layerless_nodes[node].ports[leftmost_port].side;

    if leftmost_side == PortSide::North {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::South,
            SelfLoopLabelAlignment::Center,
            None,
        );
    } else {
        assign_label_side_and_alignment_to_loop(
            hyper_loop,
            PortSide::North,
            SelfLoopLabelAlignment::Center,
            None,
        );
    }
}

fn assign_label_side_and_alignment_to_loop(
    hyper_loop: &mut SelfHyperLoop,
    side: PortSide,
    alignment: SelfLoopLabelAlignment,
    alignment_reference_port: Option<PortId>,
) {
    if let Some(labels) = &mut hyper_loop.labels {
        labels.side = Some(side);
        labels.alignment = Some(alignment);
        labels.alignment_reference_port = alignment_reference_port;
    }
}

fn compute_label_coordinates(graph: &LGraph, node: NodeId, hyper_loop: &mut SelfHyperLoop) {
    let Some(labels) = &mut hyper_loop.labels else {
        return;
    };
    let Some(alignment) = labels.alignment else {
        return;
    };

    match alignment {
        SelfLoopLabelAlignment::Center => {
            labels.position.x = (graph.layerless_nodes[node].size.width - labels.size.width) / 2.0;
        }
        SelfLoopLabelAlignment::Left => {
            if let Some(port) = labels.alignment_reference_port {
                let port_data = &graph.layerless_nodes[node].ports[port];
                labels.position.x = port_data.position.x + port_data.anchor.x;
            }
        }
        SelfLoopLabelAlignment::Right => {
            if let Some(port) = labels.alignment_reference_port {
                let port_data = &graph.layerless_nodes[node].ports[port];
                labels.position.x = port_data.position.x + port_data.anchor.x - labels.size.width;
            }
        }
        SelfLoopLabelAlignment::Top => {
            if let Some(port) = labels.alignment_reference_port {
                let port_data = &graph.layerless_nodes[node].ports[port];
                labels.position.y = port_data.position.y + port_data.anchor.y;
            }
        }
    }
}
